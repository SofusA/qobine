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
/// infos format: "`salt_b64url.info_b64url`"
/// IKM = hex-decoded `rng_init` (16 bytes)
pub fn derive_session_key(infos: &str) -> Result<[u8; 16], Error> {
    let mut parts = infos.split('.');

    let salt_part = parts.next().ok_or_else(|| Error::Stream {
        message: "session infos must have at least 2 dot-separated parts".into(),
    })?;

    let info_part = parts.next().ok_or_else(|| Error::Stream {
        message: "session infos must have at least 2 dot-separated parts".into(),
    })?;

    let salt = URL_SAFE_NO_PAD
        .decode(salt_part)
        .map_err(|e| Error::Stream {
            message: format!("failed to decode session salt: {e}"),
        })?;

    let info = URL_SAFE_NO_PAD
        .decode(info_part)
        .map_err(|e| Error::Stream {
            message: format!("failed to decode session info: {e}"),
        })?;

    let ikm = hex_decode(RNG_INIT)?;

    let hk = HkdfSha256::new(Some(&salt), &ikm);
    let mut okm = [0u8; 16];

    hk.expand(&info, &mut okm).map_err(|e| Error::Stream {
        message: format!("HKDF expand failed: {e}"),
    })?;

    Ok(okm)
}

/// Unwrap the per-track content key using the session key.
///
/// `key_str` format: "qbz-1.wrapped_key_b64url.iv_b64url"
pub fn unwrap_content_key(session_key: &[u8; 16], key_str: &str) -> Result<[u8; 16], Error> {
    let mut parts = key_str.split('.');

    let _prefix = parts.next().ok_or_else(|| Error::Stream {
        message: "key string must have at least 3 dot-separated parts".into(),
    })?;

    let wrapped_part = parts.next().ok_or_else(|| Error::Stream {
        message: "key string must have at least 3 dot-separated parts".into(),
    })?;

    let iv_part = parts.next().ok_or_else(|| Error::Stream {
        message: "key string must have at least 3 dot-separated parts".into(),
    })?;

    let wrapped = URL_SAFE_NO_PAD
        .decode(wrapped_part)
        .map_err(|e| Error::Stream {
            message: format!("failed to decode wrapped key: {e}"),
        })?;

    let iv: [u8; 16] = URL_SAFE_NO_PAD
        .decode(iv_part)
        .map_err(|e| Error::Stream {
            message: format!("failed to decode unwrap IV: {e}"),
        })?
        .try_into()
        .map_err(|iv: Vec<u8>| Error::Stream {
            message: format!("unwrap IV must be 16 bytes, got {}", iv.len()),
        })?;

    let mut buf = wrapped;
    let decrypted = Aes128CbcDec::new(session_key.into(), (&iv).into())
        .decrypt_padded::<aes::cipher::block_padding::Pkcs7>(&mut buf)
        .map_err(|e| Error::Stream {
            message: format!("AES-CBC unwrap failed: {e}"),
        })?;

    decrypted.try_into().map_err(|_| Error::Stream {
        message: format!("unwrapped key must be 16 bytes, got {}", decrypted.len()),
    })
}

/// Decrypt a FLAC frame in-place using AES-128-CTR.
///
/// `iv_8` = 8-byte IV from the segment UUID box entry, zero-padded to 16 bytes.
pub fn decrypt_frame(content_key: &[u8; 16], iv_8: &[u8; 8], data: &mut [u8]) {
    let mut nonce = [0u8; 16];
    nonce[..8].copy_from_slice(iv_8);
    let mut cipher = Aes128Ctr::new(content_key.into(), &nonce.into());
    cipher.apply_keystream(data);
}

fn hex_decode(hex: &str) -> Result<Vec<u8>, Error> {
    if !hex.len().is_multiple_of(2) {
        return Err(Error::Stream {
            message: "hex string must have an even length".into(),
        });
    }

    hex.as_bytes()
        .as_chunks::<2>()
        .0
        .iter()
        .enumerate()
        .map(|(index, chunk)| {
            let pair = std::str::from_utf8(chunk).map_err(|e| Error::Stream {
                message: format!("invalid UTF-8 in hex byte {index}: {e}"),
            })?;

            u8::from_str_radix(pair, 16).map_err(|e| Error::Stream {
                message: format!("hex decode error at byte {index}: {e}"),
            })
        })
        .collect()
}
