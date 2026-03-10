// Cryptographic operations, ported from mod/communication/crypto.py
// Hybrid RSA+AES encryption, SHA1 signatures

use rsa::pkcs1v15::SigningKey;
use rsa::pkcs8::DecodePrivateKey;
use rsa::signature::SignatureEncoding;
use rsa::signature::Signer;
use rsa::traits::PublicKeyParts;
use rsa::Oaep;
use rsa::RsaPrivateKey;
use rsa::RsaPublicKey;
use sha1::Sha1;

fn import_public_key(pem: &str) -> Result<RsaPublicKey, Box<dyn std::error::Error>> {
    use rsa::pkcs8::DecodePublicKey;
    // Try PKCS#8 public key first, then try extracting from private key
    if let Ok(key) = RsaPublicKey::from_public_key_pem(pem) {
        return Ok(key);
    }
    if let Ok(priv_key) = RsaPrivateKey::from_pkcs8_pem(pem) {
        return Ok(priv_key.into());
    }
    // Try PKCS#1 formats
    use rsa::pkcs1::{DecodeRsaPrivateKey, DecodeRsaPublicKey};
    if let Ok(key) = RsaPublicKey::from_pkcs1_pem(pem) {
        return Ok(key);
    }
    if let Ok(priv_key) = RsaPrivateKey::from_pkcs1_pem(pem) {
        return Ok(priv_key.into());
    }
    Err("Failed to import public key from PEM".into())
}

fn import_private_key(pem: &str) -> Result<RsaPrivateKey, Box<dyn std::error::Error>> {
    use rsa::pkcs1::DecodeRsaPrivateKey;
    if let Ok(key) = RsaPrivateKey::from_pkcs8_pem(pem) {
        return Ok(key);
    }
    if let Ok(key) = RsaPrivateKey::from_pkcs1_pem(pem) {
        return Ok(key);
    }
    Err("Failed to import private key from PEM".into())
}

/// Encrypt data using hybrid RSA+AES-EAX scheme.
/// 1. Generate random 16-byte AES session key
/// 2. Encrypt session key with recipient's RSA public key (OAEP)
/// 3. Encrypt data with AES-EAX using random nonce
/// 4. Output: [RSA-encrypted session key | nonce(16) | tag(16) | ciphertext]
pub fn encrypt(
    recipient_key_pem: &str,
    data: &str,
) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    use aes::Aes128;
    use eax::aead::{Aead, KeyInit};
    use eax::Eax;
    use rand::RngCore;

    let recipient_key = import_public_key(recipient_key_pem)?;

    // Generate 16-byte session key
    let mut session_key = [0u8; 16];
    rand::thread_rng().fill_bytes(&mut session_key);

    // RSA-OAEP encrypt the session key
    let mut rng = rand::thread_rng();
    let encrypted_session_key =
        recipient_key.encrypt(&mut rng, Oaep::new::<Sha1>(), &session_key)?;

    // Generate 16-byte nonce for AES-EAX
    let mut nonce_bytes = [0u8; 16];
    rand::thread_rng().fill_bytes(&mut nonce_bytes);

    // AES-EAX encrypt the data
    let cipher = Eax::<Aes128>::new((&session_key).into());
    let nonce = eax::aead::generic_array::GenericArray::from_slice(&nonce_bytes);
    let ciphertext_with_tag = cipher
        .encrypt(nonce, data.as_bytes())
        .map_err(|e| format!("AES-EAX encrypt error: {}", e))?;

    // EAX appends the tag to the ciphertext; Python format is nonce|tag|ciphertext
    // eax crate output: ciphertext || tag (tag is last 16 bytes)
    let ct_len = ciphertext_with_tag.len() - 16;
    let ciphertext = &ciphertext_with_tag[..ct_len];
    let tag = &ciphertext_with_tag[ct_len..];

    let mut out = Vec::with_capacity(
        encrypted_session_key.len() + 16 + 16 + ciphertext.len(),
    );
    out.extend_from_slice(&encrypted_session_key);
    out.extend_from_slice(&nonce_bytes);
    out.extend_from_slice(tag);
    out.extend_from_slice(ciphertext);

    Ok(out)
}

/// Decrypt data encrypted with the hybrid RSA+AES-EAX scheme.
pub fn decrypt(
    private_key_pem: &str,
    encrypted: &[u8],
) -> Result<String, Box<dyn std::error::Error>> {
    use aes::Aes128;
    use eax::aead::{Aead, KeyInit};
    use eax::Eax;

    let private_key = import_private_key(private_key_pem)?;
    let key_size = private_key.n().bits() / 8; // key size in bytes

    if encrypted.len() < key_size + 32 {
        return Err("Encrypted data too short".into());
    }

    let encrypted_session_key = &encrypted[..key_size];
    let nonce_bytes = &encrypted[key_size..key_size + 16];
    let tag = &encrypted[key_size + 16..key_size + 32];
    let ciphertext = &encrypted[key_size + 32..];

    // RSA-OAEP decrypt the session key
    let session_key = private_key.decrypt(Oaep::new::<Sha1>(), encrypted_session_key)?;

    // AES-EAX decrypt the data
    // eax crate expects ciphertext || tag
    let mut ct_with_tag = Vec::with_capacity(ciphertext.len() + 16);
    ct_with_tag.extend_from_slice(ciphertext);
    ct_with_tag.extend_from_slice(tag);

    let cipher = Eax::<Aes128>::new((&session_key[..]).into());
    let nonce = eax::aead::generic_array::GenericArray::from_slice(nonce_bytes);
    let plaintext = cipher
        .decrypt(nonce, ct_with_tag.as_slice())
        .map_err(|e| format!("AES-EAX decrypt error: {}", e))?;

    Ok(String::from_utf8(plaintext)?)
}

/// Sign a message using RSA PKCS#1 v1.5 with SHA-1.
pub fn sign_message_sha1(
    key_pem: &str,
    message: &str,
) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let private_key = import_private_key(key_pem)?;
    let signing_key = SigningKey::<Sha1>::new(private_key);
    let signature = signing_key.sign(message.as_bytes());
    Ok(signature.to_bytes().to_vec())
}

/// Verify a RSA PKCS#1 v1.5 SHA-1 signature.
pub fn verify_signature(
    sender_key_pem: &str,
    contents: &str,
    signature: &[u8],
) -> Result<bool, Box<dyn std::error::Error>> {
    use rsa::pkcs1v15::VerifyingKey;
    use rsa::signature::Verifier;

    let public_key = import_public_key(sender_key_pem)?;
    let verifying_key = VerifyingKey::<Sha1>::new(public_key);
    let sig = rsa::pkcs1v15::Signature::try_from(signature)?;
    match verifying_key.verify(contents.as_bytes(), &sig) {
        Ok(()) => Ok(true),
        Err(_) => Ok(false),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn generate_test_keypair() -> (String, String) {
        use rsa::pkcs8::{EncodePrivateKey, EncodePublicKey};
        let mut rng = rand::thread_rng();
        let private_key = RsaPrivateKey::new(&mut rng, 2048).unwrap();
        let public_key: RsaPublicKey = private_key.clone().into();
        let priv_pem = private_key.to_pkcs8_pem(rsa::pkcs8::LineEnding::LF).unwrap();
        let pub_pem = public_key.to_public_key_pem(rsa::pkcs8::LineEnding::LF).unwrap();
        (priv_pem.to_string(), pub_pem)
    }

    #[test]
    fn test_encrypt_decrypt_roundtrip() {
        let (priv_pem, pub_pem) = generate_test_keypair();
        let plaintext = "Hello, MOD Audio!";
        let encrypted = encrypt(&pub_pem, plaintext).unwrap();
        let decrypted = decrypt(&priv_pem, &encrypted).unwrap();
        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn test_sign_verify() {
        let (priv_pem, pub_pem) = generate_test_keypair();
        let message = "test-nonce-12345";
        let signature = sign_message_sha1(&priv_pem, message).unwrap();
        assert!(verify_signature(&pub_pem, message, &signature).unwrap());
        assert!(!verify_signature(&pub_pem, "wrong message", &signature).unwrap());
    }
}
