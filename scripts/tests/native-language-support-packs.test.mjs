import { test } from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import { execFileSync, ChildProcess } from 'node:child_process';
import { packageDirectory } from '../build-language-support-packs.mjs';
import { copyToolTree, smokeNativeHelper } from '../native-language-support-packs.mjs';

/** Java 私有 runtime 的权限必须穿过打包、解压两层，不能只验证原目录。 */
test('native runtime executable survives archive extraction', async () => {
  const root = await fs.mkdtemp(path.join(os.tmpdir(), 'narrant-native-permissions-'));
  try {
    const input = path.join(root, 'input');
    await fs.mkdir(path.join(input, 'runtime/jdk/bin'), { recursive: true });
    await fs.writeFile(path.join(input, 'runtime/jdk/bin/java'), '#!/bin/sh\nexit 0\n', { mode: 0o755 });
    const output = await packageDirectory({ id: 'java', version: '0.1.0', target: 'aarch64-apple-darwin' }, input, path.join(root, 'output'));
    const manifest = JSON.parse(await fs.readFile(path.join(output, 'manifest.json')));
    assert.equal(manifest.files[0].executable, true);
    const extracted = path.join(root, 'extracted');
    await fs.mkdir(extracted);
    execFileSync('tar', ['-xzf', path.join(output, 'pack.tar.gz'), '-C', extracted]);
    if (process.platform !== 'win32') {
      assert.ok((await fs.stat(path.join(extracted, 'runtime/jdk/bin/java'))).mode & 0o111);
      execFileSync(path.join(extracted, 'runtime/jdk/bin/java'));
    }
  } finally { await fs.rm(root, { recursive: true, force: true }); }
});

/** SDK 内部链接转换为普通文件，包外链接必须在复制时失败。 */
test('SDK tree copy materializes internal files and rejects escape', { skip: process.platform === 'win32' }, async () => {
  const root = await fs.mkdtemp(path.join(os.tmpdir(), 'narrant-native-tree-'));
  try {
    const input = path.join(root, 'input');
    await fs.mkdir(input);
    await fs.writeFile(path.join(input, 'source'), 'declaration');
    await fs.symlink('source', path.join(input, 'link'));
    await copyToolTree(input, path.join(root, 'good'));
    assert.equal((await fs.lstat(path.join(root, 'good/link'))).isSymbolicLink(), false);
    await fs.writeFile(path.join(root, 'outside'), 'not approved');
    await fs.symlink('../outside', path.join(input, 'escape'));
    await assert.rejects(copyToolTree(input, path.join(root, 'bad')), /escapes approved root/);
  } finally { await fs.rm(root, { recursive: true, force: true }); }
});

/** ready 中的语言身份是包启动校验的一部分，不能只要求子进程能退出。 */
test('helper smoke rejects a mismatched implementation identity', async () => {
  await assert.rejects(smokeNativeHelper({ executable: process.execPath, args: ['-e', `console.log(JSON.stringify({protocol:'narrant.native-analyzer/1',type:'ready',helper_id:'other'})); process.stdin.resume()`], id: 'narrant-go', projection: 'go-source-v1', languages: ['go'], profile: 'go-static-v1' }), /identity mismatch/);
});

/** smoke 失败后必须等进程 close，才能删除它仍在使用的 scratch。 */
test('smoke waits for killed child close before rejecting and removing cwd', async (t) => {
  // 模拟异步终止延迟，让提前 reject 的旧实现稳定暴露清理竞态。
  const originalKill = ChildProcess.prototype.kill;
  const markerRoot = await fs.mkdtemp(path.join(os.tmpdir(), 'narrant-smoke-close-test-'));
  t.after(() => fs.rm(markerRoot, { recursive: true, force: true }));
  const marker = path.join(markerRoot, 'cwd');
  let child, closed = false;
  t.mock.method(ChildProcess.prototype, 'kill', function (signal) {
    child = this;
    this.once('close', () => { closed = true; });
    setTimeout(() => originalKill.call(this, signal), 150);
    return true;
  });
  await assert.rejects(smokeNativeHelper({ executable: process.execPath,
    args: ['-e', `require('node:fs').writeFileSync(${JSON.stringify(marker)}, process.cwd()); console.log(JSON.stringify({protocol:'narrant.native-analyzer/1',type:'ready',helper_id:'other'})); process.stdin.resume()`],
    id: 'narrant-go', projection: 'go-source-v1', languages: ['go'], profile: 'go-static-v1' }), /identity mismatch/);
  try { assert.equal(closed, true, 'smoke rejected before child close'); }
  finally { if (!closed && child) await new Promise(resolve => child.once('close', resolve)); }
  await assert.rejects(fs.stat(await fs.readFile(marker, 'utf8')), { code: 'ENOENT' });
});

/** 本地开发产物没有通过真实双平台验收前不会被发布脚本纳入。 */
test('native catalog remains explicitly unpublished', async () => {
  const catalog = JSON.parse(await fs.readFile(new URL('../../resources/language-support-packs.json', import.meta.url)));
  for (const id of ['java', 'go', 'csharp', 'c-cpp']) assert.equal(catalog.packs[id].published, false);
});
