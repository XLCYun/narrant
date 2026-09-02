use super::{config, MinisignVerifier};
use base64::Engine;
use minisign_verify::{PublicKey, Signature};
use std::fs::File;
use std::io::Read;
use std::path::Path;

/// 使用编译期 Tauri minisign 公钥对真实磁盘文件执行流式验证。
pub(crate) struct DiskMinisignVerifier {
    public_key: PublicKey,
}

impl DiskMinisignVerifier {
    /// 从已验证的 Tauri 外层 base64 公钥创建磁盘验证器。
    pub(crate) fn new(encoded_public_key: &str) -> Result<Self, config::UpdaterConfigError> {
        Ok(Self {
            public_key: config::decode_public_key(encoded_public_key)?,
        })
    }
}

impl MinisignVerifier for DiskMinisignVerifier {
    /// 解码 Tauri `.sig` 外层 base64，并以固定缓冲流式复验完整磁盘包。
    fn verify(&self, package: &Path, encoded_signature: &str) -> Result<(), String> {
        let signature_text = base64::engine::general_purpose::STANDARD
            .decode(encoded_signature)
            .ok()
            .and_then(|bytes| String::from_utf8(bytes).ok())
            .ok_or_else(|| "updater-signature-encoding".to_owned())?;
        let signature = Signature::decode(&signature_text)
            .map_err(|_| "updater-signature-invalid".to_owned())?;
        let mut verifier = self
            .public_key
            .verify_stream(&signature)
            .map_err(|_| "updater-signature-mode".to_owned())?;
        let mut file = File::open(package).map_err(|_| "updater-package-unavailable".to_owned())?;
        let mut buffer = [0_u8; 64 * 1024];
        loop {
            let read = file
                .read(&mut buffer)
                .map_err(|_| "updater-package-unavailable".to_owned())?;
            if read == 0 {
                break;
            }
            verifier.update(&buffer[..read]);
        }
        verifier
            .finalize()
            .map_err(|_| "updater-signature-verification".to_owned())
    }
}
