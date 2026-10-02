import { test } from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import { execFileSync, ChildProcess } from 'node:child_process';
import { packageDirectory } from '../build-language-support-packs.mjs';
import { copyToolTree, smokeNativeHelper, bundleCsharpReferences } from '../native-language-support-packs.mjs';

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

/** 两个 TFM 只获得各自引用目录，NuGet 构建脚本不会随包执行或进入产物。 */
test('C# bundle keeps net8 and net10 declarations isolated and excludes NuGet build assets', async () => {
  const root = await fs.mkdtemp(path.join(os.tmpdir(), 'narrant-reference-catalog-'));
  try {
    const sdk = path.join(root, 'sdk'), reference8 = path.join(root, 'reference8'), payload = path.join(root, 'payload');
    const catalog = JSON.parse(await fs.readFile(new URL('../../resources/language-support-packs.json', import.meta.url)));
    const definition = catalog.packs.csharp;
    for (const [directory, value] of [[path.join(sdk, 'packs/Microsoft.NETCore.App.Ref/10.0.0/ref/net10.0'), 'net10'], [path.join(reference8, 'ref/net8.0'), 'net8']]) {
      await fs.mkdir(directory, { recursive: true });
      await fs.writeFile(path.join(directory, 'System.Runtime.dll'), value);
    }
    await fs.writeFile(path.join(reference8, 'Microsoft.NETCore.App.Ref.nuspec'), '<package><metadata><id>Microsoft.NETCore.App.Ref</id><version>8.0.0</version></metadata></package>');
    for (const name of ['LICENSE.TXT', 'THIRD-PARTY-NOTICES.TXT']) await fs.writeFile(path.join(reference8, name), 'license');
    await fs.mkdir(path.join(reference8, 'build'));
    await fs.writeFile(path.join(reference8, 'build/untrusted.targets'), 'must not copy');
    const platforms = await bundleCsharpReferences({ definition, sdk, reference8, payload });
    assert.deepEqual(platforms.map(platform => [platform.metadata.tfm, platform.version]), [['net10.0', '10.0.0'], ['net8.0', '8.0.0']]);
    for (const target of ['net8', 'net10']) assert.equal(await fs.readFile(path.join(payload, `runtime/platform/${target}.0/System.Runtime.dll`), 'utf8'), target);
    assert.deepEqual((await fs.readdir(payload)).sort(), ['licenses', 'runtime']);
    await assert.rejects(fs.stat(path.join(payload, 'build')), { code: 'ENOENT' });
    assert.equal(await fs.readFile(path.join(payload, 'licenses/dotnet-ref8-LICENSE.TXT'), 'utf8'), 'license');
    await assert.rejects(bundleCsharpReferences({ definition: { ...definition, referenceTargets: [{ tfm: 'net9.0', version: '9.0.0' }] }, sdk, reference8, payload }), /Unsupported C# reference/);
    await fs.writeFile(path.join(reference8, 'Microsoft.NETCore.App.Ref.nuspec'), '<id>Microsoft.NETCore.App.Ref</id><version>8.0.1</version>');
    await assert.rejects(bundleCsharpReferences({ definition, sdk, reference8, payload }), /identity mismatch/);
  } finally { await fs.rm(root, { recursive: true, force: true }); }
});
