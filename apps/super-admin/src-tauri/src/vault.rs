//! The signing key vault of the developer workstation.
//!
//! The vault holds the Ed25519 seed that signs customer licences. The seed
//! never leaves this file in the clear and it never leaves the workstation:
//!
//! - the seed is encrypted with AES-256-GCM under a random data key,
//! - the data key is wrapped twice, once with a key that Argon2id derives
//!   from the passphrase of the operator, and once with Windows DPAPI in the
//!   scope of the Windows account that created the vault,
//! - the data key only exists in memory while the vault is unlocked.
//!
//! The DPAPI wrap is what binds the vault to the developer machine: a copy of
//! `vault.bin` on another account or another computer cannot be opened, and
//! the passphrase alone is not enough without that Windows account.
//!
//! The vault belongs to this application only. The customer application
//! carries the public key and nothing else, so a copy of the POS installer
//! can never be turned into a licence signer.

use crate::dpapi;
use aes_gcm::{
    Aes256Gcm, KeyInit, Nonce,
    aead::{Aead, Payload},
};
use argon2::{Algorithm, Argon2, Params, Version};
use ed25519_dalek::SigningKey;
use std::{fs, path::Path};
use zeroize::Zeroizing;

/// The marker at the start of a vault file.
const MAGIC: &[u8; 8] = b"NSJVAULT";
/// The format version of the vault file.
const VERSION: u32 = 2;
/// The format version that was written before the DPAPI wrap existed.
const LEGACY_VERSION: u32 = 1;
const SALT_LEN: usize = 16;
const NONCE_LEN: usize = 12;
const KEY_LEN: usize = 32;
const SEED_LEN: usize = 32;

/// The smallest passphrase the vault accepts.
const MIN_PASSPHRASE: usize = 12;

/// Derive the encryption key of the vault from the passphrase.
fn derive_key(passphrase: &str, salt: &[u8]) -> Result<Zeroizing<[u8; KEY_LEN]>, String> {
    let params =
        Params::new(64 * 1024, 3, 1, None).map_err(|_| "ARGON2_CONFIG_INVALID".to_string())?;
    let argon2 = Argon2::new(Algorithm::Argon2id, Version::V0x13, params);
    let mut key = Zeroizing::new([0u8; KEY_LEN]);
    argon2
        .hash_password_into(passphrase.as_bytes(), salt, key.as_mut())
        .map_err(|_| "ARGON2_FAILED".to_string())?;
    Ok(key)
}

/// Read a part of the vault file and move the cursor over it.
fn read_part<'a>(bytes: &'a [u8], cursor: &mut usize, length: usize) -> Result<&'a [u8], String> {
    let end = cursor
        .checked_add(length)
        .ok_or_else(|| "VAULT_FILE_INVALID".to_string())?;
    if end > bytes.len() {
        return Err("VAULT_FILE_INVALID".to_string());
    }
    let part = &bytes[*cursor..end];
    *cursor = end;
    Ok(part)
}

/// Read a length that is written as four bytes.
fn read_length(bytes: &[u8], cursor: &mut usize) -> Result<usize, String> {
    let raw: [u8; 4] = read_part(bytes, cursor, 4)?
        .try_into()
        .map_err(|_| "VAULT_FILE_INVALID".to_string())?;
    Ok(u32::from_le_bytes(raw) as usize)
}

/// Write the vault file with the data key wrapped for the passphrase and for
/// the Windows account.
fn write_vault(
    path: &Path,
    seed: &Zeroizing<[u8; SEED_LEN]>,
    passphrase: &str,
) -> Result<(), String> {
    let mut salt = [0u8; SALT_LEN];
    let mut nonce = [0u8; NONCE_LEN];
    getrandom::fill(&mut salt).map_err(|_| "CSPRNG_FAILED".to_string())?;
    getrandom::fill(&mut nonce).map_err(|_| "CSPRNG_FAILED".to_string())?;
    let mut data_key = Zeroizing::new([0u8; KEY_LEN]);
    getrandom::fill(data_key.as_mut()).map_err(|_| "CSPRNG_FAILED".to_string())?;

    let passphrase_key = derive_key(passphrase, &salt)?;
    let passphrase_cipher = Aes256Gcm::new(passphrase_key.as_ref().into());
    let wrapped_key = passphrase_cipher
        .encrypt(
            Nonce::from_slice(&nonce),
            Payload {
                msg: data_key.as_ref(),
                aad: MAGIC,
            },
        )
        .map_err(|_| "VAULT_ENCRYPT_FAILED".to_string())?;

    // The Windows account of the developer is the second half of the lock.
    let account_key = dpapi::protect(data_key.as_ref())?;

    let seed_cipher = Aes256Gcm::new(data_key.as_ref().into());
    let ciphertext = seed_cipher
        .encrypt(
            Nonce::from_slice(&nonce),
            Payload {
                msg: seed.as_ref(),
                aad: MAGIC,
            },
        )
        .map_err(|_| "VAULT_ENCRYPT_FAILED".to_string())?;

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    let mut file = Vec::with_capacity(
        MAGIC.len()
            + 4
            + SALT_LEN
            + NONCE_LEN
            + wrapped_key.len()
            + account_key.len()
            + ciphertext.len(),
    );
    file.extend_from_slice(MAGIC);
    file.extend_from_slice(&VERSION.to_le_bytes());
    file.extend_from_slice(&salt);
    file.extend_from_slice(&nonce);
    file.extend_from_slice(&(wrapped_key.len() as u32).to_le_bytes());
    file.extend_from_slice(&wrapped_key);
    file.extend_from_slice(&(account_key.len() as u32).to_le_bytes());
    file.extend_from_slice(&account_key);
    file.extend_from_slice(&ciphertext);
    let temp = path.with_extension("tmp");
    fs::write(&temp, &file).map_err(|error| error.to_string())?;
    fs::rename(&temp, path).map_err(|error| error.to_string())?;
    Ok(())
}

/// Create a vault file that holds the given seed.
pub fn create(
    path: &Path,
    seed: &Zeroizing<[u8; SEED_LEN]>,
    passphrase: &str,
) -> Result<(), String> {
    if passphrase.len() < MIN_PASSPHRASE {
        return Err(format!(
            "the vault passphrase must be at least {MIN_PASSPHRASE} characters"
        ));
    }
    write_vault(path, seed, passphrase)
}

/// Unlock the vault and return the seed it holds.
pub fn unlock(path: &Path, passphrase: &str) -> Result<Zeroizing<[u8; SEED_LEN]>, String> {
    let bytes = fs::read(path).map_err(|_| "VAULT_FILE_MISSING".to_string())?;
    if bytes.len() < MAGIC.len() + 4 {
        return Err("VAULT_FILE_INVALID".to_string());
    }
    if &bytes[0..MAGIC.len()] != MAGIC {
        return Err("VAULT_FILE_INVALID".to_string());
    }
    let version = u32::from_le_bytes([bytes[8], bytes[9], bytes[10], bytes[11]]);
    match version {
        VERSION => unlock_v2(&bytes, passphrase),
        LEGACY_VERSION => {
            let seed = unlock_legacy(&bytes, passphrase)?;
            // The vault of the older format is re-wrapped in the format that
            // is bound to the Windows account, so it is protected from now on.
            write_vault(path, &seed, passphrase)?;
            Ok(seed)
        }
        _ => Err("VAULT_VERSION_UNSUPPORTED".to_string()),
    }
}

/// Open a vault that carries the DPAPI wrap.
fn unlock_v2(bytes: &[u8], passphrase: &str) -> Result<Zeroizing<[u8; SEED_LEN]>, String> {
    let mut cursor = MAGIC.len() + 4;
    let salt = read_part(bytes, &mut cursor, SALT_LEN)?;
    let nonce = read_part(bytes, &mut cursor, NONCE_LEN)?;
    let wrapped_length = read_length(bytes, &mut cursor)?;
    let wrapped_key = read_part(bytes, &mut cursor, wrapped_length)?;
    let account_length = read_length(bytes, &mut cursor)?;
    let account_key = read_part(bytes, &mut cursor, account_length)?;
    let remaining = bytes.len() - cursor;
    let ciphertext = read_part(bytes, &mut cursor, remaining)?;

    let passphrase_key = derive_key(passphrase, salt)?;
    let passphrase_cipher = Aes256Gcm::new(passphrase_key.as_ref().into());
    let unwrapped = passphrase_cipher
        .decrypt(
            Nonce::from_slice(nonce),
            Payload {
                msg: wrapped_key,
                aad: MAGIC,
            },
        )
        .map_err(|_| "VAULT_PASSPHRASE_WRONG".to_string())?;
    if unwrapped.len() != KEY_LEN {
        return Err("VAULT_FILE_INVALID".to_string());
    }

    // The account of the developer has to be the one that wrapped the key.
    let account_unwrapped = dpapi::unprotect(account_key)?;
    if account_unwrapped != unwrapped {
        return Err("VAULT_USER_MISMATCH".to_string());
    }

    let mut data_key = Zeroizing::new([0u8; KEY_LEN]);
    data_key.copy_from_slice(&unwrapped);
    let seed_cipher = Aes256Gcm::new(data_key.as_ref().into());
    let plain = seed_cipher
        .decrypt(
            Nonce::from_slice(nonce),
            Payload {
                msg: ciphertext,
                aad: MAGIC,
            },
        )
        .map_err(|_| "VAULT_FILE_INVALID".to_string())?;
    if plain.len() != SEED_LEN {
        return Err("VAULT_FILE_INVALID".to_string());
    }
    let mut seed = Zeroizing::new([0u8; SEED_LEN]);
    seed.copy_from_slice(&plain);
    Ok(seed)
}

/// Open a vault that was written before the DPAPI wrap existed.
fn unlock_legacy(bytes: &[u8], passphrase: &str) -> Result<Zeroizing<[u8; SEED_LEN]>, String> {
    let mut cursor = MAGIC.len() + 4;
    let salt = read_part(bytes, &mut cursor, SALT_LEN)?;
    let nonce = read_part(bytes, &mut cursor, NONCE_LEN)?;
    let remaining = bytes.len() - cursor;
    let ciphertext = read_part(bytes, &mut cursor, remaining)?;
    let passphrase_key = derive_key(passphrase, salt)?;
    let cipher = Aes256Gcm::new(passphrase_key.as_ref().into());
    let plain = cipher
        .decrypt(
            Nonce::from_slice(nonce),
            Payload {
                msg: ciphertext,
                aad: MAGIC,
            },
        )
        .map_err(|_| "VAULT_PASSPHRASE_WRONG".to_string())?;
    if plain.len() != SEED_LEN {
        return Err("VAULT_FILE_INVALID".to_string());
    }
    let mut seed = Zeroizing::new([0u8; SEED_LEN]);
    seed.copy_from_slice(&plain);
    Ok(seed)
}

/// The public key that belongs to a seed.
pub fn public_key_hex(seed: &Zeroizing<[u8; SEED_LEN]>) -> String {
    let signing_key = SigningKey::from_bytes(seed);
    hex::encode(signing_key.verifying_key().to_bytes())
}

/// Read a seed from its hexadecimal form.
pub fn seed_from_hex(value: &str) -> Result<Zeroizing<[u8; SEED_LEN]>, String> {
    let trimmed = value.trim();
    if trimmed.len() != SEED_LEN * 2 {
        return Err("the seed must be 32 bytes written as 64 hexadecimal characters".to_string());
    }
    let bytes = hex::decode(trimmed).map_err(|_| "the seed is not hexadecimal".to_string())?;
    let mut seed = Zeroizing::new([0u8; SEED_LEN]);
    seed.copy_from_slice(&bytes);
    Ok(seed)
}

/// Generate a new random seed.
pub fn generate_seed() -> Result<Zeroizing<[u8; SEED_LEN]>, String> {
    let mut seed = Zeroizing::new([0u8; SEED_LEN]);
    getrandom::fill(seed.as_mut()).map_err(|_| "CSPRNG_FAILED".to_string())?;
    Ok(seed)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_file(label: &str) -> std::path::PathBuf {
        let directory =
            std::env::temp_dir().join(format!("naseej-vault-{label}-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&directory).expect("temp directory");
        directory.join("vault.bin")
    }

    #[test]
    fn a_short_passphrase_is_refused() {
        let path = temp_file("short-passphrase");
        let seed = generate_seed().expect("seed");
        let result = create(&path, &seed, "short");
        assert!(result.is_err(), "a short passphrase must be refused");
        fs::remove_dir_all(path.parent().unwrap()).ok();
    }

    #[test]
    fn the_vault_needs_windows_dpapi() {
        let path = temp_file("dpapi-availability");
        let seed = generate_seed().expect("seed");
        let result = create(&path, &seed, "a long enough passphrase");
        if cfg!(windows) {
            assert!(result.is_ok(), "the vault is created on Windows");
            assert!(unlock(&path, "a long enough passphrase").is_ok());
        } else {
            assert_eq!(
                result.err(),
                Some("VAULT_DPAPI_UNAVAILABLE".to_string()),
                "the vault stays closed where DPAPI does not exist"
            );
        }
        fs::remove_dir_all(path.parent().unwrap()).ok();
    }

    #[cfg(windows)]
    #[test]
    fn a_vault_round_trips_its_seed() {
        let path = temp_file("round-trip");
        let seed = generate_seed().expect("seed");
        create(&path, &seed, "a long enough passphrase").expect("vault is created");
        let unlocked = unlock(&path, "a long enough passphrase").expect("vault opens");
        assert_eq!(unlocked.as_ref(), seed.as_ref());
        assert_eq!(public_key_hex(&unlocked), public_key_hex(&seed));
        fs::remove_dir_all(path.parent().unwrap()).ok();
    }

    #[cfg(windows)]
    #[test]
    fn a_wrong_passphrase_is_refused() {
        let path = temp_file("wrong-passphrase");
        let seed = generate_seed().expect("seed");
        create(&path, &seed, "a long enough passphrase").expect("vault is created");
        let result = unlock(&path, "another passphrase entirely");
        assert!(
            result.is_err(),
            "a wrong passphrase must not open the vault"
        );
        fs::remove_dir_all(path.parent().unwrap()).ok();
    }

    #[cfg(windows)]
    #[test]
    fn a_tampered_vault_is_refused() {
        let path = temp_file("tampered");
        let seed = generate_seed().expect("seed");
        create(&path, &seed, "a long enough passphrase").expect("vault is created");
        let mut bytes = fs::read(&path).expect("vault file");
        let last = bytes.len() - 1;
        bytes[last] ^= 0xff;
        fs::write(&path, &bytes).expect("write");
        let result = unlock(&path, "a long enough passphrase");
        assert!(result.is_err(), "a tampered vault must not open");
        fs::remove_dir_all(path.parent().unwrap()).ok();
    }
}
