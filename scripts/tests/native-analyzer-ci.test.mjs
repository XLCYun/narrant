import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import { createHash } from 'node:crypto';
import { validateToolchainManifest, verifyArchive } from '../provision-native-toolchains.mjs';
import { validateNativeConfiguration, verificationCommands } from '../verify-native-analyzers.mjs';

const manifest = JSON.parse(await fs.readFile(new URL('../../resources/native-toolchains.json', import.meta.url), 'utf8'));
const workflowSource = await fs.readFile(new URL('../../.github/workflows/language-support-packs.yml', import.meta.url), 'utf8');

// 显式重放两种 checkout 换行，不能让本机 LF 掩盖 Windows CRLF 的路径断言失败。
for (const [lineEnding, newline] of [['LF', '\n'], ['CRLF', '\r\n']]) {
  const workflow = workflowSource.replace(/\r?\n/g, newline);

  // Windows 清理探针必须在下载 SDK 前失败，避免每次诊断都等待完整 Tauri 编译。
  test(`Windows process probe fails early with the setup Node executable and serialized Cargo (${lineEnding})`, () => {
    const nativeJob = workflow.split('\n  native-analyzers:')[1].split('\n  publish:')[0];
    const probeName = 'Probe Windows process cleanup before toolchain provisioning';
    const probeIndex = nativeJob.indexOf(probeName);
    assert.ok(probeIndex > nativeJob.indexOf('actions/setup-node@'));
    assert.ok(probeIndex > nativeJob.indexOf('dtolnay/rust-toolchain@stable'));
    assert.ok(probeIndex < nativeJob.indexOf('pnpm install --frozen-lockfile'));
    assert.ok(probeIndex < nativeJob.indexOf('scripts/provision-native-toolchains.mjs'));
    assert.equal(nativeJob.match(/dtolnay\/rust-toolchain@stable/g)?.length, 1);
    const probe = nativeJob.slice(probeIndex).split('\n      - ')[0];
    assert.match(probe, /if: runner\.os == 'Windows'/);
    assert.match(probe, /shell: pwsh/);
    assert.match(probe, /timeout-minutes: 5/);
    assert.ok(probe.includes('node scripts/run-windows-process-probe.mjs'));
    assert.doesNotMatch(probe, /Get-Command|NARRANT_TEST_NODE/);
    assert.match(probe, /if \(\$LASTEXITCODE -ne 0\) \{ throw /);
    assert.doesNotMatch(probe, /continue-on-error/);
  });

  // 快速诊断不替代完整验收，也不能改变主程序缓存或让探针源码漏掉 PR 触发。
  test(`early probe retains complete native acceptance and the separate Tauri cache (${lineEnding})`, () => {
    const nativeJob = workflow.split('\n  native-analyzers:')[1].split('\n  publish:')[0];
    assert.match(workflow.split('\njobs:')[0], /- native-helpers\/\*\*\r?\n/);
    assert.match(workflow.split('\njobs:')[0], /- scripts\/run-windows-process-probe\.mjs\r?\n/);
    assert.match(nativeJob, /node --test [^\n]*scripts\/tests\/run-windows-process-probe\.test\.mjs/);
    assert.match(nativeJob, /workspaces: src-tauri\r?\n\s+key: all-native-\$\{\{ matrix\.target \}\}/);
    assert.doesNotMatch(nativeJob, /CARGO_TARGET_DIR/);
    const acceptance = nativeJob.split('      - name: Build packs, run helper suites, Host lifecycle and public graph/diff conformance')[1].split('\n      - ')[0];
    assert.match(acceptance, /run: node scripts\/verify-native-analyzers\.mjs native-language-support-packs --build/);
    assert.doesNotMatch(acceptance, /if:|continue-on-error/);
  });
}

// 两个平台必须拥有全部六个固定工具，清单不能遗漏语言运行时。
test('official pinned manifest covers every required tool on both platforms', () => {
  assert.equal(validateToolchainManifest(manifest), manifest);
  for (const entries of Object.values(manifest.targets)) assert.deepEqual(entries.map(entry => entry.name).sort(), ['dotnet', 'dotnet-ref8', 'go', 'jdk17', 'jdk25', 'llvm']);
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
  config.csharp.platforms = [['net8.0', '8.0.0'], ['net10.0', '10.0.0']].map(([tfm, version]) => ({ id: tfm, kind: 'dotnet-reference-assemblies', version, root: `C:\\SDK\\${tfm}`, fingerprint: 'fixed', metadata: { tfm, reference_pack: 'Microsoft.NETCore.App.Ref', reference_pack_version: version } }));
  assert.equal(validateNativeConfiguration(config, 'win32'), config);
  const singleTarget = structuredClone(config);
  singleTarget.csharp.platforms.shift();
  assert.throws(() => validateNativeConfiguration(singleTarget, 'win32'), /net8.0/);
  const sharedRoot = structuredClone(config);
  sharedRoot.csharp.platforms[1].root = sharedRoot.csharp.platforms[0].root;
  assert.throws(() => validateNativeConfiguration(sharedRoot, 'win32'), /distinct roots/);
  const incomplete = structuredClone(config);
  delete incomplete.clang;
  assert.throws(() => validateNativeConfiguration(incomplete, 'win32'), /clang/);
  config.java.platforms[0].root = '../sdk';
  assert.throws(() => validateNativeConfiguration(config, 'win32'), /platform catalog/);
});

// 验收命令覆盖所有 helper，且 Cargo 统一串行经过仓库入口。
test('verification uses every helper suite and serialized Cargo host and real graph tests', () => {
  const environment = Object.fromEntries(['GO_SDK', 'DOTNET_SDK', 'DOTNET_REF8_ROOT', 'JAVA_RUNTIME', 'JAVA_TARGET_JDK', 'LLVM_SDK'].map(name => [`NARRANT_${name}`, '/isolated/tools']));
  const commands = verificationCommands(environment, 'win32');
  assert.equal(commands.length, 9);
  assert.equal(commands[2].environment.NARRANT_TEST_REFS8, path.join('/isolated/tools', 'ref/net8.0'));
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

// 共享 Job 对象的两个生命周期回归只在 Windows 执行，且不启动忽略的子进程 fixture。
test('Windows native acceptance includes focused shared Job regressions without expanding macOS tests', () => {
  const environment = Object.fromEntries(['GO_SDK', 'DOTNET_SDK', 'DOTNET_REF8_ROOT', 'JAVA_RUNTIME', 'JAVA_TARGET_JDK', 'LLVM_SDK'].map(name => [`NARRANT_${name}`, '/isolated/tools']));
  const filter = 'lsp::transport::windows_process::tests::windows_job_';
  const windows = verificationCommands(environment, 'win32');
  const regressions = windows.filter(command => command.args.includes(filter));
  assert.equal(regressions.length, 1);
  assert.deepEqual(regressions[0], {
    executable: 'bash',
    args: ['scripts/cargo-agent.sh', 'test', '--manifest-path', 'src-tauri/Cargo.toml', '--lib', filter, '--', '--nocapture', '--test-threads=1'],
  });
  const macOS = verificationCommands(environment, 'darwin');
  assert.equal(macOS.length, 8);
  assert.ok(macOS.every(command => !command.args.includes(filter)));
  assert.deepEqual(windows.slice(4).filter(command => !command.args.includes(filter)), macOS.slice(4));
});

// 引用专用 NuGet 包跨架构使用同一来源摘要，不能偷换为浮动版本或任意 NuGet 工具。
test('net8 standard references have one official architecture-independent SHA512 pin', () => {
  const references = Object.values(manifest.targets).map(entries => entries.find(entry => entry.name === 'dotnet-ref8'));
  assert.deepEqual(references[0], references[1]);
  assert.equal(references[0].algorithm, 'sha512');
  assert.equal(references[0].environment, 'NARRANT_DOTNET_REF8_ROOT');
  for (const mutation of [{ version: '8.0.1' }, { url: references[0].url.replace('8.0.0.nupkg', '8.0.1.nupkg') }, { algorithm: 'sha256', digest: 'a'.repeat(64) }]) {
    const invalid = structuredClone(manifest);
    Object.assign(invalid.targets['aarch64-apple-darwin'].find(entry => entry.name === 'dotnet-ref8'), mutation);
    assert.throws(() => validateToolchainManifest(invalid), /reference pack/);
  }
});

// 官方引用包使用 SHA512，字节篡改必须在解包前被发现。
test('reference package verification checks SHA512 content', async () => {
  const directory = await fs.mkdtemp(path.join(os.tmpdir(), 'native-reference-checksum-'));
  try {
    const archive = path.join(directory, 'reference.nupkg');
    await fs.writeFile(archive, 'official reference declarations');
    const entry = { name: 'dotnet-ref8', algorithm: 'sha512', digest: createHash('sha512').update('official reference declarations').digest('hex') };
    await verifyArchive(archive, entry);
    await fs.appendFile(archive, 'changed');
    await assert.rejects(verifyArchive(archive, entry), /Checksum mismatch/);
  } finally { await fs.rm(directory, { recursive: true, force: true }); }
});
