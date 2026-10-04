import { test } from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import { execFileSync, ChildProcess } from 'node:child_process';
import { packageDirectory } from '../build-language-support-packs.mjs';
import { copyToolTree, smokeNativeHelper, csharpFixturePlatforms } from '../native-language-support-packs.mjs';
import { prepareDotnetFixtureReferences } from '../provision-native-toolchains.mjs';

/** 私有 runtime 的执行入口必须穿过打包与解压，两个平台都真实启动解包后的文件。 */
test('native runtime executable survives archive extraction', async () => {
  const root = await fs.mkdtemp(path.join(os.tmpdir(), 'narrant-native-permissions-'));
  try {
    const input = path.join(root, 'input');
    const windows = process.platform === 'win32';
    const entry = windows ? 'runtime/jdk/bin/java.exe' : 'runtime/jdk/bin/java';
    await fs.mkdir(path.join(input, 'runtime/jdk/bin'), { recursive: true });
    if (windows) await fs.copyFile(process.execPath, path.join(input, entry));
    else await fs.writeFile(path.join(input, entry), '#!/bin/sh\nexit 0\n', { mode: 0o755 });
    const target = windows ? 'x86_64-pc-windows-msvc' : 'aarch64-apple-darwin';
    const output = await packageDirectory({ id: 'java', version: '0.1.0', target }, input, path.join(root, 'output'));
    const manifest = JSON.parse(await fs.readFile(path.join(output, 'manifest.json')));
    assert.equal(manifest.files.find(file => file.path === entry)?.executable, true);
    const extracted = path.join(root, 'extracted');
    await fs.mkdir(extracted);
    execFileSync('tar', ['-xzf', path.join(output, 'pack.tar.gz'), '-C', extracted]);
    const executable = path.join(extracted, entry);
    if (windows) assert.equal(execFileSync(executable, ['--version'], { encoding: 'utf8', timeout: 10_000 }).trim(), process.version);
    else {
      assert.ok((await fs.stat(executable)).mode & 0o111);
      execFileSync(executable, [], { timeout: 10_000 });
    }
  } finally { await fs.rm(root, { recursive: true, force: true }); }
});

/** Windows 的 .exe 入口不依赖 POSIX 位；同名非执行文件不会让 macOS 产物获得执行权限。 */
test('runtime executable metadata follows target platform', async () => {
  const root = await fs.mkdtemp(path.join(os.tmpdir(), 'narrant-runtime-target-'));
  try {
    const input = path.join(root, 'input');
    await fs.mkdir(path.join(input, 'runtime/jdk/bin'), { recursive: true });
    await fs.writeFile(path.join(input, 'runtime/jdk/bin/java.exe'), 'fixture', { mode: 0o644 });
    await fs.writeFile(path.join(input, 'runtime/jdk/bin/java.dll'), 'fixture', { mode: 0o644 });
    for (const target of ['x86_64-pc-windows-msvc', 'aarch64-apple-darwin']) {
      const output = await packageDirectory({ id: 'java', version: '0.1.0', target }, input, path.join(root, 'output'));
      const manifest = JSON.parse(await fs.readFile(path.join(output, 'manifest.json')));
      assert.equal(manifest.files.find(file => file.path.endsWith('java.exe')).executable, target === 'x86_64-pc-windows-msvc');
      assert.equal(manifest.files.find(file => file.path.endsWith('java.dll')).executable, false);
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

/** 标准引用只出现在本机测试 SDK，生成的目录描述允许新 TFM/补丁且不复制构建资产。 */
test('C# fixture discovery uses installed SDK layout without bundling target references', async () => {
  const root = await fs.mkdtemp(path.join(os.tmpdir(), 'narrant-reference-discovery-'));
  try {
    const sdk = path.join(root, 'sdk'), source = path.join(root, 'reference9');
    await fs.mkdir(path.join(source, 'ref/net9.0'), { recursive: true });
    await fs.writeFile(path.join(source, 'ref/net9.0/System.Runtime.dll'), 'net9 reference');
    await fs.writeFile(path.join(source, 'Microsoft.NETCore.App.Ref.nuspec'), '<id>Microsoft.NETCore.App.Ref</id><version>9.0.20</version>');
    await fs.mkdir(path.join(source, 'build'));
    await fs.writeFile(path.join(source, 'build/untrusted.targets'), 'must not copy');
    await prepareDotnetFixtureReferences({ NARRANT_DOTNET_SDK: sdk, NARRANT_DOTNET_REF9_ROOT: source });
    const platforms = await csharpFixturePlatforms(sdk);
    assert.equal(platforms.length, 1);
    assert.equal(platforms[0].version, '9.0.20');
    assert.equal(platforms[0].metadata.tfm, 'net9.0');
    assert.equal(await fs.readFile(path.join(platforms[0].root, 'System.Runtime.dll'), 'utf8'), 'net9 reference');
    assert.equal(platforms[0].root, path.join(sdk, 'packs/Microsoft.NETCore.App.Ref/9.0.20/ref/net9.0'));
    await assert.rejects(fs.stat(path.join(sdk, 'packs/Microsoft.NETCore.App.Ref/9.0.20/build')), { code: 'ENOENT' });
    const catalog = JSON.parse(await fs.readFile(new URL('../../resources/language-support-packs.json', import.meta.url)));
    assert.equal(catalog.packs.csharp.referenceTargets, undefined);
    assert.equal(catalog.packs.csharp.targetStandardLibrary, 'local-dotnet-reference-packs');
  } finally { await fs.rm(root, { recursive: true, force: true }); }
});
