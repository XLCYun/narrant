import fs from 'node:fs/promises';
import path from 'node:path';
import { execFileSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';

const repository = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../..');
/** 项目标准库 metadata 必须完整离线解析；准备阶段只下载依赖，不执行项目构建代码。 */
export async function prepareRustNativeTests(environment, prepare = false) {
  if (!environment.NARRANT_PROJECT_RUST_TOOLCHAIN || !path.isAbsolute(environment.CARGO_HOME ?? '')) throw new Error('Rust tests require explicit NARRANT_PROJECT_RUST_TOOLCHAIN and absolute CARGO_HOME');
  const env = { ...environment, RUSTUP_TOOLCHAIN: environment.NARRANT_PROJECT_RUST_TOOLCHAIN, RUSTUP_AUTO_INSTALL: '0', CARGO_CACHE_RUSTC_INFO: '0', __CARGO_TEST_CHANNEL_OVERRIDE_DO_NOT_USE_THIS: 'nightly' };
  const sysroot = execFileSync('rustc', ['--print', 'sysroot'], { encoding: 'utf8', env }).trim();
  const library = path.join(sysroot, 'lib/rustlib/src/rust/library');
  for (const file of ['Cargo.toml', 'Cargo.lock', 'core/src/lib.rs', 'std/src/lib.rs']) await fs.access(path.join(library, file)).catch(() => { throw new Error(`Missing project rust-src prerequisite: ${path.join(library, file)}`); });
  const args = ['scripts/cargo-agent.sh', 'metadata', '--manifest-path', path.join(library, 'Cargo.toml'), '--format-version', '1', '--locked'];
  // Cargo 从仓库根运行，避免读取 sysroot 自身的 bootstrap 配置；与原生 loader 的 metadata 参数一致。
  if (prepare) execFileSync('bash', args, { cwd: repository, env: { ...env, CARGO_NET_OFFLINE: 'false' }, stdio: ['ignore', 'ignore', 'inherit'] });
  execFileSync('bash', [...args, '--offline'], { cwd: repository, env: { ...env, CARGO_NET_OFFLINE: 'true' }, stdio: ['ignore', 'ignore', 'inherit'] });
  console.log(`Rust project tests: ${environment.NARRANT_PROJECT_RUST_TOOLCHAIN}; rust-src=${library}; offline metadata verified; CARGO_HOME=${environment.CARGO_HOME}`);
  return { NARRANT_PROJECT_RUST_TOOLCHAIN: environment.NARRANT_PROJECT_RUST_TOOLCHAIN };
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const prepared = await prepareRustNativeTests(process.env, process.argv.includes('--prepare'));
  if (process.env.GITHUB_ENV) await fs.appendFile(process.env.GITHUB_ENV, Object.entries(prepared).map(([key, value]) => `${key}=${value}\n`).join(''));
}
