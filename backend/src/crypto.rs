//! Encryption of secrets at rest (AES-256-GCM) and small random/hash helpers.

use std::path::Path;

use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes256Gcm, Nonce};
use anyhow::{Context, anyhow, bail};
use base64::Engine as _;
use base64::engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD};
use sha2::{Digest, Sha256};

const PREFIX: &str = "v1:";
const NONCE_LEN: usize = 12;

pub struct Crypto {
    cipher: Aes256Gcm,
}

impl Crypto {
    pub fn from_key(key: &[u8]) -> anyhow::Result<Self> {
        if key.len() != 32 {
            bail!("encryption key must be exactly 32 bytes (got {})", key.len());
        }
        let cipher = Aes256Gcm::new_from_slice(key).map_err(|_| anyhow!("invalid encryption key"))?;
        Ok(Self { cipher })
    }

    /// Uses the key from the environment when given, otherwise loads (or creates) `secret.key`.
    pub fn load_or_create(env_key: Option<&str>, data_dir: &Path) -> anyhow::Result<Self> {
        if let Some(encoded) = env_key.map(str::trim).filter(|k| !k.is_empty()) {
            let key = STANDARD.decode(encoded).context("ODB_SECRET_KEY is not valid base64")?;
            return Self::from_key(&key);
        }

        let path = data_dir.join("secret.key");
        if path.exists() {
            let encoded = std::fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
            let key =
                STANDARD.decode(encoded.trim()).with_context(|| format!("{} is not valid base64", path.display()))?;
            return Self::from_key(&key);
        }

        let key = random_bytes::<32>();
        write_private_file(&path, STANDARD.encode(key).as_bytes())?;
        tracing::warn!(
            path = %path.display(),
            "generated a new encryption key; back it up, stored secrets cannot be read without it"
        );
        Self::from_key(&key)
    }

    pub fn encrypt(&self, plaintext: &str) -> String {
        let nonce_bytes = random_bytes::<NONCE_LEN>();
        let nonce = Nonce::try_from(&nonce_bytes[..]).expect("nonce length");
        let ciphertext = self
            .cipher
            .encrypt(&nonce, plaintext.as_bytes())
            .expect("AES-GCM encryption cannot fail for in-memory buffers");
        let mut out = Vec::with_capacity(NONCE_LEN + ciphertext.len());
        out.extend_from_slice(&nonce_bytes);
        out.extend_from_slice(&ciphertext);
        format!("{PREFIX}{}", STANDARD.encode(out))
    }

    pub fn decrypt(&self, value: &str) -> anyhow::Result<String> {
        let encoded = value.strip_prefix(PREFIX).ok_or_else(|| anyhow!("unsupported secret format"))?;
        let raw = STANDARD.decode(encoded).context("corrupted secret (base64)")?;
        if raw.len() <= NONCE_LEN {
            bail!("corrupted secret (too short)");
        }
        let (nonce_bytes, ciphertext) = raw.split_at(NONCE_LEN);
        let nonce = Nonce::try_from(nonce_bytes).map_err(|_| anyhow!("corrupted secret (nonce)"))?;
        let plaintext = self
            .cipher
            .decrypt(&nonce, ciphertext)
            .map_err(|_| anyhow!("cannot decrypt secret: wrong encryption key or corrupted data"))?;
        String::from_utf8(plaintext).context("decrypted secret is not UTF-8")
    }
}

fn write_private_file(path: &Path, contents: &[u8]) -> anyhow::Result<()> {
    use std::io::Write;
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(path).with_context(|| format!("creating {}", path.display()))?;
    file.write_all(contents)?;
    file.sync_all()?;
    Ok(())
}

pub fn random_bytes<const N: usize>() -> [u8; N] {
    let mut buf = [0u8; N];
    getrandom::fill(&mut buf).expect("operating system RNG unavailable");
    buf
}

/// 256-bit random token, URL-safe base64 without padding.
pub fn random_token() -> String {
    URL_SAFE_NO_PAD.encode(random_bytes::<32>())
}

pub fn sha256_hex(data: &[u8]) -> String {
    hex::encode(Sha256::digest(data))
}

/// Constant-time comparison for secrets such as bearer tokens.
pub fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip() {
        let crypto = Crypto::from_key(&random_bytes::<32>()).unwrap();
        let secret = "s3cr3t master password ✓";
        let enc = crypto.encrypt(secret);
        assert!(enc.starts_with(PREFIX));
        assert_ne!(enc, crypto.encrypt(secret), "nonce must be random");
        assert_eq!(crypto.decrypt(&enc).unwrap(), secret);
    }

    #[test]
    fn wrong_key_fails() {
        let a = Crypto::from_key(&random_bytes::<32>()).unwrap();
        let b = Crypto::from_key(&random_bytes::<32>()).unwrap();
        assert!(b.decrypt(&a.encrypt("x")).is_err());
    }

    #[test]
    fn tamper_detected() {
        let crypto = Crypto::from_key(&random_bytes::<32>()).unwrap();
        let enc = crypto.encrypt("hello");
        let mut raw = STANDARD.decode(&enc[PREFIX.len()..]).unwrap();
        let last = raw.len() - 1;
        raw[last] ^= 1;
        let tampered = format!("{PREFIX}{}", STANDARD.encode(raw));
        assert!(crypto.decrypt(&tampered).is_err());
    }

    #[test]
    fn key_file_is_created_and_reused() {
        let dir = tempfile::tempdir().unwrap();
        let a = Crypto::load_or_create(None, dir.path()).unwrap();
        let b = Crypto::load_or_create(None, dir.path()).unwrap();
        assert_eq!(b.decrypt(&a.encrypt("same key")).unwrap(), "same key");
    }
}
