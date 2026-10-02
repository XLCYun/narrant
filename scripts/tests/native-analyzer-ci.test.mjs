import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import { createHash } from 'node:crypto';
import { validateToolchainManifest, verifyArchive } from '../provision-native-toolchains.mjs';
import { validateNativeConfiguration, verificationCommands } from '../verify-native-analyzers.mjs';

const manifest = JSON.parse(await fs.readFile(new URL('../../resources/native-toolchains.json', import.meta.url), 'utf8'));

// 两个平台必须拥有全部五个固定工具，清单不能遗漏语言运行时。
test('official pinned manifest covers every required tool on both platforms', () => {
  assert.equal(validateToolchainManifest(manifest), manifest);
  for (const entries of Object.values(manifest.targets)) assert.deepEqual(entries.map(entry => entry.name).sort(), ['dotnet', 'go', 'jdk17', 'jdk25', 'llvm']);
});

// 恶意或不完整清单必须在任何下载和解包发生前被拒绝。
test('untrusted downloads, missing digests and escaping homes fail before installation', () => {
  for (const mutation of [{ url: 'https://example.org/LLVM-latest.tar.xz' }, { digest: 'deadbeef' }, { home: '../outside' }, { home: 'C:/outside' }]) {
    const invalid = structuredClone(manifest);
    Object.assign(invalid.targets['x86_64-pc-windows-msvc'][0], mutation);
    assert.throws(() => validateToolchainManifest(invalid));
  }
});

// 篡改下载文件必须触发摘要失败，而不是继续安装。
test('download verification rejects a modified archive with a fixed SHA256', async () => {
  const directory = await fs.mkdtemp(path.join(os.tmpdir(), 'native-checksum-'));
  try {
    const file = path.join(directory, 'tool.tar.gz');
    await fs.writeFile(file, 'original');
    const entry = { name: 'fixture', algorithm: 'sha256', digest: createHash('sha256').update('original').digest('hex') };
    await verifyArchive(file, entry);
    await fs.writeFile(file, 'tampered');
    await assert.rejects(verifyArchive(file, entry), /Checksum mismatch/);
  } finally { await fs.rm(directory, { recursive: true, force: true }); }
});

// Windows 空格路径可用，缺任意 helper 或相对 SDK 根都要失败。
test('all four helpers and narrow absolute platform roots are mandatory', () => {
  const config = Object.fromEntries(['go', 'java', 'csharp', 'clang'].map(id => [id, { executable: `C:\\Native Tools\\${id}.exe`, args: [], trusted_roots: ['C:\\Native Tools'], platforms: [{ id: 'fixed', kind: 'sdk', version: '1', root: 'C:\\SDK\\1', fingerprint: 'fixed' }] }]));
  assert.equal(validateNativeConfiguration(config, 'win32'), config);
  const incomplete = structuredClone(config);
  delete incomplete.clang;
  assert.throws(() => validateNativeConfiguration(incomplete, 'win32'), /clang/);
  config.java.platforms[0].root = '../sdk';
  assert.throws(() => validateNativeConfiguration(config, 'win32'), /platform catalog/);
});

// 验收命令覆盖所有 helper，且 Cargo 统一串行经过仓库入口。
test('verification uses every helper suite and serialized Cargo host and real graph tests', () => {
  const environment = Object.fromEntries(['GO_SDK', 'DOTNET_SDK', 'JAVA_RUNTIME', 'JAVA_TARGET_JDK', 'LLVM_SDK'].map(name => [`NARRANT_${name}`, '/isolated/tools']));
  const commands = verificationCommands(environment, 'win32');
  assert.equal(commands.length, 8);
  assert.match(commands[0].executable, /go\.exe$/);
  assert.deepEqual(commands.slice(1, 4).map(command => command.args[4]), ['native-helpers/java/tests', 'native-helpers/csharp/tests', 'native-helpers/clang/tests']);
  for (const command of commands.slice(4)) {
    assert.equal(command.executable, 'bash');
    assert.equal(command.args[0], 'scripts/cargo-agent.sh');
  }
  assert.ok(commands.at(-1).args.includes('native_helper_graphs'));
  assert.ok(commands.at(-1).args.includes('--ignored'));
  assert.ok(commands.at(-1).args.includes('--test-threads=1'));
  assert.throws(() => verificationCommands({}), /Missing fixed toolchain/);
});
