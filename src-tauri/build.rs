use std::path::Path;

/// 应用构建保留 Tauri 资源流程，原生测试按显式入口补齐 Windows 加载要求。
fn main() {
    load_local_github_app_configuration();
    track_application_update_configuration();
    configure_windows_native_tests();
    tauri_build::build()
}

/// Tauri 只向应用二进制链接 manifest；库单元测试也需要 Common Controls v6。
fn configure_windows_native_tests() {
    const FLAG: &str = "NARRANT_WINDOWS_TEST_MANIFEST";
    println!("cargo:rerun-if-env-changed={FLAG}");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows")
        && std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc")
        && std::env::var(FLAG).as_deref() == Ok("1")
    {
        println!("cargo:rustc-link-arg=/MANIFEST:EMBED");
        println!("cargo:rustc-link-arg=/MANIFESTDEPENDENCY:type='win32' name='Microsoft.Windows.Common-Controls' version='6.0.0.0' processorArchitecture='*' publicKeyToken='6595b64144ccf1df' language='*'");
        // 集成测试同时构建应用；它已有 Tauri 资源内的 manifest，不能再生成第二份资源 1。
        println!("cargo:rustc-link-arg-bin=narrant=/MANIFEST:NO");
    }
}

/// 让编译期 updater trust root 变化必然触发应用 crate 重新编译。
fn track_application_update_configuration() {
    for name in ["NARRANT_UPDATER_ENDPOINT", "NARRANT_UPDATER_PUBLIC_KEY"] {
        println!("cargo:rerun-if-env-changed={name}");
    }
}

/// 将根目录的本地 GitHub App 公开配置传递给 Rust 编译器。
///
/// `.env.local` 只服务于本地开发；已存在的 shell 或 CI 环境变量优先，避免本地
/// 文件意外覆盖发布构建的配置。
fn load_local_github_app_configuration() {
    const LOCAL_ENV_PATH: &str = "../.env.local";
    const GITHUB_APP_ENVIRONMENTS: [&str; 2] =
        ["NARRANT_GITHUB_CLIENT_ID", "NARRANT_GITHUB_APP_SLUG"];

    // Cargo 会把不存在的 `rerun-if-changed` 路径永久视为 Dirty。只有文件实际
    // 存在时才监听它，避免每条 check/test 命令都重新编译整个应用 crate。
    if Path::new(LOCAL_ENV_PATH).is_file() {
        println!("cargo:rerun-if-changed={LOCAL_ENV_PATH}");
    }
    for name in GITHUB_APP_ENVIRONMENTS {
        println!("cargo:rerun-if-env-changed={name}");
    }
    if !Path::new(LOCAL_ENV_PATH).is_file() {
        return;
    }

    for entry in
        dotenvy::from_path_iter(LOCAL_ENV_PATH).expect("读取 .env.local 中的 GitHub App 配置")
    {
        let (name, value) = entry.expect("解析 .env.local 中的 GitHub App 配置");
        if GITHUB_APP_ENVIRONMENTS.contains(&name.as_str()) && std::env::var_os(&name).is_none() {
            println!("cargo:rustc-env={name}={value}");
        }
    }
}
