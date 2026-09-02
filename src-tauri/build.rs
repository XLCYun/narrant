use std::path::Path;

fn main() {
    load_local_github_app_configuration();
    track_application_update_configuration();
    tauri_build::build()
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
