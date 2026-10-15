import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import test from 'node:test';
import { runInNewContext } from 'node:vm';

// 执行实际调度表达式，覆盖未配置变量与两个平台，防止任务回退到付费镜像。
test('all workflow jobs default to self-hosted runners and keep desktop jobs separate', async () => {
  for (const name of ['quality-gate', 'release', 'promote-stable', 'update-smoke', 'language-support-packs']) {
    const source = await readFile(new URL(`../../.github/workflows/${name}.yml`, import.meta.url), 'utf8');
    for (const job of source.split('\njobs:\n')[1].split(/\n(?= {2}[\w-]+:\n)/)) {
      if (/^ {4}uses:/m.test(job)) continue;
      const expression = job.match(/^ {4}runs-on: \$\{\{ (.+) \}\}$/m)?.[1];
      assert.ok(expression, `${name}: missing self-hosted routing`);
      for (const platform of ['macos', 'windows']) {
        const labels = runInNewContext(expression, {
          vars: {}, fromJSON: JSON.parse,
          matrix: { runner: platform, target: platform === 'macos' ? 'aarch64-apple-darwin' : 'x86_64-pc-windows-msvc' },
        });
        assert.ok(Array.isArray(labels) && labels.includes('self-hosted'), `${name}: ${labels}`);
        const desktop = /^(windows-e2e|macos-e2e|native-smoke):/.test(job.trimStart());
        assert.equal(labels.includes('narrant-desktop-ci'), desktop, name);
      }
    }
    assert.doesNotMatch(source, /Swatinem\/rust-cache|cache: pnpm|sudo apt-get/);
  }
});

// 发布仍经过原质量门禁；迁移不能把桌面检查或签名凭据清理变成可忽略步骤。
test('release retains required quality and persistent-host signing cleanup', async () => {
  const release = await readFile(new URL('../../.github/workflows/release.yml', import.meta.url), 'utf8');
  assert.equal((release.match(/needs: \[validate, quality-gate\]/g) ?? []).length, 2);
  assert.match(release, /name: 恢复 CI 钥匙串并删除本次签名凭据\n\s+if: always\(\)/);
  assert.match(release, /list-keychains.*narrant-signing-keychains\.txt/);
  assert.match(release, /delete-keychain/);
  assert.doesNotMatch(release, /continue-on-error/);
  const quality = await readFile(new URL('../../.github/workflows/quality-gate.yml', import.meta.url), 'utf8');
  assert.equal((quality.match(/scripts\/verify-desktop-runner\.mjs/g) ?? []).length, 2);
  assert.match(quality, /cargo-agent\.sh test --manifest-path src-tauri\/Cargo\.toml --all-targets/);
  assert.match(quality, /cargo-agent\.sh test --manifest-path src-tauri\/Cargo\.toml --doc/);
  assert.doesNotMatch(quality, /continue-on-error/);
});
