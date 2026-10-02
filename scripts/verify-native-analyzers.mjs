import fs from 'node:fs/promises';
import path from 'node:path';
import { spawn } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import { nativeTargets } from './provision-native-toolchains.mjs';

const repository = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');

/** 所有原生语言必须存在；不允许缺工具时让集成测试跳过后宣称通过。 */
export function validateNativeConfiguration(config, platform = process.platform) {
  const paths = platform === 'win32' ? path.win32 : path.posix;
  for (const id of ['go', 'java', 'csharp', 'clang']) {
    const item = config[id];
    if (!item || !paths.isAbsolute(item.executable ?? '') || !Array.isArray(item.args) || !item.args.every(arg => typeof arg === 'string') || !Array.isArray(item.trusted_roots) || !item.trusted_roots.length || !item.trusted_roots.every(root => paths.isAbsolute(root)) || !Array.isArray(item.platforms) || !item.platforms.length) throw new Error(`Missing or incomplete native helper configuration: ${id}`);
    for (const sdk of item.platforms) if (!paths.isAbsolute(sdk.root ?? '') || !sdk.fingerprint || !sdk.id || !sdk.kind || !sdk.version) throw new Error(`Incomplete platform catalog: ${id}`);
  }
  return config;
}

/** 明确列出串行验收命令；所有 Cargo 入口统一经过仓库锁。 */
export function verificationCommands(environment, platform = process.platform) {
  const suffix = platform === 'win32' ? '.exe' : '';
  const python = environment.NARRANT_PYTHON ?? (platform === 'win32' ? 'python' : 'python3');
  for (const name of ['NARRANT_GO_SDK', 'NARRANT_DOTNET_SDK', 'NARRANT_JAVA_RUNTIME', 'NARRANT_JAVA_TARGET_JDK', 'NARRANT_LLVM_SDK']) if (!environment[name]) throw new Error(`Missing fixed toolchain: ${name}`);
  const cargo = ['scripts/cargo-agent.sh', 'test', '--manifest-path', 'src-tauri/Cargo.toml'];
  return [
    { executable: path.join(environment.NARRANT_GO_SDK, `bin/go${suffix}`), args: ['test', './...'], cwd: 'native-helpers/go', environment: { GOTOOLCHAIN: 'local', GOWORK: 'off', GOPROXY: 'off', GOSUMDB: 'off' } },
    { executable: python, args: ['-m', 'unittest', 'discover', '-s', 'native-helpers/java/tests', '-v'], environment: { NARRANT_JAVA_TARGET: environment.NARRANT_JAVA_TARGET_JDK, NARRANT_JAVA_LIB: path.join(repository, '.tools/java-native/lib') } },
    { executable: python, args: ['-m', 'unittest', 'discover', '-s', 'native-helpers/csharp/tests', '-v'], environment: { NARRANT_TEST_DOTNET: path.join(environment.NARRANT_DOTNET_SDK, `dotnet${suffix}`), NARRANT_TEST_REFS: path.join(environment.NARRANT_DOTNET_SDK, 'packs/Microsoft.NETCore.App.Ref/10.0.0/ref/net10.0') } },
    { executable: python, args: ['-m', 'unittest', 'discover', '-s', 'native-helpers/clang/tests', '-v'], environment: { NARRANT_LLVM_ROOT: environment.NARRANT_LLVM_SDK, NARRANT_CLANG_HELPER: path.join(repository, `native-helpers/clang/build/narrant-clang${suffix}`) } },
    { executable: 'bash', args: [...cargo, '--lib', 'native_analyzer::', '--', '--test-threads=4'] },
    { executable: 'bash', args: [...cargo, '--lib', 'native_languages::', '--', '--test-threads=4'] },
    { executable: 'bash', args: [...cargo, '--lib', 'real_native_packs_install_and_analyze_through_production_factory', '--', '--ignored', '--nocapture'] },
    { executable: 'bash', args: [...cargo, '--test', 'native_helper_graphs', '--', '--ignored', '--nocapture', '--test-threads=1'] },
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

/** 先构建固定本地支持包，再以真实 helper 和公共 Rust 入口完成五种语言模式验收。 */
export async function verifyNativeAnalyzers(artifacts, build = false) {
  const target = nativeTargets[`${process.platform}-${process.arch}`];
  if (!target) throw new Error('Native validation requires macOS ARM64 or Windows x64');
  artifacts = path.resolve(artifacts);
  const environment = { ...process.env, NARRANT_TEST_NODE: process.execPath, NARRANT_NATIVE_PACKS: artifacts, NARRANT_NATIVE_HELPERS_CONFIG: path.join(artifacts, 'native-helpers-config.json'), ...(process.platform === 'win32' ? { NARRANT_WINDOWS_TEST_MANIFEST: '1' } : {}) };
  const commands = verificationCommands(environment);
  if (build) {
    await execute({ executable: environment.NARRANT_PYTHON ?? (process.platform === 'win32' ? 'python' : 'python3'), args: ['native-helpers/java/tools/fetch-deps.py'] }, environment);
    for (const id of ['go', 'java', 'csharp', 'c-cpp']) await execute({ executable: process.execPath, args: ['scripts/build-language-support-packs.mjs', '--id', id, '--target', target, '--output', artifacts] }, environment);
  }
  validateNativeConfiguration(JSON.parse(await fs.readFile(environment.NARRANT_NATIVE_HELPERS_CONFIG, 'utf8')));
  for (const command of commands) await execute(command, environment);
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) await verifyNativeAnalyzers(process.argv[2] ?? 'native-language-support-packs', process.argv.includes('--build'));
