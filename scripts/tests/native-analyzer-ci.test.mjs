import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import { createHash } from 'node:crypto';
import { execFileSync, spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import { validateToolchainManifest, verifyArchive } from '../provision-native-toolchains.mjs';
import { runVerificationCommands, validateNativeConfiguration, verificationCommands } from '../verify-native-analyzers.mjs';

const manifest = JSON.parse(await fs.readFile(new URL('../../resources/native-toolchains.json', import.meta.url), 'utf8'));
const workflowSource = await fs.readFile(new URL('../../.github/workflows/language-support-packs.yml', import.meta.url), 'utf8');

// 用真实 Cargo 解析生产命令生成的 runner 配置，覆盖 Node → Git Bash → 缓存入口边界。
test('Rust helper runner configuration survives the complete Cargo agent chain', async () => {
  const directory = await fs.mkdtemp(path.join(os.tmpdir(), 'narrant cargo config 空格-'));
  try {
    const manifestPath = path.join(directory, 'Cargo.toml');
    await fs.writeFile(manifestPath, '[package]\nname = "runner_config_regression"\nversion = "0.1.0"\nedition = "2021"\n[lib]\npath = "lib.rs"\n');
    await fs.writeFile(path.join(directory, 'lib.rs'), '');
    const command = verificationCommands({ NARRANT_PROJECT_RUST_TOOLCHAIN: 'stable', NARRANT_TEST_CARGO_TOOLCHAINS: 'fixture', NARRANT_TEST_CARGO_194: 'fixture' }, process.platform, 'rust')[0];
    const runner = command.args[command.args.indexOf('--config') + 1];
    // metadata 不编译、不下载；沿用已安装工具链，保留导致 Windows 转义丢失的完整调用链。
    const result = spawnSync(command.executable, [command.args[0], 'metadata', '--offline', '--no-deps', '--format-version', '1', '--manifest-path', manifestPath, '--config', runner], {
      cwd: fileURLToPath(new URL('../../', import.meta.url)),
      encoding: 'utf8',
      timeout: 20_000,
    });
    assert.equal(result.status, 0, result.error?.message ?? result.stderr);
    assert.equal(JSON.parse(result.stdout).packages[0].name, 'runner_config_regression');
  } finally { await fs.rm(directory, { recursive: true, force: true }); }
});

// 显式测试标记保留库测试 manifest，但应用目标必须复用 Tauri 的资源，避免重复资源 1。
test('Windows native test manifest keeps Common Controls and excludes duplicate app generation', async () => {
  const build = (await fs.readFile(new URL('../../src-tauri/build.rs', import.meta.url), 'utf8')).replace(/\r\n/g, '\n');
  const configuration = build.split('fn configure_windows_native_tests() {')[1].split('\n}\n')[0];
  assert.match(configuration, /var\("CARGO_CFG_TARGET_OS"\)\.as_deref\(\) == Ok\("windows"\)/);
  assert.match(configuration, /var\("CARGO_CFG_TARGET_ENV"\)\.as_deref\(\) == Ok\("msvc"\)/);
  assert.match(configuration, /var\(FLAG\)\.as_deref\(\) == Ok\("1"\)/);
  assert.match(configuration, /cargo:rerun-if-env-changed=\{FLAG\}/);
  const directives = [...configuration.matchAll(/println!\("(cargo:rustc-link-arg[^"\n]*)"\);/g)].map(match => match[1]);
  assert.deepEqual(directives, [
    'cargo:rustc-link-arg=/MANIFEST:EMBED',
    "cargo:rustc-link-arg=/MANIFESTDEPENDENCY:type='win32' name='Microsoft.Windows.Common-Controls' version='6.0.0.0' processorArchitecture='*' publicKeyToken='6595b64144ccf1df' language='*'",
    'cargo:rustc-link-arg-bin=narrant=/MANIFEST:NO',
  ]);
  assert.match(build, /configure_windows_native_tests\(\);\s+tauri_build::build\(\)/);
});

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
  test(`early probe retains complete native acceptance with local caches (${lineEnding})`, () => {
    const nativeJob = workflow.split('\n  native-analyzers:')[1].split('\n  publish:')[0];
    assert.match(workflow.split('\njobs:')[0], /- native-helpers\/\*\*\r?\n/);
    assert.match(workflow.split('\njobs:')[0], /- scripts\/run-windows-process-probe\.mjs\r?\n/);
    assert.match(nativeJob, /node --test [^\n]*scripts\/tests\/run-windows-process-probe\.test\.mjs/);
    assert.doesNotMatch(nativeJob, /Swatinem\/rust-cache/);
    assert.doesNotMatch(nativeJob, /CARGO_TARGET_DIR/);
    const acceptance = nativeJob.split('      - name: Build packs, run helper suites, Host lifecycle and public graph/diff conformance')[1].split('\n      - ')[0];
    assert.match(acceptance, /run: node scripts\/verify-native-analyzers\.mjs native-language-support-packs --build/);
    assert.doesNotMatch(acceptance, /if:|continue-on-error/);
  });
}

// 两个平台必须拥有全部固定构建工具和引用夹具，清单不能遗漏语言运行时。
test('official pinned manifest covers every required tool on both platforms', () => {
  assert.equal(validateToolchainManifest(manifest), manifest);
  for (const entries of Object.values(manifest.targets)) assert.deepEqual(entries.map(entry => entry.name).sort(), ['cmake', 'dotnet', 'dotnet-ref8', 'dotnet-ref9', 'go', 'go-target', 'jdk17', 'jdk25', 'jdk8', 'llvm']);
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
test('all enabled helpers and narrow absolute platform roots are mandatory', () => {
  const config = Object.fromEntries(['rust', 'python', 'typescript', 'go', 'java', 'csharp', 'clang'].map(id => [id, { executable: `C:\\Native Tools\\${id}.exe`, args: [], trusted_roots: ['C:\\Native Tools'], platforms: [{ id: 'fixed', kind: 'sdk', version: '1', root: 'C:\\SDK\\1' }] }]));
  config.csharp.platforms = [['net8.0', '8.0.0'], ['net9.0', '9.0.20'], ['net10.0', '10.0.0']].map(([tfm, version]) => ({ id: tfm, kind: 'dotnet-reference-assemblies', version, root: `C:\\SDK\\${tfm}`, metadata: { tfm, reference_pack: 'Microsoft.NETCore.App.Ref', reference_pack_version: version } }));
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
  const environment = Object.fromEntries(['GO_SDK', 'GO_TARGET_SDK', 'DOTNET_SDK', 'DOTNET_REF8_ROOT', 'DOTNET_REF9_ROOT', 'JAVA_RUNTIME', 'JAVA_TARGET_JDK', 'JAVA_TARGET_8', 'LLVM_SDK', 'PROJECT_RUST_TOOLCHAIN', 'TEST_CARGO_TOOLCHAINS', 'TEST_CARGO_194'].map(name => [`NARRANT_${name}`, '/isolated/tools']));
  const commands = verificationCommands(environment, 'win32');
  assert.equal(commands.length, 25);
  assert.equal(commands[2].environment.NARRANT_TEST_REFS8, path.join('/isolated/tools', 'ref/net8.0'));
  assert.match(commands[0].executable, /go\.exe$/);
  assert.deepEqual(commands.slice(1, 4).map(command => command.args[4]), ['native-helpers/java/tests', 'native-helpers/csharp/tests', 'native-helpers/clang/tests']);
  for (const command of commands.slice(4).filter(command => command.executable === 'bash')) {
    assert.equal(command.executable, 'bash');
    assert.equal(command.args[0], 'scripts/cargo-agent.sh');
  }
  assert.ok(commands.at(-1).args.includes('native_helper_graphs'));
  assert.ok(commands.at(-1).args.includes('--ignored'));
  assert.ok(commands.at(-1).args.includes('--test-threads=1'));
  assert.throws(() => verificationCommands({}), /Missing fixed toolchain/);
});

// 公共进程所有者和原生协议在两个平台使用相同的实际验收入口。
test('shared process and native acceptance use the same live groups on both platforms', () => {
  const environment = Object.fromEntries(['GO_SDK', 'GO_TARGET_SDK', 'DOTNET_SDK', 'DOTNET_REF8_ROOT', 'DOTNET_REF9_ROOT', 'JAVA_RUNTIME', 'JAVA_TARGET_JDK', 'JAVA_TARGET_8', 'LLVM_SDK', 'PROJECT_RUST_TOOLCHAIN', 'TEST_CARGO_TOOLCHAINS', 'TEST_CARGO_194'].map(name => [`NARRANT_${name}`, '/isolated/tools']));
  const ownerFilter = 'child_process::';
  const windows = verificationCommands(environment, 'win32');
  const macOS = verificationCommands(environment, 'darwin');
  assert.equal(macOS.length, 25);
  for (const commands of [windows, macOS]) {
    const owners = commands.filter(command => command.args.includes(ownerFilter));
    assert.equal(owners.length, 1);
    assert.deepEqual(owners[0].args, ['scripts/cargo-agent.sh', 'test', '--manifest-path', 'src-tauri/Cargo.toml', '--lib', ownerFilter, '--', '--nocapture', '--test-threads=1']);
  }
  for (const commands of [windows, macOS]) assert.doesNotMatch(JSON.stringify(commands), /lsp::/);
  // 两个平台的 runner 可使用不同分隔符；只规范这些路径，其他命令与参数仍严格对照。
  const runnerPrefix = "target.'cfg(all())'.runner=";
  const normalizeRunnerPaths = commands => commands.map(command => ({
    ...command,
    args: command.args.map(arg => arg.startsWith(runnerPrefix)
      ? [runnerPrefix, ...JSON.parse(arg.slice(runnerPrefix.length)).map(file => path.normalize(file))]
      : arg),
  }));
  assert.deepEqual(normalizeRunnerPaths(windows.slice(4)), normalizeRunnerPaths(macOS.slice(4)));
});

// 前组真实子进程失败后仍必须执行后组，同时整体保持失败，不能只收集成功日志。
test('failed validation child does not prevent later child execution or make the result successful', async t => {
  const diagnostics = t.mock.method(console, 'error', () => {});
  const directory = await fs.mkdtemp(path.join(os.tmpdir(), 'native validation groups '));
  const marker = path.join(directory, 'executed');
  const commands = [7, 0].map(code => ({
    executable: process.execPath,
    args: ['-e', `require('node:fs').appendFileSync(process.argv[1], '${code}\\n'); process.exit(${code})`, marker],
  }));
  try {
    await assert.rejects(runVerificationCommands(commands, process.env), error => {
      assert.ok(error instanceof AggregateError);
      assert.equal(error.errors.length, 1);
      assert.ok(error.errors[0].message.includes(commands[0].args[1]));
      assert.match(error.errors[0].cause.message, /Native validation failed \(7\)/);
      return true;
    });
    assert.equal(await fs.readFile(marker, 'utf8'), '7\n0\n');
    assert.equal(diagnostics.mock.callCount(), 1);
    assert.match(diagnostics.mock.calls[0].arguments[0].cause.message, /failed \(7\)/);
  } finally { await fs.rm(directory, { recursive: true, force: true }); }
});

// 多组失败保留各自命令和原始错误，异步执行始终串行且次序不变。
test('validation collects multiple original failures in order without overlapping groups', async t => {
  const diagnostics = t.mock.method(console, 'error', () => {});
  const commands = ['first', 'middle', 'last'].map(name => ({ executable: name, args: ['--fixture'] }));
  const failures = [new Error('first original failure'), new Error('last original failure')];
  const environment = { FIXTURE: 'same environment' };
  const events = [];
  let running = false;
  await assert.rejects(runVerificationCommands(commands, environment, async (command, passedEnvironment) => {
    assert.equal(running, false);
    assert.equal(passedEnvironment, environment);
    running = true;
    events.push(`start:${command.executable}`);
    await new Promise(resolve => setImmediate(resolve));
    events.push(`end:${command.executable}`);
    running = false;
    if (command === commands[0]) throw failures[0];
    if (command === commands[2]) throw failures[1];
  }), error => {
    assert.ok(error instanceof AggregateError);
    assert.equal(error.errors.length, 2);
    assert.equal(error.errors[0].cause, failures[0]);
    assert.equal(error.errors[1].cause, failures[1]);
    assert.match(error.errors[0].message, /first --fixture/);
    assert.match(error.errors[1].message, /last --fixture/);
    return true;
  });
  assert.deepEqual(events, ['start:first', 'end:first', 'start:middle', 'end:middle', 'start:last', 'end:last']);
  assert.deepEqual(diagnostics.mock.calls.map(call => call.arguments[0].cause), failures);
  assert.match(diagnostics.mock.calls[0].arguments[0].message, /first --fixture/);
  assert.match(diagnostics.mock.calls[1].arguments[0].message, /last --fixture/);
});

// 全部验收成功时正常完成，不能因汇总机制产生伪失败。
test('successful validation groups all complete normally', async () => {
  const commands = ['one', 'two'].map(executable => ({ executable, args: [] }));
  const visited = [];
  await runVerificationCommands(commands, {}, async command => { visited.push(command); });
  assert.deepEqual(visited, commands);
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
    assert.throws(() => validateToolchainManifest(invalid), /reference fixture/);
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

// 聚焦验收仅要求本语言工具，但始终运行公共 Host、真实安装和 Review 接入。
test('focused acceptance requires only the selected tools and keeps shared integration', () => {
  const env = { NARRANT_GO_SDK: '/go', NARRANT_GO_TARGET_SDK: '/go-target' };
  for (const platform of ['darwin', 'win32']) {
    const commands = verificationCommands(env, platform, 'go');
    assert.equal(commands.length, 7);
    assert.ok(commands[0].executable.endsWith(platform === 'win32' ? 'go.exe' : 'go'));
    const serialized = JSON.stringify(commands);
    for (const required of ['child_process::', 'native_analyzer::', 'native_languages::', 'application::tests::native::', 'real_native_packs_install_and_analyze_through_production_factory', 'native_helper_graphs', 'go_']) assert.ok(serialized.includes(required), required);
    assert.doesNotMatch(serialized, /lsp::|java\/tests|csharp\/tests|clang\/tests/);
    assert.throws(() => verificationCommands({}, platform, 'go'), /Missing fixed toolchain/);
    for (const id of ['typo']) assert.throws(() => verificationCommands(env, platform, id), /not enabled/);
  }
  const config = { go: { executable: '/go/helper', args: [], trusted_roots: ['/go'], platforms: [{ id: 'go', kind: 'go-sdk', version: '1', root: '/sdk' }] } };
  assert.equal(validateNativeConfiguration(config, 'darwin', 'go'), config);
  assert.throws(() => validateNativeConfiguration({}, 'darwin', 'go'), /go/);
});

// 聚焦验收必须跳过其他语言构建及无关声明工具准备。
test('focused workflow skips unrelated builds and declaration provisioning', () => {
  assert.match(workflowSource, /options: \[all, typescript-javascript, rust, python, java, go, csharp, c-cpp\]/);
  const build = workflowSource.split('\n  build:')[1].split('\n  native-analyzers:')[0];
  assert.match(build, /inputs\.language == 'all'/);
  const native = workflowSource.split('\n  native-analyzers:')[1].split('\n  publish:')[0];
  assert.equal((native.match(/--language "\$\{\{ inputs.language \|\| 'all' \}\}"/g) ?? []).length, 2);
  assert.equal((native.match(/inputs\.language == 'c-cpp'/g) ?? []).length, 1);
  assert.match(native, /node scripts\/verify-macos-runner\.mjs/);
  assert.match(workflowSource.split('\n  publish:')[1], /inputs\.language == 'all'/);
});

/** TS 聚焦门禁必须运行真实安装和 Review，不使用旧服务或空筛选充当验收。 */
test('TypeScript focused acceptance includes native projection, complete old graph assertions and Review', async () => {
  const commands = verificationCommands({}, 'darwin', 'typescript-javascript');
  assert.ok(commands[0].args.includes('--test-concurrency=1'));
  const serialized = JSON.stringify(commands);
  for (const required of ['projection.test.mjs','config.test.mjs','workspace.test.mjs','helper.test.mjs','syntax.test.mjs','lifecycle.test.mjs','output.test.mjs','protocol.test.mjs','native_languages::typescript_graph::','real_installed_typescript_pack_analyzes_without_development_tools','real_typescript_javascript_graphs_flow_through_review_lifecycle_to_dto','typescript_']) assert.ok(serialized.includes(required), required);
  assert.ok(!serialized.includes('lsp::tests::'));
  // 退役后仍校验筛选器对应的真实测试入口，防止旧模块删除后零测试被记为通过。
  const source = async relative => fs.readFile(new URL(`../../${relative}`, import.meta.url), 'utf8');
  const graph = await source('src-tauri/src/structure_review/native_languages/tests/typescript_graph.rs');
  assert.ok([...graph.matchAll(/fn (?:graph_|typescript_)[a-z_]+\(/g)].length >= 26);
  assert.match(graph, /fn typescript_historical_identity_and_source_contract\(/);
  assert.match(await source('src-tauri/src/structure_review/native_languages/mod.rs'), /#\[path = "tests\/typescript_graph\.rs"\]\s*mod typescript_graph;/);
  for (const [file, name] of [
    ['src-tauri/src/language_support/tests/service.rs', 'real_installed_typescript_pack_analyzes_without_development_tools'],
    ['src-tauri/src/structure_review/application/tests/mod.rs', 'real_typescript_javascript_graphs_flow_through_review_lifecycle_to_dto'],
    ['src-tauri/tests/native_helper_graphs.rs', 'typescript_real_helper_graph_and_diff'],
  ]) assert.ok((await source(file)).includes(`fn ${name}(`), name);
  assert.match(workflowSource, /scripts\/tests\/archive-validation\.test\.mjs/);
  assert.doesNotMatch(workflowSource, /setup-typescript-analyzer/);
});

/** Rust 聚焦门禁必须检查真实仍存在的入口，三档批准平台由生产工厂逐侧发现。 */
test('Rust focused acceptance retains live graph, factory, installation and Review selectors', async () => {
  const commands = verificationCommands({ NARRANT_PROJECT_RUST_TOOLCHAIN: 'stable', NARRANT_TEST_CARGO_TOOLCHAINS: '/81:/94:/96', NARRANT_TEST_CARGO_194: '/94' }, 'darwin', 'rust');
  const serialized = JSON.stringify(commands);
  for (const required of ['--locked', 'initial_protocol', 'lifecycle', '--include-ignored', 'native_analyzer::tests::rust_graph::', 'native_analyzer::tests::rust_syntax::', 'native_languages::rust::tests::', 'real_installed_rust_pack_analyzes_with_existing_toolchain', 'real_rust_pack_flows_through_review_lifecycle_to_dto', 'rust_', 'native_helper_graphs']) assert.ok(serialized.includes(required), required);
  assert.doesNotMatch(serialized, /lsp::|java\/tests|csharp\/tests|clang\/tests/);
  const config = { rust: { executable:'/pack/bin/narrant-rust', args:[], trusted_roots:['/pack'], platforms:[] } };
  assert.equal(validateNativeConfiguration(config, 'darwin', 'rust'), config);
  assert.throws(() => validateNativeConfiguration(config, 'darwin', 'go'), /go/);
  const read = file => fs.readFile(new URL(`../../${file}`, import.meta.url), 'utf8');
  for (const [file, name] of [
    ['src-tauri/tests/native_helper_graphs.rs','rust_real_helper_graph_and_diff'],
    ['src-tauri/src/structure_review/application/tests/mod.rs','real_rust_pack_flows_through_review_lifecycle_to_dto'],
    ['src-tauri/src/language_support/tests/service.rs','real_installed_rust_pack_analyzes_with_existing_toolchain'],
  ]) assert.ok((await read(file)).includes(`fn ${name}(`), name);
  const graph = await read('src-tauri/src/structure_review/native_analyzer/tests/rust_graph.rs');
  assert.ok([...graph.matchAll(/fn rust_helper_[a-z_]+\(/g)].length >= 25);
});

/** Python 聚焦门禁绑定存活业务入口，删除旧模块后不能把零测试计为通过。 */
test('Python focused acceptance retains version, graph, installed factory and Review selectors', async () => {
  const commands = verificationCommands({}, 'darwin', 'python');
  const serialized = JSON.stringify(commands);
  for (const name of ['native-helpers/python/Cargo.toml', 'initial_protocol', 'lifecycle', 'native_analyzer::tests::python_graph::', 'native_analyzer::tests::python_native::', 'language_support::python_tests::', 'application::tests::python::', 'native_helper_graphs', 'python_']) assert.ok(serialized.includes(name), name);
  assert.doesNotMatch(serialized, /python_analyzer::|NARRANT_TY_PATH|lsp::/);
  const read = file => fs.readFile(new URL(`../../${file}`, import.meta.url), 'utf8');
  for (const [file, name] of [
    ['src-tauri/tests/native_helper_graphs.rs', 'python_real_helper_graph_and_diff'],
    ['src-tauri/src/structure_review/application/tests/python.rs', 'real_python_pack_flows_through_review_lifecycle_to_dto'],
    ['src-tauri/src/language_support/tests/python.rs', 'real_installed_python_pack_analyzes_without_user_python'],
  ]) assert.ok((await read(file)).includes(`fn ${name}(`), name);
  const graph = await read('src-tauri/src/structure_review/native_analyzer/tests/python_native.rs');
  assert.ok([...graph.matchAll(/fn native_[a-z_]+\(/g)].length >= 22);
});

/** CI 必须用独立 Cargo home 准备项目 rust-src 和离线 metadata，不能依赖宿主已有组件缓存。 */
test('Rust acceptance declares fresh project prerequisites independently from helper build toolchain', async () => {
  const native = workflowSource.split('\n  native-analyzers:')[1].split('\n  publish:')[0];
  assert.match(native, /NARRANT_PROJECT_RUST_TOOLCHAIN: stable/);
  assert.match(native, /NARRANT_RUST_TEST_CARGO_HOME: \$\{\{ runner.temp \}\}\/narrant-native-cargo/);
  assert.match(native, /appendFileSync\(process.env.GITHUB_ENV, 'CARGO_HOME=' \+ process.env.NARRANT_RUST_TEST_CARGO_HOME/);
  assert.ok(native.indexOf('Select isolated Rust project test Cargo home') < native.indexOf('Prepare Rust project sources and historical Cargo test inputs'));
  const prepare = native.split('name: Prepare Rust project sources and historical Cargo test inputs')[1].split('\n      - ')[0];
  assert.match(prepare, /rustup component add rust-src --toolchain stable/);
  assert.match(prepare, /rustup toolchain install 1\.81\.0 1\.94\.0 1\.96\.0 --profile minimal/);
  assert.match(prepare, /node scripts\/prepare-rust-native-tests\.mjs --prepare/);
  assert.ok(native.indexOf('prepare-rust-native-tests.mjs --prepare') < native.indexOf('node scripts/verify-native-analyzers.mjs'));
  const commands = verificationCommands({ NARRANT_PROJECT_RUST_TOOLCHAIN: 'stable', NARRANT_TEST_CARGO_TOOLCHAINS: '/81:/94:/96', NARRANT_TEST_CARGO_194: '/94' }, 'darwin', 'rust');
  assert.deepEqual(commands[0].args.slice(0, 6), ['scripts/cargo-agent.sh', '+1.98.0', 'test', '--manifest-path', 'native-helpers/rust/Cargo.toml', '--config']);
  assert.match(commands[0].args[6], /target\.'cfg\(all\(\)\)'\.runner=.*run-rust-helper-test\.mjs/);
  assert.deepEqual(commands[0].args.slice(7), ['--locked', '--no-fail-fast', '--lib', '--test', 'initial_protocol', '--test', 'lifecycle', '--', '--include-ignored', '--test-threads=1']);
  assert.throws(() => verificationCommands({}, 'darwin', 'rust'), /NARRANT_PROJECT_RUST_TOOLCHAIN/);
  const { prepareRustNativeTests } = await import('../prepare-rust-native-tests.mjs');
  await assert.rejects(prepareRustNativeTests({}), /explicit.*CARGO_HOME/);
  await assert.rejects(prepareRustNativeTests({ NARRANT_PROJECT_RUST_TOOLCHAIN: 'stable', CARGO_HOME: '/cargo', NARRANT_TEST_CARGO_TOOLCHAINS: '/only-one' }), /exactly/);
});

/** Cargo 的锁定 Git checkout 也读取全局配置，两个 Windows 构建入口都先启用长路径。 */
test('Windows pack builds enable Git long paths before locked Cargo source checkout', async () => {
  for (const job of ['build', 'native-analyzers']) {
    const source = workflowSource.split(`\n  ${job}:`)[1].split(/\n {2}\S/)[0];
    const setup = source.split('name: Enable Windows Git checkout long paths')[1]?.split('\n      - ')[0];
    assert.ok(setup, `${job} must prepare long paths`);
    assert.match(setup, /if: runner\.os == 'Windows'/);
    assert.match(setup, /run: git config --global core\.longpaths true/);
    assert.ok(source.indexOf('Enable Windows Git checkout long paths') < source.indexOf('Prepare pinned Rust helper build toolchain'));
  }
  const root = await fs.mkdtemp(path.join(os.tmpdir(), 'narrant-git-longpaths-引用-'));
  try {
    // 只给 Git 子进程指定私有配置文件，不修改用户 HOME、全局配置或其他测试环境。
    const environment = { ...process.env, GIT_CONFIG_GLOBAL: path.join(root, 'global.gitconfig'), GIT_CONFIG_NOSYSTEM: '1' };
    execFileSync('git', ['config', '--global', 'core.longpaths', 'true'], { env: environment });
    assert.equal(execFileSync('git', ['config', '--global', '--bool', 'core.longpaths'], { encoding: 'utf8', env: environment }).trim(), 'true');
    // NUL 分隔保留真实来源路径，避免解析 Windows 反斜杠和 Unicode 的人读引号转义。
    const origin = execFileSync('git', ['config', '--global', '--null', '--show-origin', '--bool', 'core.longpaths'], { encoding: 'utf8', env: environment });
    assert.deepEqual(origin.split('\0'), [`file:${environment.GIT_CONFIG_GLOBAL}`, 'true', '']);
  } finally { await fs.rm(root, { recursive: true, force: true }); }
});
