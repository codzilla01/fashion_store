/**
 * The browser half of the owner panel ceremonies.
 *
 * The browser performs the WebAuthn ceremony and hands the result to the
 * application, which verifies it. Nothing here decides anything: a ceremony
 * that the application does not accept leaves the panel locked.
 *
 * The ceremony works with the platform authenticator of the device, which is
 * Windows Hello on a computer and the fingerprint or face unlock of a phone.
 */

const textEncoder = new TextEncoder();

/** Decode a base64url value into bytes. */
export function base64UrlToBytes(value: string): Uint8Array<ArrayBuffer> {
  const normalized = value.replace(/-/g, '+').replace(/_/g, '/');
  const padded = normalized.padEnd(Math.ceil(normalized.length / 4) * 4, '=');
  const binary = atob(padded);
  const bytes = new Uint8Array(binary.length);
  for (let index = 0; index < binary.length; index += 1) {
    bytes[index] = binary.charCodeAt(index);
  }
  return bytes;
}

/** Encode text into bytes. */
export function textToBytes(value: string): Uint8Array<ArrayBuffer> {
  const encoded = textEncoder.encode(value);
  const bytes = new Uint8Array(encoded.length);
  bytes.set(encoded);
  return bytes;
}

/** Encode bytes into a base64url value. */
export function bytesToBase64Url(buffer: ArrayBuffer): string {
  const bytes = new Uint8Array(buffer);
  let binary = '';
  for (let index = 0; index < bytes.length; index += 1) {
    binary += String.fromCharCode(bytes[index]);
  }
  return btoa(binary).replace(/\+/g, '-').replace(/\//g, '_').replace(/=+$/, '');
}

/** Whether the browser knows the WebAuthn API at all. */
export function webauthnSupported(): boolean {
  return typeof window !== 'undefined'
    && typeof (window as Window & { PublicKeyCredential?: unknown }).PublicKeyCredential !== 'undefined'
    && typeof navigator.credentials?.create === 'function'
    && typeof navigator.credentials?.get === 'function';
}

/**
 * Whether the device has a platform authenticator that can verify the
 * person, such as Windows Hello or a phone fingerprint.
 */
export async function platformAuthenticatorAvailable(): Promise<boolean> {
  if (!webauthnSupported()) return false;
  const credential = (window as Window & {
    PublicKeyCredential?: {
      isUserVerifyingPlatformAuthenticatorAvailable?: () => Promise<boolean>;
    };
  }).PublicKeyCredential;
  if (!credential?.isUserVerifyingPlatformAuthenticatorAvailable) return false;
  try {
    return await credential.isUserVerifyingPlatformAuthenticatorAvailable();
  } catch {
    return false;
  }
}

/** The origin the application is served from, used as the relying party. */
export function currentRpId(): string {
  if (typeof window === 'undefined') return 'localhost';
  return window.location.hostname || 'localhost';
}

type RawAttestation = {
  id: string;
  rawId: ArrayBuffer;
  response: { attestationObject: ArrayBuffer; clientDataJSON: ArrayBuffer };
};

type RawAssertion = {
  id: string;
  rawId: ArrayBuffer;
  response: { authenticatorData: ArrayBuffer; clientDataJSON: ArrayBuffer; signature: ArrayBuffer };
};

export type RegistrationResult = {
  credentialId: string;
  attestationObject: string;
  clientDataJson: string;
};

export type AssertionResult = {
  credentialId: string;
  authenticatorData: string;
  clientDataJson: string;
  signature: string;
};

/** Register the platform authenticator of this device. */
export async function registerPlatformAuthenticator(options: {
  challenge: string;
  rpId: string;
  userId: string;
  userName: string;
}): Promise<RegistrationResult> {
  if (!webauthnSupported()) {
    throw new Error('WEBAUTHN_NOT_SUPPORTED');
  }
  const credential = (await navigator.credentials.create({
    publicKey: {
      challenge: base64UrlToBytes(options.challenge),
      rp: { id: options.rpId, name: 'Naseej POS' },
      user: {
        id: textToBytes(options.userId).slice(0, 64),
        name: options.userName,
        displayName: options.userName,
      },
      pubKeyCredParams: [{ type: 'public-key', alg: -7 }],
      authenticatorSelection: {
        authenticatorAttachment: 'platform',
        userVerification: 'required',
        residentKey: 'required',
        requireResidentKey: true,
      },
      attestation: 'none',
      timeout: 120000,
    },
  })) as unknown as RawAttestation | null;
  if (!credential) {
    throw new Error('WEBAUTHN_CANCELLED');
  }
  return {
    credentialId: bytesToBase64Url(credential.rawId),
    attestationObject: bytesToBase64Url(credential.response.attestationObject),
    clientDataJson: bytesToBase64Url(credential.response.clientDataJSON),
  };
}

/** Ask the platform authenticator to verify the person on this device. */
export async function getPlatformAssertion(options: {
  challenge: string;
  rpId: string;
  credentialIds: string[];
}): Promise<AssertionResult> {
  if (!webauthnSupported()) {
    throw new Error('WEBAUTHN_NOT_SUPPORTED');
  }
  const assertion = (await navigator.credentials.get({
    publicKey: {
      challenge: base64UrlToBytes(options.challenge),
      rpId: options.rpId,
      userVerification: 'required',
      allowCredentials: options.credentialIds.map((id) => ({
        type: 'public-key',
        id: base64UrlToBytes(id),
      })),
      timeout: 120000,
    },
  })) as unknown as RawAssertion | null;
  if (!assertion) {
    throw new Error('WEBAUTHN_CANCELLED');
  }
  return {
    credentialId: bytesToBase64Url(assertion.rawId),
    authenticatorData: bytesToBase64Url(assertion.response.authenticatorData),
    clientDataJson: bytesToBase64Url(assertion.response.clientDataJSON),
    signature: bytesToBase64Url(assertion.response.signature),
  };
}
