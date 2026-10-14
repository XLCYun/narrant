import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import { runInNewContext } from 'node:vm';
import { runWindowsLifecycleTests } from '../run-windows-lifecycle-tests.mjs';

// 非 Windows 主机不能因为 cfg 排除了全部测试而得到成功结论。
test('lifecycle validation rejects non-Windows hosts before starting Cargo', async () => {
  await assert.rejects(runWindowsLifecycleTests({ platform: 'darwin' }), /requires a Windows runner/);
});

// 跨真实 bash/Node 边界验证生产所有者筛选器、原生 Node 路径和真实失败传播。
test('lifecycle validation runs only the production owner and preserves its failure', async t => {
  const diagnostics = t.mock.method(console, 'error', () => {});
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
assert.equal(filter, 'child_process::');
assert.deepEqual(args, [...common, filter, '--', '--nocapture', '--test-threads=1']);
const previous = fs.existsSync('executed') ? fs.readFileSync('executed', 'utf8') : '';
assert.equal(previous, '');
fs.appendFileSync('executed', filter);
process.exit(7);
`);
    await assert.rejects(runWindowsLifecycleTests({ cwd: directory, platform: 'win32' }), error => {
      assert.ok(error instanceof AggregateError);
      assert.equal(error.errors.length, 1);
      assert.match(error.errors[0].cause.message, /failed \(7\)/);
      return true;
    });
    assert.equal(await fs.readFile(path.join(directory, 'executed'), 'utf8'), 'child_process::');
    assert.equal(diagnostics.mock.callCount(), 1);
    assert.match(diagnostics.mock.calls[0].arguments[0].message, /child_process::/);
    assert.match(diagnostics.mock.calls[0].arguments[0].cause.message, /failed \(7\)/);
  } finally {
    await fs.rm(directory, { recursive: true, force: true });
  }
});

// 轻量诊断独占 Windows；聚焦语言只运行 native 验收，发布必须等待完整双平台且显式授权。
test('manual lifecycle scope isolates one cheap job and cannot publish packs', async () => {
  const workflow = (await fs.readFile(new URL('../../.github/workflows/language-support-packs.yml', import.meta.url), 'utf8')).replace(/\r\n/g, '\n');
  const job = name => workflow.split(`\n  ${name}:\n`)[1].split(/\n {2}[\w-]+:\n/)[0];
  assert.match(workflow, /validation_scope:\n[\s\S]*default: full/);
  const lifecycle = job('windows-lifecycle');
  assert.match(lifecycle, /if: github.event_name == 'workflow_dispatch' && inputs.validation_scope == 'windows-lifecycle'/);
  assert.match(lifecycle, /runs-on:.*fromJSON\(vars\.NARRANT_WINDOWS_RUNNER_LABELS\).*\|\| 'windows-2022'/);
  assert.match(lifecycle, /run: node scripts\/run-windows-lifecycle-tests.mjs/);
  assert.match(lifecycle, /workspaces: native-helpers\/windows-process-probe/);
  assert.doesNotMatch(lifecycle, /pnpm|provision-native-toolchains|continue-on-error/);
  assert.match(job('publish'), /needs: \[build, native-analyzers\]/);
  /** 这些 GitHub guards 只使用 JS 同义的布尔运算；直接验证实际条件，避免要求不同职责共享文本。 */
  const enabled = (name, event, scope, language, publish, ref, platform = 'all') => runInNewContext(job(name).match(/^ {4}if: (.+)$/m)[1], {
    github: { event_name: event, ref }, inputs: { validation_scope: scope, language, publish, platform },
  });
  for (const scope of ['full', 'windows-lifecycle']) {
    for (const language of ['all', 'typescript-javascript', 'java']) {
      const context = ['workflow_dispatch', scope, language, true, 'refs/heads/main'];
      assert.equal(enabled('windows-lifecycle', ...context), scope === 'windows-lifecycle');
      assert.equal(enabled('build', ...context), scope === 'full' && language === 'all');
      assert.equal(enabled('native-analyzers', ...context), scope === 'full');
      assert.equal(enabled('publish', ...context), scope === 'full' && language === 'all');
    }
  }
  for (const [event, publish, ref] of [['pull_request', true, 'refs/heads/main'], ['workflow_dispatch', false, 'refs/heads/main'], ['workflow_dispatch', true, 'refs/heads/codex/migration']]) {
    assert.equal(enabled('publish', event, 'full', 'all', publish, ref), false);
  }
  for (const platform of ['macos', 'windows']) {
    assert.equal(enabled('publish', 'workflow_dispatch', 'full', 'all', true, 'refs/heads/main', platform), false);
  }
  for (const name of ['build', 'native-analyzers']) assert.equal(enabled(name, 'pull_request', undefined, undefined, undefined, 'refs/pull/1/merge'), true);
  assert.equal(enabled('windows-lifecycle', 'pull_request', undefined, undefined, undefined, 'refs/pull/1/merge'), false);
});

// 执行工作流实际矩阵表达式，确认单平台验收不会调度另一平台，默认仍覆盖两个目标。
test('manual platform selection schedules only the requested native targets', async () => {
  const workflow = (await fs.readFile(new URL('../../.github/workflows/language-support-packs.yml', import.meta.url), 'utf8')).replace(/\r\n/g, '\n');
  for (const name of ['build', 'native-analyzers']) {
    const job = workflow.split(`\n  ${name}:\n`)[1].split(/\n {2}[\w-]+:\n/)[0];
    const expression = job.match(/include: >-\n([\s\S]*?)\n {4}runs-on:/)?.[1].trim();
    assert.ok(expression?.startsWith('${{') && expression.endsWith('}}'));
    for (const platform of [undefined, 'all', 'macos', 'windows']) {
      const selected = runInNewContext(expression.slice(3, -2), { inputs: { platform }, fromJSON: JSON.parse });
      const targets = selected.map(item => item.target);
      assert.deepEqual(targets, platform === 'windows' ? ['x86_64-pc-windows-msvc'] : platform === 'macos' ? ['aarch64-apple-darwin'] : ['aarch64-apple-darwin', 'x86_64-pc-windows-msvc']);
      assert.ok(selected.every(item => item.runner === (item.target === 'aarch64-apple-darwin' ? 'macos-15' : name === 'build' ? 'windows-latest' : 'windows-2022')));
    }
  }
});
