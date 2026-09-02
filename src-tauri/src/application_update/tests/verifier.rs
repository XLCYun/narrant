use super::super::{verifier::DiskMinisignVerifier, MinisignVerifier};
use base64::Engine;
use std::io::Write;

const PUBLIC_KEY: &str = "untrusted comment: minisign public key E7620F1842B4E81F\nRWQf6LRCGA9i53mlYecO4IzT51TGPpvWucNSCh1CBM0QTaLn73Y7GFO3";
const PREHASHED_SIGNATURE: &str = "untrusted comment: signature from minisign secret key\nRUQf6LRCGA9i559r3g7V1qNyJDApGip8MfqcadIgT9CuhV3EMhHoN1mGTkUidF/z7SrlQgXdy8ofjb7bNJJylDOocrCo8KLzZwo=\ntrusted comment: timestamp:1556193335\tfile:test\ny/rUw2y8/hOUYjZU71eHp/Wo1KZ40fGy2VJEDl34XMJM+TX48Ss/17u3IvIfbVR1FkZZSNCisQbuQY+bHwhEBg==";

/// 按 Tauri updater wire format 对 minisign 文本增加外层 base64。
fn encode(value: &str) -> String {
    base64::engine::general_purpose::STANDARD.encode(value)
}

#[test]
fn verifier_checks_real_signature_against_disk_bytes_and_rejects_tampering() {
    let verifier = DiskMinisignVerifier::new(&encode(PUBLIC_KEY)).unwrap();
    let mut package = tempfile::NamedTempFile::new().unwrap();
    package.write_all(b"test").unwrap();
    package.as_file_mut().sync_all().unwrap();
    assert!(verifier
        .verify(package.path(), &encode(PREHASHED_SIGNATURE))
        .is_ok());

    package.as_file_mut().set_len(0).unwrap();
    package.write_all(b"Test").unwrap();
    package.as_file_mut().sync_all().unwrap();
    assert!(verifier
        .verify(package.path(), &encode(PREHASHED_SIGNATURE))
        .is_err());
}
