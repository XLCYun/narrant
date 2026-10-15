import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import fs from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';

// 在下载大型工具链前确认自托管身份、缓存与本机 Apple SDK；不改系统开发目录。
assert.equal(process.platform, 'darwin', 'macOS runner verification requires macOS');
assert.equal(process.arch, 'arm64', 'The language pack target requires Apple Silicon');
assert.equal(process.env.RUNNER_ENVIRONMENT, 'self-hosted');
assert.notEqual(process.getuid(), 0, 'The runner must not execute jobs as root');
assert.equal(process.env.SCCACHE_CACHE_SIZE, '2G');
assert.equal(process.env.RUSTC_WRAPPER, 'sccache');
assert.equal(process.env.CARGO_INCREMENTAL, '0');
assert.equal(process.env.CARGO_BUILD_JOBS, '1');
assert.ok(path.isAbsolute(process.env.NARRANT_CARGO_LOCK_FILE ?? ''), 'A shared local/CI Cargo lock is required');
assert.ok(path.isAbsolute(process.env.SCCACHE_SERVER_UDS ?? ''), 'Use a separate sccache socket for CI');
const statistics = JSON.parse(execFileSync('sccache', ['--show-stats', '--stats-format=json'], {
  encoding: 'utf8', timeout: 15_000,
}));
assert.equal(statistics.max_cache_size, 2 * 1024 ** 3);
console.log(`macOS runner: ${os.userInfo().username}; compiler cache limit: 2 GiB; Cargo jobs: 1.`);

const sdk = execFileSync('/usr/bin/xcrun', ['--sdk', 'macosx', '--show-sdk-path'], { encoding: 'utf8' }).trim();
const version = execFileSync('/usr/bin/xcrun', ['--sdk', 'macosx', '--show-sdk-version'], { encoding: 'utf8' }).trim();
await fs.access(path.join(sdk, 'usr/include/stdio.h'));
execFileSync('/usr/bin/xcrun', ['clang', '-x', 'c', '-fsyntax-only', '-isysroot', sdk, '-'], {
  input: '#include <stdio.h>\nint main(void) { return puts("runner"); }\n', stdio: ['pipe', 'inherit', 'inherit'],
});
if (process.env.GITHUB_ENV) {
  await fs.appendFile(process.env.GITHUB_ENV, `NARRANT_MACOS_SDK=${sdk}\nNARRANT_MACOS_SDK_VERSION=${version}\n`);
}
console.log(`Native SDK declarations: macOS ${version}, ${sdk}`);

// 仅轻量验收创建无依赖 Rust 库，实际验证 Cargo、链接器和缓存接入，不编译 Tauri。
if (process.argv.includes('--smoke')) {
  const root = await fs.mkdtemp(path.join(os.tmpdir(), 'narrant-macos-runner-'));
  try {
    await fs.mkdir(path.join(root, 'src'));
    await fs.writeFile(path.join(root, 'Cargo.toml'), '[package]\nname="narrant-runner-smoke"\nversion="0.0.0"\nedition="2021"\n');
    await fs.writeFile(path.join(root, 'src/lib.rs'), '// 验证本机编译目标。\npub fn platform() -> bool { cfg!(all(target_os = "macos", target_arch = "aarch64")) }\n');
    await fs.mkdir(path.join(root, 'tests'));
    await fs.writeFile(path.join(root, 'tests/platform.rs'), '#[test]\nfn native_platform() { assert!(narrant_runner_smoke::platform()); }\n');
    await fs.symlink('src', path.join(root, 'directory-link'));
    await fs.symlink('src/lib.rs', path.join(root, 'file-link'));
    assert.equal(await fs.readFile(path.join(root, 'file-link'), 'utf8'), await fs.readFile(path.join(root, 'directory-link/lib.rs'), 'utf8'));
    const target = path.resolve('src-tauri/target/runner-smoke');
    execFileSync('bash', ['scripts/cargo-agent.sh', 'test', '--offline', '--manifest-path', path.join(root, 'Cargo.toml'), '--target-dir', target], {
      stdio: 'inherit', timeout: 120_000,
    });
    await fs.writeFile(path.join(target, 'runner-smoke.log'), 'Keep this log while removing generated build files.\n');
    console.log('Native Rust compilation, execution and filesystem checks passed; generated files await the completion hook.');
  } finally {
    await fs.rm(root, { recursive: true, force: true });
  }
}
