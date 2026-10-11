import { spawn } from 'node:child_process';

// Cargo +1.98.0 会给 test executable 注入 RUSTUP_TOOLCHAIN；它仅用于构建 helper，
// 不能覆盖夹具的目录工具链。项目测试的显式选择使用 NARRANT_PROJECT_RUST_TOOLCHAIN。
const environment = { ...process.env, RUSTUP_AUTO_INSTALL: '0' };
delete environment.RUSTUP_TOOLCHAIN;
const [executable, ...args] = process.argv.slice(2);
if (!executable) throw new Error('Rust helper test runner requires the Cargo test executable');
const child = spawn(executable, args, { env: environment, stdio: 'inherit' });
child.once('error', error => { console.error(error); process.exitCode = 1; });
child.once('close', (code, signal) => { process.exitCode = signal ? 1 : code ?? 1; });
