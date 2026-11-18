//! WebAuthn verification for the owner control panel.
//!
//! The owner proves who they are with the authenticator of the device they
//! are sitting at: Windows Hello on a computer, the platform authenticator on
//! a phone, or any other FIDO2 authenticator the browser exposes. The
//! browser performs the ceremony, but the browser is not trusted with the
//! decision: the assertion is verified here, against the public key that was
//! stored when the authenticator was enrolled.
//!
//! Everything in this module is pure data handling, so it can be exercised
//! by unit tests without a browser and without an authenticator.

use p256::ecdsa::{Signature, VerifyingKey, signature::Verifier};
use serde::Deserialize;
use sha2::{Digest, Sha256};

/// The user was present for the ceremony.
pub const FLAG_USER_PRESENT: u8 = 0x01;
/// The user was verified by the authenticator (a fingerprint, a face, a PIN).
pub const FLAG_USER_VERIFIED: u8 = 0x04;
/// The authenticator data carries attested credential data.
pub const FLAG_ATTESTED_CREDENTIAL_DATA: u8 = 0x40;

/// The COSE algorithm identifier for ECDSA with SHA-256 over P-256.
pub const ALGORITHM_ES256: i64 = -7;

/// The authenticator data of a ceremony.
#[derive(Debug, Clone)]
pub struct AuthenticatorData {
    pub rp_id_hash: [u8; 32],
    pub flags: u8,
    pub sign_count: u32,
    pub attested_credential: Option<AttestedCredential>,
}

/// The credential that was created during a registration ceremony.
#[derive(Debug, Clone)]
pub struct AttestedCredential {
    pub credential_id: Vec<u8>,
    /// The COSE encoded public key, kept so the raw enrolment stays auditable.
    pub public_key_cose: Vec<u8>,
}

/// The client data the browser signs over.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClientData {
    #[serde(rename = "type")]
    pub ceremony: String,
    pub challenge: String,
    pub origin: String,
}

/// Parse the authenticator data of a registration or assertion ceremony.
pub fn parse_authenticator_data(data: &[u8]) -> Result<AuthenticatorData, String> {
    if data.len() < 37 {
        return Err("WEBAUTHN_AUTHENTICATOR_DATA_TRUNCATED".to_string());
    }
    let mut rp_id_hash = [0u8; 32];
    rp_id_hash.copy_from_slice(&data[0..32]);
    let flags = data[32];
    let sign_count = u32::from_be_bytes([data[33], data[34], data[35], data[36]]);
    let mut attested_credential = None;
    if flags & FLAG_ATTESTED_CREDENTIAL_DATA != 0 {
        // attestedCredentialData = aaguid (16 bytes) || credentialIdLength
        // (2 bytes, big endian) || credentialId || credentialPublicKey (COSE).
        if data.len() < 55 {
            return Err("WEBAUTHN_ATTESTED_CREDENTIAL_TRUNCATED".to_string());
        }
        let credential_id_length = u16::from_be_bytes([data[53], data[54]]) as usize;
        let credential_start: usize = 55;
        let credential_end = credential_start
            .checked_add(credential_id_length)
            .ok_or_else(|| "WEBAUTHN_CREDENTIAL_LENGTH_OVERFLOW".to_string())?;
        if data.len() < credential_end {
            return Err("WEBAUTHN_CREDENTIAL_TRUNCATED".to_string());
        }
        attested_credential = Some(AttestedCredential {
            credential_id: data[credential_start..credential_end].to_vec(),
            public_key_cose: data[credential_end..].to_vec(),
        });
    }
    Ok(AuthenticatorData {
        rp_id_hash,
        flags,
        sign_count,
        attested_credential,
    })
}

/// The uncompressed P-256 public key of a credential.
#[derive(Debug, Clone)]
pub struct Es256PublicKey {
    pub x: [u8; 32],
    pub y: [u8; 32],
}

impl Es256PublicKey {
    /// The SEC1 uncompressed encoding of the key.
    pub fn to_sec1(&self) -> [u8; 65] {
        let mut point = [0u8; 65];
        point[0] = 0x04;
        point[1..33].copy_from_slice(&self.x);
        point[33..65].copy_from_slice(&self.y);
        point
    }

    /// The raw 64 byte form that is stored next to the COSE key.
    pub fn to_raw(&self) -> [u8; 64] {
        let mut raw = [0u8; 64];
        raw[0..32].copy_from_slice(&self.x);
        raw[32..64].copy_from_slice(&self.y);
        raw
    }
}

/// Read an integer out of a CBOR value.
fn integer_of(value: &ciborium::Value) -> Option<i128> {
    value.as_integer().map(|integer| integer.0)
}

/// Read the COSE encoded ES256 public key of a credential.
///
/// The COSE key is a CBOR map with integer labels: 1 is the key type, 3 the
/// algorithm, -1 the curve, -2 the X coordinate and -3 the Y coordinate.
pub fn parse_cose_es256_public_key(cose: &[u8]) -> Result<Es256PublicKey, String> {
    let document = ciborium::from_reader::<ciborium::Value, _>(cose)
        .map_err(|_| "WEBAUTHN_COSE_KEY_INVALID".to_string())?;
    let ciborium::Value::Map(entries) = document else {
        return Err("WEBAUTHN_COSE_KEY_INVALID".to_string());
    };
    let mut key_type: Option<i128> = None;
    let mut algorithm: Option<i128> = None;
    let mut curve: Option<i128> = None;
    let mut x: Option<Vec<u8>> = None;
    let mut y: Option<Vec<u8>> = None;
    for (label, value) in entries {
        let Some(index) = integer_of(&label) else {
            continue;
        };
        match index {
            1 => key_type = integer_of(&value),
            3 => algorithm = integer_of(&value),
            -1 => curve = integer_of(&value),
            -2 => x = value.as_bytes().cloned(),
            -3 => y = value.as_bytes().cloned(),
            _ => {}
        }
    }
    if key_type != Some(2) {
        return Err("WEBAUTHN_COSE_KEY_TYPE_UNSUPPORTED".to_string());
    }
    if algorithm != Some(ALGORITHM_ES256 as i128) {
        return Err("WEBAUTHN_COSE_ALGORITHM_UNSUPPORTED".to_string());
    }
    if curve != Some(1) {
        return Err("WEBAUTHN_COSE_CURVE_UNSUPPORTED".to_string());
    }
    let x = x.ok_or_else(|| "WEBAUTHN_COSE_KEY_INCOMPLETE".to_string())?;
    let y = y.ok_or_else(|| "WEBAUTHN_COSE_KEY_INCOMPLETE".to_string())?;
    if x.len() != 32 || y.len() != 32 {
        return Err("WEBAUTHN_COSE_COORDINATE_INVALID".to_string());
    }
    let mut x_bytes = [0u8; 32];
    let mut y_bytes = [0u8; 32];
    x_bytes.copy_from_slice(&x);
    y_bytes.copy_from_slice(&y);
    Ok(Es256PublicKey {
        x: x_bytes,
        y: y_bytes,
    })
}

/// Verify an ES256 signature over a message.
pub fn verify_es256(
    public_key: &Es256PublicKey,
    message: &[u8],
    signature: &[u8],
) -> Result<(), String> {
    let verifying_key = VerifyingKey::from_sec1_bytes(&public_key.to_sec1())
        .map_err(|_| "WEBAUTHN_PUBLIC_KEY_INVALID".to_string())?;
    // WebAuthn produces a DER encoded signature, but a raw 64 byte signature
    // is accepted as well so an authenticator that emits the fixed length
    // form still works.
    let signature = if signature.len() == 64 {
        Signature::from_slice(signature)
    } else {
        Signature::from_der(signature)
    }
    .map_err(|_| "WEBAUTHN_SIGNATURE_MALFORMED".to_string())?;
    verifying_key
        .verify(message, &signature)
        .map_err(|_| "WEBAUTHN_SIGNATURE_INVALID".to_string())
}

/// Check that the client data belongs to the ceremony that was started.
///
/// The challenge is the part that matters: it is a random value that only the
/// application and the authenticator saw, so a signature over a different
/// challenge cannot be replayed. The origin is checked as well, because the
/// panel must not be reachable from a page that merely looks like the
/// application.
pub fn verify_client_data(
    client_data: &ClientData,
    expected_ceremony: &str,
    expected_challenge: &str,
) -> Result<(), String> {
    if client_data.ceremony != expected_ceremony {
        return Err("WEBAUTHN_CEREMONY_MISMATCH".to_string());
    }
    if client_data.challenge != expected_challenge {
        return Err("WEBAUTHN_CHALLENGE_MISMATCH".to_string());
    }
    if !is_allowed_origin(&client_data.origin) {
        return Err("WEBAUTHN_ORIGIN_NOT_ALLOWED".to_string());
    }
    Ok(())
}

/// The origins the application is allowed to run under.
///
/// The desktop application is served from `tauri.localhost` on Windows and
/// from `localhost` on the other platforms and in development. A phone
/// browser or a web build of the panel uses `localhost` as well.
pub fn is_allowed_origin(origin: &str) -> bool {
    let Some((scheme, rest)) = origin.split_once("://") else {
        return false;
    };
    if !matches!(scheme, "http" | "https" | "tauri") {
        return false;
    }
    let host = rest.split('/').next().unwrap_or("");
    let host = host.split(':').next().unwrap_or("");
    matches!(host, "localhost" | "tauri.localhost")
}

/// The relying party identifier the ceremony is bound to.
///
/// The relying party has to be the origin the application is served from,
/// so the identifier is not a constant: the application asks for it and the
/// panel only accepts the origins it actually runs under.
pub fn is_allowed_rp_id(rp_id: &str) -> bool {
    matches!(rp_id, "localhost" | "tauri.localhost" | "127.0.0.1")
}

/// The relying party identifier used when nothing else is known.
pub fn default_rp_id() -> &'static str {
    "localhost"
}

/// The message an authenticator signs during an assertion.
pub fn assertion_message(authenticator_data: &[u8], client_data_json: &[u8]) -> Vec<u8> {
    let mut message = Vec::with_capacity(authenticator_data.len() + 32);
    message.extend_from_slice(authenticator_data);
    message.extend_from_slice(&Sha256::digest(client_data_json));
    message
}

/// The hash of the relying party identifier, as the authenticator data holds it.
pub fn rp_id_hash(rp_id: &str) -> [u8; 32] {
    let mut hash = [0u8; 32];
    hash.copy_from_slice(&Sha256::digest(rp_id.as_bytes()));
    hash
}

#[cfg(test)]
mod tests {
    use super::*;
    use p256::ecdsa::signature::Signer;

    /// Build an authenticator data buffer with attested credential data.
    fn attestation_authenticator_data(rp_id: &str, credential_id: &[u8], cose: &[u8]) -> Vec<u8> {
        let mut data = Vec::new();
        data.extend_from_slice(&rp_id_hash(rp_id));
        data.push(FLAG_USER_PRESENT | FLAG_USER_VERIFIED | FLAG_ATTESTED_CREDENTIAL_DATA);
        data.extend_from_slice(&0u32.to_be_bytes());
        data.extend_from_slice(&[0u8; 16]);
        data.extend_from_slice(&(credential_id.len() as u16).to_be_bytes());
        data.extend_from_slice(credential_id);
        data.extend_from_slice(cose);
        data
    }

    #[test]
    fn parses_attested_credential_data() {
        let cose = [0xa5, 0x01, 0x02, 0x03, 0x26, 0x20, 0x01, 0x21, 0x58, 0x20];
        let data = attestation_authenticator_data("localhost", &[9, 8, 7], &cose);
        let parsed = parse_authenticator_data(&data).expect("authenticator data parses");
        assert_eq!(parsed.rp_id_hash, rp_id_hash("localhost"));
        assert!(parsed.flags & FLAG_USER_PRESENT != 0);
        assert!(parsed.flags & FLAG_USER_VERIFIED != 0);
        let attested = parsed
            .attested_credential
            .expect("attested credential is present");
        assert_eq!(attested.credential_id, vec![9, 8, 7]);
        assert_eq!(attested.public_key_cose, cose.to_vec());
    }

    #[test]
    fn rejects_truncated_authenticator_data() {
        assert!(parse_authenticator_data(&[0u8; 10]).is_err());
    }

    #[test]
    fn accepts_only_the_local_origins() {
        assert!(is_allowed_origin("http://localhost:1420"));
        assert!(is_allowed_origin("http://tauri.localhost"));
        assert!(is_allowed_origin("https://localhost"));
        assert!(!is_allowed_origin("https://evil.example"));
        assert!(!is_allowed_origin("file:///etc/passwd"));
        assert!(!is_allowed_origin("localhost"));
    }

    #[test]
    fn rejects_a_mismatched_challenge() {
        let client = ClientData {
            ceremony: "webauthn.get".to_string(),
            challenge: "AAA".to_string(),
            origin: "http://localhost".to_string(),
        };
        assert!(
            verify_client_data(&client, "webauthn.get", "BBB").is_err(),
            "a signature over another challenge must be refused"
        );
        assert!(verify_client_data(&client, "webauthn.get", "AAA").is_ok());
        assert!(
            verify_client_data(&client, "webauthn.create", "AAA").is_err(),
            "the ceremony type must match the ceremony that was started"
        );
    }

    #[test]
    fn signs_and_verifies_an_es256_assertion() {
        // A fixed secret keeps the test vector deterministic.
        let signing_key = p256::ecdsa::SigningKey::from_bytes(&[7u8; 32].into())
            .expect("the signing key material is valid");
        let public_key = Es256PublicKey {
            x: signing_key
                .verifying_key()
                .to_encoded_point(false)
                .as_bytes()
                .get(1..33)
                .expect("x coordinate")
                .try_into()
                .expect("x coordinate is 32 bytes"),
            y: signing_key
                .verifying_key()
                .to_encoded_point(false)
                .as_bytes()
                .get(33..65)
                .expect("y coordinate")
                .try_into()
                .expect("y coordinate is 32 bytes"),
        };
        let message = b"authenticator data and client data hash";
        let signature: p256::ecdsa::Signature = signing_key.sign(message);
        assert!(verify_es256(&public_key, message, &signature.to_bytes()).is_ok());
        assert!(
            verify_es256(&public_key, b"another message", &signature.to_bytes()).is_err(),
            "a signature over another message must be refused"
        );
    }
}
