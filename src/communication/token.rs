// Authentication token operations, ported from mod/communication/token.py

use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;
use serde_json::json;

use crate::communication::{crypto, device};
use crate::settings::Settings;

/// Create an encrypted token request message from a nonce.
/// Signs the nonce with the device key, packages device info,
/// encrypts with the server's public key, and base64-encodes.
pub fn create_token_message(
    settings: &Settings,
    nonce: &str,
) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
    let device_key = device::get_device_key(settings)
        .ok_or("Missing device key")?;
    let server_key = device::get_server_key(settings)
        .ok_or("Missing API key")?;

    let signature = crypto::sign_message_sha1(&device_key, nonce)?;

    let data = json!({
        "nonce": nonce,
        "device_tag": device::get_tag(settings).unwrap_or_default(),
        "device_uid": device::get_uid(settings).unwrap_or_default(),
        "image_version": device::get_image_version(settings),
        "signature": BASE64.encode(&signature),
    });

    let data_str = serde_json::to_string(&data)?;
    let encrypted = crypto::encrypt(&server_key, &data_str)?;
    let encoded = BASE64.encode(&encrypted);

    Ok(json!({"message": encoded}))
}

/// Decode and decrypt a server response containing an access token.
/// Optionally verifies the server's signature for firmware >= 2.3.2.
pub fn decode_and_decrypt(
    settings: &Settings,
    message: &str,
) -> Result<String, Box<dyn std::error::Error>> {
    let data: serde_json::Value = serde_json::from_str(message)?;

    let encrypted_b64 = data["message"]
        .as_str()
        .ok_or("Missing 'message' field")?;
    let encrypted = BASE64.decode(encrypted_b64)?;

    let device_key = device::get_device_key(settings)
        .ok_or("Missing device key")?;
    let token = crypto::decrypt(&device_key, &encrypted)?;

    // Verify signature on newer versions (>= 2.3.2)
    if let Some(jwt_payload_b64) = token.split('.').nth(1) {
        let padded = format!("{}===", jwt_payload_b64);
        if let Ok(payload_bytes) = BASE64.decode(&padded) {
            if let Ok(jwt_payload) = serde_json::from_slice::<serde_json::Value>(&payload_bytes) {
                let version_str = jwt_payload
                    .get("version")
                    .and_then(|v| v.as_str())
                    .unwrap_or("2.2.2");
                let version_parts: Vec<i32> = version_str
                    .split('.')
                    .filter_map(|s| s.parse().ok())
                    .collect();

                if version_parts >= vec![2, 3, 2] {
                    let signature_b64 = data
                        .get("signature")
                        .and_then(|v| v.as_str())
                        .map(|s| s.trim())
                        .ok_or("Missing server signature")?;

                    if signature_b64.is_empty() {
                        return Err("Empty server signature".into());
                    }

                    let signature = BASE64.decode(signature_b64)?;
                    let server_key = device::get_server_key(settings)
                        .ok_or("Missing API key")?;

                    if !crypto::verify_signature(&server_key, encrypted_b64, &signature)? {
                        return Err("Server signature verification failed".into());
                    }
                }
            }
        }
    }

    Ok(token)
}
