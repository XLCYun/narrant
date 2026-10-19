import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import { runWindowsLifecycleTests } from '../language-packs/run-windows-lifecycle-tests.mjs';

// 非 Windows 主机不能因为 cfg 排除了全部测试而得到成功结论。
test('lifecycle validation rejects non-Windows hosts before starting Cargo', async () => {
  await assert.rejects(runWindowsLifecycleTests({ platform: 'darwin' }), /requires a Windows runner/);
});

// 跨真实 bash/Node 边界验证生产所有者筛选器、原生 Node 路径和真实失败传播。
test('lifecycle validation runs only the production owner and preserves its failure', async () => {
  const directory = await fs.mkdtemp(path.join(os.tmpdir(), 'windows lifecycle '));
  try {
    await fs.mkdir(path.join(directory, 'scripts'));
    await fs.writeFile(path.join(directory, 'scripts/cargo-agent.sh'), 'set -eu\nnode scripts/fixture.cjs "$@"\n');
    await fs.writeFile(path.join(directory, 'scripts/fixture.cjs'), `
const assert = require('node:assert/strict');
const fs = require('node:fs');
assert.equal(process.env.NARRANT_TEST_NODE, process.execPath);
const args = process.argv.slice(2);
const common = ['test', '--manifest-path', 'src-tauri/Cargo.toml', '--locked', '--lib'];
const filter = args[5];
assert.equal(filter, 'child_process::');
assert.deepEqual(args, [...common, filter, '--', '--nocapture', '--test-threads=1']);
const previous = fs.existsSync('executed') ? fs.readFileSync('executed', 'utf8') : '';
assert.equal(previous, '');
fs.appendFileSync('executed', filter);
process.exit(7);
`);
    await assert.rejects(runWindowsLifecycleTests({ cwd: directory, platform: 'win32' }), /failed \(7\)/);
    assert.equal(await fs.readFile(path.join(directory, 'executed'), 'utf8'), 'child_process::');
  } finally {
    await fs.rm(directory, { recursive: true, force: true });
  }
});
