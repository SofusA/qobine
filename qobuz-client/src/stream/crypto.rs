use aes::cipher::{BlockModeDecrypt, KeyIvInit, StreamCipher};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use hkdf::{GenericHkdf, hmac::Hmac};
use sha2::Sha256;

use crate::Error;

type Aes128CbcDec = cbc::Decryptor<aes::Aes128>;
type Aes128Ctr = ctr::Ctr128BE<aes::Aes128>;
type HkdfSha256 = GenericHkdf<Hmac<Sha256>>;

const RNG_INIT: &str = "abb21364945c0583309667d13ca3d93a";

/// Derive the 16-byte session key from the session/start `infos` field.
///
/// infos format: "salt_b64url.info_b64url"
/// IKM = hex-decoded rng_init (16 bytes)
pub fn derive_session_key(infos: &str) -> Result<[u8; 16], Error> {
    let parts: Vec<&str> = infos.split('.').collect();
    if parts.len() < 2 {
        return Err(Error::StreamError {
            message: "session infos must have at least 2 dot-separated parts".into(),
        });
    }

    let salt = URL_SAFE_NO_PAD
        .decode(parts[0])
        .map_err(|e| Error::StreamError {
            message: format!("failed to decode session salt: {e}"),
        })?;

    let info = URL_SAFE_NO_PAD
        .decode(parts[1])
        .map_err(|e| Error::StreamError {
            message: format!("failed to decode session info: {e}"),
        })?;

    let ikm = hex_decode(RNG_INIT)?;

    let hk = HkdfSha256::new(Some(&salt), &ikm);
    let mut okm = [0u8; 16];
    hk.expand(&info, &mut okm).map_err(|e| Error::StreamError {
        message: format!("HKDF expand failed: {e}"),
    })?;

    Ok(okm)
}

/// Unwrap the per-track content key using the session key.
///
/// key_str format: "qbz-1.wrapped_key_b64url.iv_b64url"
pub fn unwrap_content_key(session_key: &[u8; 16], key_str: &str) -> Result<[u8; 16], Error> {
    let parts: Vec<&str> = key_str.split('.').collect();
    if parts.len() < 3 {
        return Err(Error::StreamError {
            message: "key string must have at least 3 dot-separated parts".into(),
        });
    }

    let wrapped = URL_SAFE_NO_PAD
        .decode(parts[1])
        .map_err(|e| Error::StreamError {
            message: format!("failed to decode wrapped key: {e}"),
        })?;

    let iv = URL_SAFE_NO_PAD
        .decode(parts[2])
        .map_err(|e| Error::StreamError {
            message: format!("failed to decode unwrap IV: {e}"),
        })?;

    if iv.len() != 16 {
        return Err(Error::StreamError {
            message: format!("unwrap IV must be 16 bytes, got {}", iv.len()),
        });
    }

    let mut buf = wrapped.clone();
    let decrypted = Aes128CbcDec::new(session_key.into(), iv.as_slice().try_into().unwrap())
        .decrypt_padded::<aes::cipher::block_padding::Pkcs7>(&mut buf)
        .map_err(|e| Error::StreamError {
            message: format!("AES-CBC unwrap failed: {e}"),
        })?;

    if decrypted.len() != 16 {
        return Err(Error::StreamError {
            message: format!("unwrapped key must be 16 bytes, got {}", decrypted.len()),
        });
    }

    let mut key = [0u8; 16];
    key.copy_from_slice(decrypted);
    Ok(key)
}

/// Decrypt a FLAC frame in-place using AES-128-CTR.
///
/// iv_8 = 8-byte IV from the segment UUID box entry, zero-padded to 16 bytes.
pub fn decrypt_frame(content_key: &[u8; 16], iv_8: &[u8; 8], data: &mut [u8]) {
    let mut nonce = [0u8; 16];
    nonce[..8].copy_from_slice(iv_8);
    let mut cipher = Aes128Ctr::new(content_key.into(), &nonce.into());
    cipher.apply_keystream(data);
}

fn hex_decode(hex: &str) -> Result<Vec<u8>, Error> {
    (0..hex.len())
        .step_by(2)
        .map(|i| {
            u8::from_str_radix(&hex[i..i + 2], 16).map_err(|e| Error::StreamError {
                message: format!("hex decode error at {i}: {e}"),
            })
        })
        .collect()
}
