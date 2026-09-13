#[cfg(unix)]
mod session;
#[cfg(unix)]
mod timing;

use super::file_uri;

/// URI 转换是公开的语言无关工具，空格和 Unicode 路径不会成为错误 URL。
#[test]
fn file_uris_encode_special_characters() {
    let directory = tempfile::tempdir().unwrap();
    let uri = file_uri(&directory.path().join("中 文.py")).unwrap();
    assert!(uri.ends_with("%E4%B8%AD%20%E6%96%87.py"));
    assert!(file_uri(std::path::Path::new("relative.py")).is_err());
}
