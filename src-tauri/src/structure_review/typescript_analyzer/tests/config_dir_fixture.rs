//! 真实语言服务和公共图共用的直接/继承模板夹具。

/// 共享配置目录含同名导出竞争目标；正确目标始终属于消费配置目录。
pub(super) fn fixture(inherited: bool) -> tempfile::TempDir {
    let directory = tempfile::tempdir().unwrap();
    let base = r#"{"compilerOptions":{"target":"ES2022","module":"Preserve","moduleResolution":"Bundler","types":[],"baseUrl":"${configDir}","rootDir":"${configDir}/src","rootDirs":["${configDir}/src","${configDir}/generated"],"typeRoots":["${configDir}/types"],"paths":{"@app/*":["src/*"]}},"include":["${configDir}/src/**/*.ts"],"exclude":["${configDir}/src/excluded/**"]}"#;
    for (file, source) in [
        ("app/tsconfig.json", if inherited { r#"{"extends":"../config/base.json"}"# } else { base }),
        ("config/base.json", base),
        ("app/src/main.ts", "import {target} from '@app/dep';\nexport function caller() { target(); }"),
        ("app/src/dep.ts", "export function target() { return 'correct consumer'; }"),
        ("config/src/dep.ts", "export function wrongTarget() { return 'wrong shared base'; } export {wrongTarget as target};"),
        ("app/src/excluded/hidden.ts", "export const hidden = true;"),
    ] {
        let path = directory.path().join(file);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, source).unwrap();
    }
    directory
}
