import { spawn } from 'node:child_process';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

/** 只在 Cargo 测试子环境移除已确认的构建选择，保留用户 PATH 和其他工具覆盖。 */
export function rustHelperTestEnvironment(env = process.env, platform = process.platform) {
  const environment = { ...env, RUSTUP_AUTO_INSTALL: '0' };
  // 固定上游优先读取 CARGO；构建版本不能绕过夹具的目录/显式旧工具链选择。
  const paths = platform === 'win32' ? path.win32 : path;
  const toolchain = environment.RUSTUP_TOOLCHAIN;
  const userHome = platform === 'win32' ? environment.USERPROFILE : environment.HOME;
  const rustupHome = environment.RUSTUP_HOME ?? (userHome && paths.join(userHome, '.rustup'));
  if (toolchain && rustupHome && environment.CARGO) {
    const root = paths.isAbsolute(toolchain) ? toolchain : paths.join(rustupHome, 'toolchains', toolchain);
    const cargo = paths.join(root, 'bin', platform === 'win32' ? 'cargo.exe' : 'cargo');
    if (paths.relative(cargo, environment.CARGO) === '') delete environment.CARGO;
  }
  delete environment.RUSTUP_TOOLCHAIN;
  return environment;
}

/** 原生测试仍接收原始 argv，仅复制环境，不修改生产进程的工具链选择。 */
function run() {
  const [executable, ...args] = process.argv.slice(2);
  if (!executable) throw new Error('Rust helper test runner requires the Cargo test executable');
  const child = spawn(executable, args, { env: rustHelperTestEnvironment(), stdio: 'inherit' });
  child.once('error', error => { console.error(error); process.exitCode = 1; });
  child.once('close', (code, signal) => { process.exitCode = signal ? 1 : code ?? 1; });
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) run();
