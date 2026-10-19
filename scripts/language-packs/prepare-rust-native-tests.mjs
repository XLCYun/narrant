import fs from 'node:fs/promises';
import path from 'node:path';
import { execFileSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';

const repository = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../..');
export const historicalCargoVersions = ['1.81.0', '1.94.0', '1.96.0'];

/** 验收工具链只读探测，缺少已安装版本时失败；这些历史样本不限制产品用户版本。 */
function toolchainRoot(version, supplied) {
  const suffix = process.platform === 'win32' ? '.exe' : '';
  const root = supplied ?? path.dirname(path.dirname(execFileSync('rustup', ['which', '--toolchain', version, 'rustc'], { encoding: 'utf8', env: { ...process.env, RUSTUP_AUTO_INSTALL: '0' } }).trim()));
  if (!path.isAbsolute(root)) throw new Error(`Rust test toolchain root must be absolute: ${version}`);
  const actual = execFileSync(path.join(root, `bin/rustc${suffix}`), ['--version'], { encoding: 'utf8' }).trim();
  if (!actual.startsWith(`rustc ${version} `)) throw new Error(`Missing designated Cargo test toolchain ${version}: ${actual}`);
  return root;
}

/** 项目标准库 metadata 必须完整离线解析；准备阶段只下载依赖，不执行项目构建代码。 */
export async function prepareRustNativeTests(environment, prepare = false) {
  if (!environment.NARRANT_PROJECT_RUST_TOOLCHAIN || !path.isAbsolute(environment.CARGO_HOME ?? '')) throw new Error('Rust tests require explicit NARRANT_PROJECT_RUST_TOOLCHAIN and absolute CARGO_HOME');
  const supplied = environment.NARRANT_TEST_CARGO_TOOLCHAINS?.split(path.delimiter);
  if (supplied && supplied.length !== historicalCargoVersions.length) throw new Error('Rust old-Cargo matrix requires exactly 1.81.0/1.94.0/1.96.0 roots');
  const roots = historicalCargoVersions.map((version, index) => toolchainRoot(version, supplied?.[index]));
  if (environment.NARRANT_TEST_CARGO_194 && path.resolve(environment.NARRANT_TEST_CARGO_194) !== path.resolve(roots[1])) throw new Error('NARRANT_TEST_CARGO_194 must match the designated 1.94.0 matrix root');
  const env = { ...environment, RUSTUP_TOOLCHAIN: environment.NARRANT_PROJECT_RUST_TOOLCHAIN, RUSTUP_AUTO_INSTALL: '0', CARGO_CACHE_RUSTC_INFO: '0', __CARGO_TEST_CHANNEL_OVERRIDE_DO_NOT_USE_THIS: 'nightly' };
  const sysroot = execFileSync('rustc', ['--print', 'sysroot'], { encoding: 'utf8', env }).trim();
  const library = path.join(sysroot, 'lib/rustlib/src/rust/library');
  for (const file of ['Cargo.toml', 'Cargo.lock', 'core/src/lib.rs', 'std/src/lib.rs']) await fs.access(path.join(library, file)).catch(() => { throw new Error(`Missing project rust-src prerequisite: ${path.join(library, file)}`); });
  const args = ['scripts/cargo-agent.sh', 'metadata', '--manifest-path', path.join(library, 'Cargo.toml'), '--format-version', '1', '--locked'];
  // Cargo 从仓库根运行，避免读取 sysroot 自身的 bootstrap 配置；与原生 loader 的 metadata 参数一致。
  if (prepare) execFileSync('bash', args, { cwd: repository, env: { ...env, CARGO_NET_OFFLINE: 'false' }, stdio: ['ignore', 'ignore', 'inherit'] });
  execFileSync('bash', [...args, '--offline'], { cwd: repository, env: { ...env, CARGO_NET_OFFLINE: 'true' }, stdio: ['ignore', 'ignore', 'inherit'] });
  console.log(`Rust project tests: ${environment.NARRANT_PROJECT_RUST_TOOLCHAIN}; rust-src=${library}; offline metadata verified; CARGO_HOME=${environment.CARGO_HOME}`);
  return { NARRANT_PROJECT_RUST_TOOLCHAIN: environment.NARRANT_PROJECT_RUST_TOOLCHAIN, NARRANT_TEST_CARGO_TOOLCHAINS: roots.join(path.delimiter), NARRANT_TEST_CARGO_194: roots[1] };
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const prepared = await prepareRustNativeTests(process.env, process.argv.includes('--prepare'));
  if (process.env.GITHUB_ENV) await fs.appendFile(process.env.GITHUB_ENV, Object.entries(prepared).map(([key, value]) => `${key}=${value}\n`).join(''));
}
