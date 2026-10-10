use super::*;

/// 未安装工具时明确失败，不能跳过已识别语言。
#[test]
fn missing_pack_is_an_error() {
    let root = tempfile::tempdir().unwrap();
    assert!(from_pack(PackId::Go, root.path()).is_err());
}

/// 控制文件不能把工具或标准库指向包外。
#[test]
fn relative_paths_cannot_escape_pack() {
    let root = tempfile::tempdir().unwrap();
    for path in ["../sdk", "/sdk", "C:/sdk", "runtime/../sdk", "runtime\\sdk"] {
        assert!(inside(root.path(), path).is_err(), "{path}");
    }
}
