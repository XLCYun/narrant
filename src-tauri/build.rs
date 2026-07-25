use std::path::Path;

fn main() {
    load_local_github_app_configuration();
    tauri_build::build()
}

/// 将根目录的本地 GitHub App 公开配置传递给 Rust 编译器。
///
/// `.env.local` 只服务于本地开发；已存在的 shell 或 CI 环境变量优先，避免本地
/// 文件意外覆盖发布构建的配置。
fn load_local_github_app_configuration() {
    const LOCAL_ENV_PATH: &str = "../.env.local";
    const GITHUB_APP_ENVIRONMENTS: [&str; 2] =
        ["NARRANT_GITHUB_CLIENT_ID", "NARRANT_GITHUB_APP_SLUG"];

    println!("cargo:rerun-if-changed={LOCAL_ENV_PATH}");
    for name in GITHUB_APP_ENVIRONMENTS {
        println!("cargo:rerun-if-env-changed={name}");
    }
    if !Path::new(LOCAL_ENV_PATH).exists() {
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
