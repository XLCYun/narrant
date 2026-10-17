import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { spawnSync } from 'node:child_process';
import test from 'node:test';

/** 读取实际工作流并统一换行，让 macOS 和 Windows 验证同一份调度逻辑。 */
function workflow(name) {
  return readFileSync(new URL(`../../.github/workflows/${name}.yml`, import.meta.url), 'utf8').replace(/\r\n/g, '\n');
}

const ci = workflow('ci');
const quality = workflow('quality-gate');
const languages = workflow('validate-language-support-packs');

test('every main pull request runs both complete acceptance suites, including drafts and documentation changes', () => {
  assert.match(ci, /pull_request:\n\s+branches: \[main\]/);
  assert.doesNotMatch(ci, /paths:|paths-ignore:|draft|pull_request_target|secrets:|contents: write/);
  assert.match(ci, /group: ci-\$\{\{ github.event.pull_request.number \}\}/);
  assert.match(ci, /cancel-in-progress: true/);
  assert.match(ci, /quality:\n\s+uses: \.\/.github\/workflows\/quality-gate.yml/);
  assert.match(ci, /languages:\n\s+uses: \.\/.github\/workflows\/validate-language-support-packs.yml/);
  for (const input of ['validation_scope: full', 'language: all', 'platform: all', 'upload_artifacts: false']) {
    assert.ok(ci.includes(input), input);
  }
  assert.doesNotMatch(workflow('language-support-packs'), /pull_request:/);
  assert.match(languages, /workflow_call:/);
  assert.doesNotMatch(languages, /contents: write|\n {2}publish:|secrets:/);
});

test('the required check runs after both suites even when one fails, skips or is cancelled', () => {
  assert.match(ci, /name: Complete CI\n\s+if: \$\{\{ always\(\) \}\}\n\s+needs: \[quality, languages\]/);
  const program = ci.match(/node --input-type=module -e '([\s\S]*?)'/)[1];
  const statuses = ['success', 'failure', 'cancelled', 'skipped', undefined];
  for (const qualityResult of statuses) {
    for (const languageResult of statuses) {
      const results = {};
      if (qualityResult) results.quality = { result: qualityResult };
      if (languageResult) results.languages = { result: languageResult };
      const result = spawnSync(process.execPath, ['--input-type=module', '-e', program], {
        env: { ...process.env, CI_RESULTS: JSON.stringify(results) }, encoding: 'utf8',
      });
      assert.ifError(result.error);
      assert.equal(result.status, qualityResult === 'success' && languageResult === 'success' ? 0 : 1, result.stdout + result.stderr);
    }
  }
});

test('application acceptance includes Rust and desktop E2E on both supported platforms', () => {
  assert.match(quality, /os: \[macos-15, windows-2022\]/);
  assert.match(quality, /runs-on: \$\{\{ matrix.os \}\}/);
  assert.match(quality, /NARRANT_WINDOWS_TEST_MANIFEST:.*windows-2022.*'1'/);
  for (const command of ['fmt --all', 'clippy --locked', '--all-targets', '--doc', 'pnpm lint', 'pnpm build', 'pnpm test', 'pnpm test:scripts']) {
    assert.ok(quality.includes(command), command);
  }
  // Windows 默认 PowerShell 以最后一个原生命令的退出码结束；每组测试独立报告失败。
  const rustTestSteps = quality.split(/(?=^ {6}- )/m).filter(step => step.includes('bash scripts/cargo-agent.sh test'));
  assert.equal(rustTestSteps.length, 2);
  for (const step of rustTestSteps) {
    assert.equal(step.match(/bash scripts\/cargo-agent\.sh test/g).length, 1);
  }
  for (const [job, runner] of [['windows-e2e', 'windows-2022'], ['macos-e2e', 'macos-15']]) {
    const source = quality.split(`\n  ${job}:`)[1].split(/\n {2}\S/)[0];
    assert.ok(source.includes(`runs-on: ${runner}`));
    assert.ok(source.includes('pnpm test:e2e'));
  }
});
