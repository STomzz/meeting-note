//! 本地密钥保险箱：用 AES-256-GCM 加密 API Key 等敏感值。
//!
//! 说明：密钥文件（32 字节随机数）与数据库同目录，权限 0600。
//! 这属于"本机加密存储"，能防备份/误拷贝泄露；后续可平滑升级为
//! Windows Credential Manager / Android Keystore（接口不变）。

use aes_gcm::aead::{Aead, KeyInit, OsRng};
use aes_gcm::{AeadCore, Aes256Gcm, Key, Nonce};
use anyhow::{anyhow, bail, Result};
use base64::Engine as _;
use std::path::Path;

const KEY_LEN: usize = 32;
const NONCE_LEN: usize = 12;

pub struct SecretBox {
    cipher: Aes256Gcm,
}

impl SecretBox {
    /// 读取或创建本机密钥文件。
    pub fn load_or_create(path: &Path) -> Result<Self> {
        let key_bytes: [u8; KEY_LEN] = if path.exists() {
            let raw = std::fs::read(path)?;
            raw.as_slice()
                .try_into()
                .map_err(|_| anyhow!("密钥文件损坏（长度应为 {KEY_LEN} 字节）"))?
        } else {
            let key = Aes256Gcm::generate_key(&mut OsRng);
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            let bytes: [u8; KEY_LEN] = key.into();
            std::fs::write(path, bytes)?;
            restrict_permissions(path)?;
            bytes
        };
        Ok(Self {
            cipher: Aes256Gcm::new(&Key::<Aes256Gcm>::from(key_bytes)),
        })
    }

    /// 加密 → base64(nonce || ciphertext)。
    pub fn encrypt(&self, plain: &str) -> Result<String> {
        let nonce = Aes256Gcm::generate_nonce(&mut OsRng);
        let ct = self
            .cipher
            .encrypt(&nonce, plain.as_bytes())
            .map_err(|e| anyhow!("加密失败: {e}"))?;
        let mut out = nonce.to_vec();
        out.extend_from_slice(&ct);
        Ok(base64::engine::general_purpose::STANDARD.encode(out))
    }

    /// 解密。
    pub fn decrypt(&self, encoded: &str) -> Result<String> {
        let raw = base64::engine::general_purpose::STANDARD
            .decode(encoded.trim())
            .map_err(|e| anyhow!("密文不是合法 base64: {e}"))?;
        if raw.len() <= NONCE_LEN {
            bail!("密文长度异常");
        }
        let (nonce, ct) = raw.split_at(NONCE_LEN);
        let pt = self
            .cipher
            .decrypt(Nonce::from_slice(nonce), ct)
            .map_err(|_| anyhow!("解密失败（密钥不匹配或数据被篡改）"))?;
        Ok(String::from_utf8(pt)?)
    }
}

#[cfg(unix)]
fn restrict_permissions(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let perms = std::fs::Permissions::from_mode(0o600);
    std::fs::set_permissions(path, perms)?;
    Ok(())
}

#[cfg(not(unix))]
fn restrict_permissions(_path: &Path) -> Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let sb = SecretBox::load_or_create(&dir.path().join("secret.key")).unwrap();
        let enc = sb.encrypt("sk-测试-key-123").unwrap();
        assert_ne!(enc, "sk-测试-key-123");
        assert_eq!(sb.decrypt(&enc).unwrap(), "sk-测试-key-123");
    }

    #[test]
    fn persists_across_reload() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("secret.key");
        let enc = SecretBox::load_or_create(&path).unwrap().encrypt("hello").unwrap();
        let sb2 = SecretBox::load_or_create(&path).unwrap();
        assert_eq!(sb2.decrypt(&enc).unwrap(), "hello");
    }

    #[test]
    fn tampered_ciphertext_fails() {
        let dir = tempfile::tempdir().unwrap();
        let sb = SecretBox::load_or_create(&dir.path().join("secret.key")).unwrap();
        let enc = sb.encrypt("secret").unwrap();
        let mut bytes = base64::engine::general_purpose::STANDARD.decode(&enc).unwrap();
        let last = bytes.len() - 1;
        bytes[last] ^= 0xff;
        let bad = base64::engine::general_purpose::STANDARD.encode(bytes);
        assert!(sb.decrypt(&bad).is_err());
    }
}
