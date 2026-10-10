import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import { runWindowsLifecycleTests } from '../run-windows-lifecycle-tests.mjs';

// 非 Windows 主机不能因为 cfg 排除了全部测试而得到成功结论。
test('lifecycle validation rejects non-Windows hosts before starting Cargo', async () => {
  await assert.rejects(runWindowsLifecycleTests({ platform: 'darwin' }), /requires a Windows runner/);
});

// 跨真实 bash/Node 边界验证串行调度、原生 Node 路径和首组失败后的测试执行。
test('lifecycle validation preserves failure while completing the same-source test group', async () => {
  const directory = await fs.mkdtemp(path.join(os.tmpdir(), 'windows lifecycle '));
  try {
    await fs.mkdir(path.join(directory, 'scripts'));
    await fs.writeFile(path.join(directory, 'scripts/cargo-agent.sh'), 'set -eu\nnode scripts/fixture.cjs "$@"\n');
    await fs.writeFile(path.join(directory, 'scripts/fixture.cjs'), `
const assert = require('node:assert/strict');
const fs = require('node:fs');
assert.equal(process.env.NARRANT_TEST_NODE, process.execPath);
const args = process.argv.slice(2);
const common = ['test', '--manifest-path', 'native-helpers/windows-process-probe/Cargo.toml', '--locked', '--lib'];
const filter = args[5];
assert.ok(['child_process::', 'lsp::'].includes(filter));
assert.deepEqual(args, [...common, filter, '--', '--nocapture', '--test-threads=1']);
const previous = fs.existsSync('executed') ? fs.readFileSync('executed', 'utf8') : '';
assert.equal(previous, filter === 'child_process::' ? '' : 'child_process::');
fs.appendFileSync('executed', filter);
process.exit(filter === 'child_process::' ? 7 : 0);
`);
    await assert.rejects(runWindowsLifecycleTests({ cwd: directory, platform: 'win32' }), error => {
      assert.ok(error instanceof AggregateError);
      assert.equal(error.errors.length, 1);
      assert.match(error.errors[0].cause.message, /failed \(7\)/);
      return true;
    });
    assert.equal(await fs.readFile(path.join(directory, 'executed'), 'utf8'), 'child_process::lsp::');
  } finally {
    await fs.rm(directory, { recursive: true, force: true });
  }
});

// 手动轻量诊断必须只占一个 Windows runner，正常 PR 仍保留完整四组验收。
test('manual lifecycle scope isolates one cheap job and cannot publish packs', async () => {
  const workflow = (await fs.readFile(new URL('../../.github/workflows/language-support-packs.yml', import.meta.url), 'utf8')).replace(/\r\n/g, '\n');
  const job = name => workflow.split(`\n  ${name}:\n`)[1].split(/\n  [\w-]+:\n/)[0];
  assert.match(workflow, /validation_scope:\n[\s\S]*default: full/);
  const lifecycle = job('windows-lifecycle');
  assert.match(lifecycle, /if: github.event_name == 'workflow_dispatch' && inputs.validation_scope == 'windows-lifecycle'/);
  assert.match(lifecycle, /runs-on: windows-2022/);
  assert.match(lifecycle, /run: node scripts\/run-windows-lifecycle-tests.mjs/);
  assert.match(lifecycle, /workspaces: native-helpers\/windows-process-probe/);
  assert.doesNotMatch(lifecycle, /pnpm|provision-native-toolchains|continue-on-error/);
  for (const name of ['build', 'native-analyzers']) assert.match(job(name), /if: github.event_name != 'workflow_dispatch' \|\| inputs.validation_scope != 'windows-lifecycle'/);
  assert.match(job('publish'), /inputs.validation_scope == 'full' && inputs.publish/);
});
