import { migratedNativePacks, selectedNativePacks } from './native-migration.mjs';
import fs from 'node:fs/promises';
import path from 'node:path';
import { spawn } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import { prepareRustNativeTests } from './prepare-rust-native-tests.mjs';
import { nativeTargets } from './provision-native-toolchains.mjs';

const repository = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');

/** 所有原生语言必须存在；不允许缺工具时让集成测试跳过后宣称通过。 */
export function validateNativeConfiguration(config, platform = process.platform, language = 'all') {
  const ids = selectedNativePacks(language);
  const paths = platform === 'win32' ? path.win32 : path.posix;
  for (const id of ids.map(id => migratedNativePacks[id].helper)) {
    const item = config[id];
    if (!item || !paths.isAbsolute(item.executable ?? '') || !Array.isArray(item.args) || !item.args.every(arg => typeof arg === 'string') || !Array.isArray(item.trusted_roots) || !item.trusted_roots.length || !item.trusted_roots.every(root => paths.isAbsolute(root)) || !Array.isArray(item.platforms) || (id !== 'rust' && !item.platforms.length)) throw new Error(`Missing or incomplete native helper configuration: ${id}`);
    for (const sdk of item.platforms) if (!paths.isAbsolute(sdk.root ?? '') || !sdk.id || !sdk.kind || !sdk.version) throw new Error(`Incomplete platform catalog: ${id}`);
  }
  // 完整验收使用三个明确样本验证目标隔离和动态发现，不构成产品支持列表。
  if (ids.includes('csharp')) {
    const references = config.csharp.platforms.filter(platform => platform.kind === 'dotnet-reference-assemblies');
    for (const [tfm, version] of [['net8.0', '8.0.0'], ['net9.0', '9.0.20'], ['net10.0', '10.0.0']]) {
      if (references.filter(platform => platform.version === version && platform.metadata?.tfm === tfm && platform.metadata?.reference_pack === 'Microsoft.NETCore.App.Ref' && platform.metadata?.reference_pack_version === version).length !== 1) throw new Error(`Missing fixed C# target reference: ${tfm}`);
    }
    if (new Set(references.map(platform => paths.normalize(platform.root))).size !== references.length) throw new Error('C# target references must have distinct roots');
  }
  return config;
}

/** 明确列出串行验收命令；所有 Cargo 入口统一经过仓库锁。 */
export function verificationCommands(environment, platform = process.platform, language = 'all') {
  const ids = selectedNativePacks(language);
  const required = { python: [], rust: ['PROJECT_RUST_TOOLCHAIN', 'TEST_CARGO_TOOLCHAINS', 'TEST_CARGO_194'], 'typescript-javascript': [], go: ['GO_SDK', 'GO_TARGET_SDK'], java: ['JAVA_RUNTIME', 'JAVA_TARGET_JDK', 'JAVA_TARGET_8'], csharp: ['DOTNET_SDK', 'DOTNET_REF8_ROOT', 'DOTNET_REF9_ROOT'], 'c-cpp': ['LLVM_SDK'] };
  const suffix = platform === 'win32' ? '.exe' : '';
  const python = environment.NARRANT_PYTHON ?? (platform === 'win32' ? 'python' : 'python3');
  for (const name of ids.flatMap(id => required[id].map(name => `NARRANT_${name}`))) if (!environment[name]) throw new Error(`Missing fixed toolchain: ${name}`);
  const cargo = ['scripts/cargo-agent.sh', 'test', '--manifest-path', 'src-tauri/Cargo.toml'];
  const rustTestRunner = `target.'cfg(all())'.runner=${JSON.stringify([process.execPath, path.join(repository, 'scripts/run-rust-helper-test.mjs')])}`;
  return [
    ...(ids.includes('go') ? [{ executable: path.join(environment.NARRANT_GO_SDK, `bin/go${suffix}`), args: ['test', './...'], cwd: 'native-helpers/go', environment: { GOROOT: environment.NARRANT_GO_SDK, GOTOOLCHAIN: 'local', GOWORK: 'off', GOPROXY: 'off', GOSUMDB: 'off', NARRANT_GO_TEST_SDKS: [environment.NARRANT_GO_TARGET_SDK, environment.NARRANT_GO_SDK].join(platform === 'win32' ? ';' : ':') } }] : []),
    ...(ids.includes('java') ? [{ executable: python, args: ['-m', 'unittest', 'discover', '-s', 'native-helpers/java/tests', '-v'], environment: { NARRANT_JAVA_TARGET: environment.NARRANT_JAVA_TARGET_JDK, NARRANT_JAVA_LIB: path.join(repository, '.tools/java-native/lib') } }] : []),
    ...(ids.includes('csharp') ? [{ executable: python, args: ['-m', 'unittest', 'discover', '-s', 'native-helpers/csharp/tests', '-v'], environment: { NARRANT_TEST_DOTNET: path.join(environment.NARRANT_DOTNET_SDK, `dotnet${suffix}`), NARRANT_TEST_REFS: path.join(environment.NARRANT_DOTNET_SDK, 'packs/Microsoft.NETCore.App.Ref/10.0.0/ref/net10.0'), NARRANT_TEST_REFS8: path.join(environment.NARRANT_DOTNET_REF8_ROOT, 'ref/net8.0'), NARRANT_TEST_REFS9: path.join(environment.NARRANT_DOTNET_REF9_ROOT, 'ref/net9.0'), NARRANT_TEST_REFS9_VERSION: '9.0.20' } }] : []),
    ...(ids.includes('c-cpp') ? [{ executable: python, args: ['-m', 'unittest', 'discover', '-s', 'native-helpers/clang/tests', '-v'], environment: { NARRANT_LLVM_ROOT: environment.NARRANT_LLVM_SDK, NARRANT_CLANG_HELPER: path.join(repository, `native-helpers/clang/build/narrant-clang${suffix}`) } }] : []),
    // 真实协议子进程与 CPU 生命周期夹具按文件串行，避免准备测试本身耗尽退出观察期限。
    ...(ids.includes('typescript-javascript') ? [
      { executable: process.execPath, args: ['--test', '--test-concurrency=1', 'native-helpers/typescript/tests/projection.test.mjs', 'native-helpers/typescript/tests/occurrences.test.mjs', 'native-helpers/typescript/tests/config.test.mjs', 'native-helpers/typescript/tests/workspace.test.mjs', 'native-helpers/typescript/tests/helper.test.mjs', 'native-helpers/typescript/tests/syntax.test.mjs', 'native-helpers/typescript/tests/lifecycle.test.mjs', 'native-helpers/typescript/tests/output.test.mjs', 'native-helpers/typescript/tests/protocol.test.mjs'] },
      { executable: 'bash', args: [...cargo, '--lib', 'native_languages::typescript_graph::', '--', '--ignored', '--test-threads=4'] },
      { executable: 'bash', args: [...cargo, '--lib', 'real_installed_typescript_pack_analyzes_without_development_tools', '--', '--ignored'] },
      { executable: 'bash', args: [...cargo, '--lib', 'real_typescript_javascript_graphs_flow_through_review_lifecycle_to_dto', '--', '--ignored'] },
    ] : []),
    ...(ids.includes('rust') ? [
      { executable: 'bash', args: ['scripts/cargo-agent.sh', '+1.98.0', 'test', '--manifest-path', 'native-helpers/rust/Cargo.toml', '--config', rustTestRunner, '--locked', '--lib', '--test', 'initial_protocol', '--test', 'lifecycle', '--', '--include-ignored', '--test-threads=1'] },
      { executable: 'bash', args: [...cargo, '--lib', 'native_analyzer::tests::rust_graph::', '--', '--ignored', '--test-threads=1'] },
      { executable: 'bash', args: [...cargo, '--lib', 'native_analyzer::tests::rust_syntax::', '--', '--ignored', '--test-threads=1'] },
      { executable: 'bash', args: [...cargo, '--lib', 'native_languages::rust::tests::', '--', '--ignored', '--test-threads=1'] },
      { executable: 'bash', args: [...cargo, '--lib', 'real_installed_rust_pack_analyzes_with_existing_toolchain', '--', '--ignored'] },
      { executable: 'bash', args: [...cargo, '--lib', 'real_rust_pack_flows_through_review_lifecycle_to_dto', '--', '--ignored'] },
    ] : []),
    ...(ids.includes('python') ? [
      { executable: 'bash', args: ['scripts/cargo-agent.sh', '+1.98.0', 'test', '--manifest-path', 'native-helpers/python/Cargo.toml', '--locked', '--lib', '--test', 'initial_protocol', '--test', 'lifecycle', '--', '--test-threads=1'] },
      { executable: 'bash', args: [...cargo, '--lib', 'native_analyzer::tests::python_graph::', '--', '--ignored', '--test-threads=1'] },
      { executable: 'bash', args: [...cargo, '--lib', 'native_analyzer::tests::python_native::', '--', '--ignored', '--test-threads=1'] },
      { executable: 'bash', args: [...cargo, '--lib', 'language_support::python_tests::', '--', '--include-ignored', '--test-threads=1'] },
      { executable: 'bash', args: [...cargo, '--lib', 'application::tests::python::', '--', '--include-ignored', '--test-threads=1'] },
    ] : []),
    // 所有平台复用同一进程所有者矩阵；Windows 同组还包含准确句柄与 Job 回归。
    { executable: 'bash', args: [...cargo, '--lib', 'child_process::', '--', '--nocapture', '--test-threads=1'] },
    { executable: 'bash', args: [...cargo, '--lib', 'native_analyzer::', '--', '--test-threads=4'] },
    { executable: 'bash', args: [...cargo, '--lib', 'structure_review::application::tests::native::', '--', '--test-threads=4'] },
    { executable: 'bash', args: [...cargo, '--lib', 'native_languages::', '--', '--test-threads=4'] },
    { executable: 'bash', args: [...cargo, '--lib', 'real_native_packs_install_and_analyze_through_production_factory', '--', '--ignored', '--nocapture'] },
    { executable: 'bash', args: [...cargo, '--test', 'native_helper_graphs', ...(language === 'all' ? [] : [`${migratedNativePacks[language].helper}_`]), '--', '--ignored', '--nocapture', '--test-threads=1'] },
  ];
}

/** 原样传递参数而不经过 shell，保留 Windows 路径中的空格与分号。 */
async function execute(command, environment) {
  console.log(`Native validation: ${command.executable} ${command.args.join(' ')}`);
  const child = spawn(command.executable, command.args, { cwd: path.resolve(repository, command.cwd ?? '.'), stdio: 'inherit', env: { ...environment, ...command.environment } });
  await new Promise((resolve, reject) => {
    child.once('error', reject);
    child.once('close', code => code === 0 ? resolve() : reject(new Error(`Native validation failed (${code}): ${command.executable}`)));
  });
}

/** 串行完成全部独立验收组，再汇总命令身份及原始失败；先决构建和配置不走此入口。 */
export async function runVerificationCommands(commands, environment, run = execute) {
  const failures = [];
  for (const command of commands) {
    try {
      await run(command, environment);
    } catch (cause) {
      const failure = new Error(`Native validation group failed: ${command.executable} ${command.args.join(' ')}`, { cause });
      failures.push(failure);
      console.error(failure);
    }
  }
  if (failures.length) throw new AggregateError(failures, `${failures.length} native validation group(s) failed`);
}

/** 先构建固定本地支持包，再以真实 helper 和公共 Rust 入口完成七套语言 helper 验收。 */
export async function verifyNativeAnalyzers(artifacts, build = false, language = 'all') {
  const ids = selectedNativePacks(language);
  const target = nativeTargets[`${process.platform}-${process.arch}`];
  if (!target) throw new Error('Native validation requires macOS ARM64 or Windows x64');
  artifacts = path.resolve(artifacts);
  const environment = { ...process.env, NARRANT_TEST_NODE: process.execPath, NARRANT_NATIVE_LANGUAGE: language, NARRANT_NATIVE_PACKS: artifacts, NARRANT_NATIVE_HELPERS_CONFIG: path.join(artifacts, 'native-helpers-config.json'), ...(process.platform === 'win32' ? { NARRANT_WINDOWS_TEST_MANIFEST: '1' } : {}) };
  if (ids.includes('python')) {
    const catalog = JSON.parse(await fs.readFile(path.join(repository, 'resources/language-support-packs.json'), 'utf8'));
    environment.NARRANT_PYTHON_PACK = path.join(artifacts, 'smoke-installed/python', catalog.packs.python.version, target);
    environment.NARRANT_PYTHON_HELPER = path.join(environment.NARRANT_PYTHON_PACK, `bin/narrant-python${process.platform === 'win32' ? '.exe' : ''}`);
    environment.NARRANT_LANGUAGE_PACK_FIXTURE = artifacts;
  }
  if (ids.includes('rust')) {
    const catalog = JSON.parse(await fs.readFile(path.join(repository, 'resources/language-support-packs.json'), 'utf8'));
    environment.NARRANT_RUST_PACK = path.join(artifacts, 'smoke-installed/rust', catalog.packs.rust.version, target);
    environment.NARRANT_RUST_HELPER = path.join(environment.NARRANT_RUST_PACK, `bin/narrant-rust${process.platform === 'win32' ? '.exe' : ''}`);
    environment.NARRANT_LANGUAGE_PACK_FIXTURE = artifacts;
  }
  if (ids.includes('typescript-javascript')) {
    const catalog = JSON.parse(await fs.readFile(path.join(repository, 'resources/language-support-packs.json'), 'utf8'));
    environment.NARRANT_TYPESCRIPT_PACK = path.join(artifacts, 'smoke-installed/typescript-javascript', catalog.packs['typescript-javascript'].version, target);
    environment.NARRANT_LANGUAGE_PACK_FIXTURE = artifacts;
  }
  if (ids.includes('rust')) Object.assign(environment, await prepareRustNativeTests(environment));
  const commands = verificationCommands(environment, process.platform, language);
  if (build) {
    if (ids.includes('java')) await execute({ executable: environment.NARRANT_PYTHON ?? (process.platform === 'win32' ? 'python' : 'python3'), args: ['native-helpers/java/tools/fetch-deps.py'] }, environment);
    for (const id of ids) await execute({ executable: process.execPath, args: ['scripts/build-language-support-packs.mjs', '--id', id, '--target', target, '--output', artifacts] }, environment);
  }
  validateNativeConfiguration(JSON.parse(await fs.readFile(environment.NARRANT_NATIVE_HELPERS_CONFIG, 'utf8')), process.platform, language);
  await runVerificationCommands(commands, environment);
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const language = process.argv.includes('--language') ? process.argv[process.argv.indexOf('--language') + 1] : 'all';
  if (!language) throw new Error('--language requires a completed language or all');
  await verifyNativeAnalyzers(process.argv[2] ?? 'native-language-support-packs', process.argv.includes('--build'), language);
}
