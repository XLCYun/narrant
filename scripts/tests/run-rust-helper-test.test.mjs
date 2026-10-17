import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import { test } from 'node:test';
import { rustHelperTestEnvironment } from '../run-rust-helper-test.mjs';

// 实际测试 runner 只移除 Cargo 构建覆盖，不改用户工具路径或进程全局环境。
for (const injected of [true, false]) {
  test(`Rust 测试子环境保留用户路径并识别构建 CARGO injected=${injected}`, t => {
    const directory = fs.mkdtempSync(path.join(os.tmpdir(), 'narrant test env 空格-'));
    t.after(() => fs.rmSync(directory, { recursive: true, force: true }));
    const fixture = path.join(directory, 'environment.mjs');
    fs.writeFileSync(fixture, "process.stdout.write(JSON.stringify({CARGO:process.env.CARGO,RUSTUP_TOOLCHAIN:process.env.RUSTUP_TOOLCHAIN,PATH:process.env.PATH,RUSTC:process.env.RUSTC}));\n");
    const toolchain = '1.98.0-test';
    const cargo = path.join(directory, injected ? `toolchains/${toolchain}/bin` : 'user-tools', process.platform === 'win32' ? 'cargo.exe' : 'cargo');
    const env = { ...process.env, RUSTUP_HOME: directory, RUSTUP_TOOLCHAIN: toolchain, CARGO: cargo, RUSTC: path.join(directory, 'user-rustc') };
    const child = spawnSync(process.execPath, [fileURLToPath(new URL('../run-rust-helper-test.mjs', import.meta.url)), process.execPath, fixture], { env, encoding: 'utf8' });
    assert.equal(child.status, 0, child.stderr);
    const output = JSON.parse(child.stdout);
    assert.equal(output.RUSTUP_TOOLCHAIN, undefined);
    assert.equal(output.CARGO, injected ? undefined : cargo);
    assert.equal(output.RUSTC, env.RUSTC);
    assert.equal(output.PATH, env.PATH);
  });
}

// 在本机重放 Windows 路径运算；真实 Windows 执行仍由 CI 原生目标确认。
for (const [home, cargo] of [
  ['C:\\Users\\runneradmin\\.rustup', 'C:\\Users\\runneradmin\\.rustup\\toolchains\\1.98.0-x86_64-pc-windows-msvc\\bin\\cargo.exe'],
  ['C:/Users/runneradmin/.rustup', 'c:\\users\\RUNNERADMIN\\.rustup\\toolchains\\1.98.0-x86_64-pc-windows-msvc\\bin\\CARGO.EXE'],
]) {
  test(`Windows 构建覆盖按相同 .exe 路径身份移除 ${cargo}`, () => {
    const input = { RUSTUP_HOME: home, RUSTUP_TOOLCHAIN: '1.98.0-x86_64-pc-windows-msvc', CARGO: cargo, RUSTC: 'D:\\user-tools\\rustc.exe', PATH: 'D:\\first;C:\\second' };
    const output = rustHelperTestEnvironment(input, 'win32');
    assert.equal(output.CARGO, undefined);
    assert.equal(output.PATH, input.PATH);
    assert.equal(output.RUSTC, input.RUSTC);
    assert.equal(input.CARGO, cargo);
    const userCargo = 'D:\\user-tools\\cargo.exe';
    assert.equal(rustHelperTestEnvironment({ ...input, CARGO: userCargo }, 'win32').CARGO, userCargo);
    assert.equal(rustHelperTestEnvironment({ ...input, CARGO: cargo.replace(/\.exe$/i, '') }, 'win32').CARGO, cargo.replace(/\.exe$/i, ''));
  });
}
