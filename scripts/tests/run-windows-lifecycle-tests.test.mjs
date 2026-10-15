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
  for (const scope of ['full', 'native-analyzers', 'windows-lifecycle', 'macos-runner']) {
    for (const language of ['all', 'typescript-javascript', 'java']) {
      const context = ['workflow_dispatch', scope, language, true, 'refs/heads/main'];
      assert.equal(enabled('windows-lifecycle', ...context), scope === 'windows-lifecycle');
      assert.equal(enabled('macos-runner', ...context), scope === 'macos-runner');
      assert.equal(enabled('build', ...context), scope === 'full' && language === 'all');
      assert.equal(enabled('native-analyzers', ...context), !['windows-lifecycle', 'macos-runner'].includes(scope));
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
  assert.equal(enabled('macos-runner', 'pull_request', undefined, undefined, undefined, 'refs/pull/1/merge'), false);
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

// 自托管机器省去大型远端缓存；托管机器仍保留原缓存行为。
test('self-hosted runners skip remote Rust build caches', async () => {
  const workflow = (await fs.readFile(new URL('../../.github/workflows/language-support-packs.yml', import.meta.url), 'utf8')).replace(/\r\n/g, '\n');
  for (const name of ['windows-lifecycle', 'build', 'native-analyzers']) {
    const job = workflow.split(`\n  ${name}:\n`)[1].split(/\n {2}[\w-]+:\n/)[0];
    const cache = job.split('      - uses: Swatinem/rust-cache@v2\n')[1].split('\n      - ')[0];
    const condition = cache.match(/^ {8}if: (.+)$/m)?.[1] ?? 'true';
    for (const os of ['Windows', 'macOS', 'Linux']) {
      for (const environment of ['self-hosted', 'github-hosted']) {
        assert.equal(runInNewContext(condition, { runner: { os, environment } }),
          environment !== 'self-hosted', `${name}: ${os}/${environment}`);
      }
    }
  }
});

// 两个平台的标签分别生效；缺少标签时保留对应托管镜像，不能把任务发往另一平台。
test('self-hosted runner routing remains independent for each platform', async () => {
  const workflow = await fs.readFile(new URL('../../.github/workflows/language-support-packs.yml', import.meta.url), 'utf8');
  for (const name of ['build', 'native-analyzers']) {
    const job = workflow.split(`\n  ${name}:\n`)[1].split(/\n {2}[\w-]+:\n/)[0];
    const expression = job.match(/^ {4}runs-on: \$\{\{ (.+) \}\}$/m)[1];
    for (const target of ['aarch64-apple-darwin', 'x86_64-pc-windows-msvc']) {
      for (const mac of ['', '["self-hosted","macOS","ARM64","narrant-ci"]']) {
        for (const windows of ['', '["self-hosted","Windows","X64","narrant-ci"]']) {
          const labels = target === 'aarch64-apple-darwin' ? mac : windows;
          const result = runInNewContext(expression, {
            vars: { NARRANT_MACOS_RUNNER_LABELS: mac, NARRANT_WINDOWS_RUNNER_LABELS: windows },
            matrix: { target, runner: 'original-hosted-runner' }, fromJSON: JSON.parse,
          });
          assert.deepEqual(result, labels ? JSON.parse(labels) : 'original-hosted-runner');
        }
      }
    }
  }
});

// 仅验收时可省去产物存储；自动 PR 与正式发布仍必须上传，不能用此开关绕过发布输入。
test('manual validation can omit artifacts while pull requests and publishing retain uploads', async () => {
  const workflow = (await fs.readFile(new URL('../../.github/workflows/language-support-packs.yml', import.meta.url), 'utf8')).replace(/\r\n/g, '\n');
  for (const name of ['build', 'native-analyzers']) {
    const job = workflow.split(`\n  ${name}:\n`)[1].split(/\n {2}[\w-]+:\n/)[0];
    const upload = job.split('      - uses: actions/upload-artifact@')[1].split('\n      - ')[0];
    const condition = upload.match(/^ {8}if: (.+)$/m)?.[1] ?? 'success()';
    for (const [event, uploadArtifacts, publish, expected] of [
      ['workflow_dispatch', false, false, false],
      ['workflow_dispatch', true, false, true],
      ['workflow_dispatch', false, true, true],
      ['workflow_dispatch', true, true, true],
      ['pull_request', undefined, undefined, true],
    ]) {
      const selected = runInNewContext(condition, {
        github: { event_name: event }, inputs: { upload_artifacts: uploadArtifacts, publish },
        always: () => true, success: () => true,
      });
      assert.equal(Boolean(selected), expected, `${name}: ${event}, upload=${uploadArtifacts}, publish=${publish}`);
    }
  }
  assert.match(workflow, /upload_artifacts:\n[\s\S]*?type: boolean\n\s+default: true/);
});
