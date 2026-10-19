import { migratedNativePacks } from './native-migration.mjs';
import fs from 'node:fs/promises';
import path from 'node:path';
import os from 'node:os';
import { spawn, execFileSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';

const repository = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../..');
const nativeIds = Object.keys(migratedNativePacks);

/** 只复制自己构建的工具和批准的 SDK 声明，展开 SDK 内部链接，拒绝外向链接。 */
export async function copyToolTree(source, destination) {
  const root = await fs.realpath(source);
  /** 对每个实际文件重新验证来源，归档最终只含普通文件。 */
  async function copy(current, target, ancestors = new Set()) {
    const resolved = await fs.realpath(current);
    if (resolved !== root && !resolved.startsWith(`${root}${path.sep}`)) throw new Error(`Tool symlink escapes approved root: ${current}`);
    const info = await fs.stat(resolved);
    if (info.isDirectory()) {
      if (ancestors.has(resolved)) throw new Error('Tool directory symlink cycle');
      const next = new Set(ancestors).add(resolved);
      await fs.mkdir(target, { recursive: true });
      for (const name of (await fs.readdir(resolved)).sort()) await copy(path.join(resolved, name), path.join(target, name), next);
    } else if (info.isFile()) {
      await fs.mkdir(path.dirname(target), { recursive: true });
      await fs.copyFile(resolved, target);
      await fs.chmod(target, info.mode & 0o111 ? 0o755 : 0o644);
    } else throw new Error('Tool tree contains a special file');
  }
  await copy(root, destination);
}

/** 工具候选只来自明确构建输入或约定安装位置，不扫描被分析项目。 */
async function findDirectory(candidates, label) {
  for (const candidate of candidates.filter(Boolean)) {
    try { if ((await fs.stat(candidate)).isDirectory()) return await fs.realpath(candidate); } catch (error) { if (error.code !== 'ENOENT') throw error; }
  }
  throw new Error(`${label} not installed; provide an isolated tool directory for local pack construction`);
}

/** 为真实 helper 验收读取私有测试 SDK 的引用目录；目标声明不再复制进支持包。 */
export async function csharpFixturePlatforms(sdk) {
  const base = path.join(sdk, 'packs/Microsoft.NETCore.App.Ref');
  const targets = new Map();
  for (const version of (await fs.readdir(base)).sort((a, b) => a.localeCompare(b, 'en', { numeric: true }))) {
    if (!/^[1-9]\d*\.0\.(0|[1-9]\d*)$/.test(version)) continue;
    const tfm = `net${version.split('.')[0]}.0`;
    const root = path.join(base, version, 'ref', tfm);
    try { await fs.access(path.join(root, 'System.Runtime.dll')); } catch (error) { if (error.code === 'ENOENT') continue; throw error; }
    targets.set(tfm, { id: `${tfm}-reference-assemblies`, kind: 'dotnet-reference-assemblies', version, root,
      metadata: { tfm, reference_pack: 'Microsoft.NETCore.App.Ref', reference_pack_version: version } });
  }
  return [...targets.values()];
}

/** 验收只描述明确准备的真实 JDK；产品的安装发现由 Rust Host 负责。 */
export async function javaFixturePlatforms(roots) {
  const platforms = [];
  for (const root of [...new Set(roots.filter(Boolean))]) {
    const release = await fs.readFile(path.join(root, 'release'), 'utf8');
    const version = release.match(/^JAVA_VERSION="([^"]+)"/m)?.[1];
    const major = version?.startsWith('1.') ? version.split('.')[1] : version?.split('.')[0];
    if (!/^\d+$/.test(major ?? '')) throw new Error('Java fixture has no stable version');
    platforms.push({ id: `java${major}`, kind: 'java-jdk', version, root, metadata: { release: major } });
  }
  return platforms;
}

/** 新旧 Go SDK 作为真实版本样本传入，不随支持包复制标准源码。 */
export async function goFixturePlatforms(roots) {
  const platforms = [];
  for (const root of [...new Set(roots.filter(Boolean))]) {
    const version = (await fs.readFile(path.join(root, 'VERSION'), 'utf8')).split(/\r?\n/)[0].match(/^go(\d+\.\d+(?:\.\d+)?)$/)?.[1];
    if (!version) throw new Error('Go fixture has no stable version');
    platforms.push({ id: `go-sdk-${version}`, kind: 'go-sdk', version, root,
      metadata: { goos: process.platform === 'win32' ? 'windows' : 'darwin', goarch: process.arch === 'x64' ? 'amd64' : 'arm64', ...(process.arch === 'x64' ? { goamd64: 'v1' } : {}) } });
  }
  return platforms;
}

/** 以空业务环境启动自己的 helper，验证 ready 身份以及协议错误的受控终结。 */
export async function smokeNativeHelper({ executable, args = [], id, projection, languages, profile, environment = {} }) {
  const root = await fs.mkdtemp(path.join(os.tmpdir(), 'narrant-native-pack-smoke-'));
  let primaryError;
  try {
    await new Promise((resolve, reject) => {
      const child = spawn(executable, args, { cwd: root, env: { ...(process.platform === 'win32' ? { SystemRoot: process.env.SystemRoot } : {}), TMPDIR: root, TMP: root, TEMP: root, DOTNET_EnableDiagnostics: '0', ...environment }, stdio: ['pipe', 'pipe', 'pipe'] });
      let stdout = '', stderr = '', ready = false;
      let failure, closeTimer;
      /** 保存首个失败，等待异步 close 后才允许调用方清理 cwd。 */
      function stop(error) {
        if (failure) return;
        failure = error;
        clearTimeout(timer);
        closeTimer = setTimeout(() => {
          failure.message += '; native helper close timeout';
          child.stdin.destroy(); child.stdout.destroy(); child.stderr.destroy();
          child.unref();
          reject(failure);
        }, 3_000);
        try { child.kill('SIGKILL'); } catch { /* 原始错误仍由 close 或有界兜底返回。 */ }
      }
      const timer = setTimeout(() => stop(new Error('Native helper startup smoke timeout')), 30_000);
      child.on('error', stop);
      child.stdin.on('error', stop);
      child.stdout.on('data', chunk => {
        stdout += chunk;
        if (failure) return;
        if (stdout.length > 262144) { stop(new Error('Native helper smoke output exceeds limit')); return; }
        if (!ready && stdout.includes('\n')) {
          try {
            const message = JSON.parse(stdout.slice(0, stdout.indexOf('\n')));
            if (message.protocol !== 'narrant.native-analyzer/1' || message.type !== 'ready' || message.helper_id !== id || message.helper_version !== '0.1.0' || message.projection_revision !== projection || JSON.stringify(message.languages) !== JSON.stringify(languages) || !message.profiles.includes(profile)) throw new Error('Native helper ready identity mismatch');
            ready = true;
            // EOF 在请求前只允许终止，不应遗留自己的输入线程或子进程。
            child.stdin.end();
          } catch (error) { stop(error); }
        }
      });
      child.stderr.on('data', chunk => { stderr = (stderr + chunk).slice(-262144); });
      child.on('close', (code, signal) => {
        clearTimeout(timer);
        clearTimeout(closeTimer);
        if (failure) reject(failure);
        else if (!ready || signal || ![0, 1, 2].includes(code)) reject(new Error(`Native helper smoke failed (${code}/${signal}): ${stderr}`));
        else resolve();
      });
    });
  } catch (error) { primaryError = error; }
  try { await fs.rm(root, { recursive: true, force: true }); }
  catch (error) {
    if (primaryError) primaryError.message += `; scratch cleanup failed: ${error.message}`;
    else primaryError = error;
  }
  if (primaryError) throw primaryError;
}

/** 从实际目标的锁定依赖闭包收集许可；Rust 正常构建入口使用，后续 Python 可复用此归档步骤。 */
export async function packageCargoLicenses(source, target, payload, label) {
  // 构建自己的工具时允许获取锁定 metadata 所需的开发 crate；release 缓存不保证含它们。
  // 分发仍只收集下面的目标生产闭包，此步骤不参与用户项目的离线分析。
  const metadata = JSON.parse(execFileSync('bash', [path.join(repository, 'scripts/cargo-agent.sh'), 'metadata', '--locked', '--filter-platform', target, '--format-version', '1'], { cwd: source, encoding: 'utf8', maxBuffer: 16 * 1024 * 1024 }));
  const nodes = new Map(metadata.resolve.nodes.map(node => [node.id, node]));
  const included = new Set();
  const pending = [metadata.resolve.root];
  while (pending.length) {
    const id = pending.pop();
    if (included.has(id)) continue;
    included.add(id);
    for (const dependency of nodes.get(id).deps) {
      if (dependency.dep_kinds.some(kind => kind.kind !== 'dev')) pending.push(dependency.pkg);
    }
  }
  const fallbackRoot = path.join(source, 'licenses');
  let fallbacks = {};
  try { fallbacks = JSON.parse(await fs.readFile(path.join(fallbackRoot, 'fallbacks.json'), 'utf8')); }
  catch (error) { if (error.code !== 'ENOENT') throw error; }
  const notices = [];
  for (const pkg of metadata.packages.filter(pkg => included.has(pkg.id))) {
    const destination = path.join(payload, 'licenses', `${pkg.name}-${pkg.version}`);
    await fs.mkdir(destination, { recursive: true });
    const files = new Set();
    let directory = path.dirname(pkg.manifest_path);
    for (let level = 0; level < 5; level++) {
      const names = await fs.readdir(directory);
      for (const name of names.filter(name => /^(LICENSE|COPYING|NOTICE)([.-]|$)/i.test(name))) {
        if ((await fs.stat(path.join(directory, name))).isFile()) {
          await fs.copyFile(path.join(directory, name), path.join(destination, name));
          files.add(name);
        }
      }
      if (files.size || path.dirname(directory) === directory) break;
      directory = path.dirname(directory);
    }
    if (pkg.license_file) {
      const name = path.basename(pkg.license_file);
      await fs.copyFile(path.resolve(path.dirname(pkg.manifest_path), pkg.license_file), path.join(destination, name));
      files.add(name);
    }
    if (!files.size && pkg.license) {
      // 缺文本的精确锁定版本只能使用已保存一手出处的原许可，不猜测作者或许可模板。
      const names = fallbacks[`${pkg.name}@${pkg.version}`];
      if (!Array.isArray(names) || !names.length) throw new Error(`Locked crate has no distributable license text: ${pkg.name}`);
      for (const name of names) {
        await fs.copyFile(path.join(fallbackRoot, name), path.join(destination, name));
        files.add(name);
      }
    }
    notices.push({ name: pkg.name, version: pkg.version, license: pkg.license, source: pkg.source, repository: pkg.repository, authors: pkg.authors, files: [...files].sort() });
  }
  if (Object.keys(fallbacks).length) await fs.copyFile(path.join(fallbackRoot, 'sources.json'), path.join(payload, `licenses/${label}-license-sources.json`));
  await fs.writeFile(path.join(payload, `licenses/${label}-dependencies.json`), JSON.stringify(notices, null, 2));
}

/** 从固定语言源码和私有 SDK 组装本机开发包；不下载项目依赖、不调用项目构建。 */
export async function buildNativePack({ id, target, output, definition, packageDirectory }) {
  if (!nativeIds.includes(id)) throw new Error('Unknown native pack');
  const expectedTarget = ({ 'darwin-arm64': 'aarch64-apple-darwin', 'win32-x64': 'x86_64-pc-windows-msvc' })[`${process.platform}-${process.arch}`];
  if (target !== expectedTarget) throw new Error('Native pack must be built and smoke-tested on its target OS and CPU');
  output = path.resolve(output);
  await fs.mkdir(output, { recursive: true });
  const temporary = await fs.mkdtemp(path.join(output, '.native-build-'));
  const payload = path.join(temporary, 'payload');
  const windows = process.platform === 'win32';
  const suffix = windows ? '.exe' : '';
  const platforms = [];
  const externalPlatforms = [];
  let executable, args = [];
  try {
    for (const part of ['bin', 'runtime', 'licenses', 'control']) await fs.mkdir(path.join(payload, part), { recursive: true });
    if (id === 'rust' || id === 'python') {
      const { buildMigratingNativeHelper } = await import('./build-migrating-native-helper.mjs');
      await buildMigratingNativeHelper(id, payload);
      executable = path.join(payload, `bin/narrant-${id}${suffix}`);
      await packageCargoLicenses(path.join(repository, `native-helpers/${id}`), target, payload, id === 'rust' ? 'Rust' : 'Python');
      if (id === 'python') {
        const source = path.join(repository, 'native-helpers/python/licenses');
        for (const name of ['ruff-LICENSE', 'typeshed-LICENSE']) await fs.copyFile(path.join(source, name), path.join(payload, 'licenses', name));
        platforms.push({ id: 'python-stdlib', kind: 'python-embedded-stdlib', version: definition.ty, root: 'bin', metadata: { source_revision: definition.sourceCommit } });
      }
    } else if (id === 'go') {
      const sdk = await findDirectory([process.env.NARRANT_GO_SDK, windows ? 'C:\\Program Files\\Go' : '/usr/local/go'], `Go ${definition.go} helper build SDK`);
      if (!(await fs.readFile(path.join(sdk, 'VERSION'), 'utf8')).startsWith(`go${definition.go}\n`)) throw new Error('Go SDK version mismatch');
      executable = path.join(payload, `bin/narrant-go${suffix}`);
      execFileSync(path.join(sdk, `bin/go${suffix}`), ['build', '-trimpath', '-o', executable, '.'], { cwd: path.join(repository, 'native-helpers/go'), stdio: 'inherit', env: { ...process.env, GOROOT: sdk, GOTOOLCHAIN: 'local', GOWORK: 'off', GOPROXY: 'off', GOSUMDB: 'off' } });
      await fs.copyFile(path.join(sdk, 'LICENSE'), path.join(payload, 'licenses/Go-LICENSE'));
      externalPlatforms.push(...await goFixturePlatforms([process.env.NARRANT_GO_TARGET_SDK, sdk]));
    } else if (id === 'csharp') {
      const sdk = await findDirectory([process.env.NARRANT_DOTNET_SDK, '/private/tmp/narrant-dotnet-10.0.100', '/usr/local/share/dotnet', windows && 'C:\\Program Files\\dotnet'], '.NET 10.0.100 SDK');
      const tool = path.join(sdk, `dotnet${suffix}`);
      const buildEnvironment = { ...process.env, DOTNET_ROOT: sdk, DOTNET_CLI_HOME: path.join(temporary, 'dotnet-home'), DOTNET_CLI_TELEMETRY_OPTOUT: '1', DOTNET_SKIP_FIRST_TIME_EXPERIENCE: '1', DOTNET_GENERATE_ASPNET_CERTIFICATE: 'false' };
      const version = execFileSync(tool, ['--version'], { encoding: 'utf8', env: buildEnvironment, cwd: path.join(repository, 'native-helpers/csharp') }).trim();
      if (version !== definition.dotnetSdk) throw new Error('.NET SDK version mismatch');
      const publish = path.join(temporary, 'publish');
      execFileSync(tool, ['publish', path.join(repository, 'native-helpers/csharp/Narrant.CSharp.Helper.csproj'), '-c', 'Release', '--no-self-contained', '-p:UseAppHost=false', '-o', publish], { stdio: 'inherit', env: buildEnvironment });
      // 将实际 SDK 自带 runtime 私有化，避免 self-contained restore 访问 NuGet。
      await copyToolTree(path.join(sdk, `dotnet${suffix}`), path.join(payload, `bin/dotnet${suffix}`));
      await copyToolTree(path.join(sdk, 'host'), path.join(payload, 'bin/host'));
      await copyToolTree(path.join(sdk, 'shared/Microsoft.NETCore.App', definition.dotnetRuntime), path.join(payload, 'bin/shared/Microsoft.NETCore.App', definition.dotnetRuntime));
      await copyToolTree(publish, path.join(payload, 'runtime/helper'));
      externalPlatforms.push(...await csharpFixturePlatforms(sdk));
      for (const name of ['LICENSE.txt', 'ThirdPartyNotices.txt']) await fs.copyFile(path.join(sdk, name), path.join(payload, 'licenses', name));
      executable = path.join(payload, `bin/dotnet${suffix}`);
      args = [path.join(payload, 'runtime/helper/Narrant.CSharp.Helper.dll')];
    } else if (id === 'java') {
      const runtime = await findDirectory([process.env.NARRANT_JAVA_RUNTIME, path.join(repository, '.tools/java-native/jdk-25.0.4.1+1/Contents/Home')], 'Pinned Java helper runtime');
      const runtimeRelease = await fs.readFile(path.join(runtime, 'release'), 'utf8');
      if (!runtimeRelease.includes(`IMPLEMENTOR_VERSION="Temurin-${definition.runtimeJava}"`)) throw new Error('Java helper runtime version mismatch');
      execFileSync(process.env.NARRANT_PYTHON ?? process.env.PYTHON ?? (windows ? 'python' : 'python3'), [path.join(repository, 'native-helpers/java/build.py'), runtime, path.join(repository, '.tools/java-native/lib')], { stdio: 'inherit', env: { ...process.env, NARRANT_JAVA_RUNTIME: runtime } });
      await copyToolTree(runtime, path.join(payload, 'runtime/jdk'));
      await copyToolTree(path.join(repository, 'native-helpers/java/build/classes'), path.join(payload, 'runtime/helper/classes'));
      await copyToolTree(path.join(repository, '.tools/java-native/lib'), path.join(payload, 'runtime/helper/lib'));
      await copyToolTree(path.join(repository, 'native-helpers/java/dependencies.lock.json'), path.join(payload, 'licenses/Java-dependencies.json'));
      await copyToolTree(path.join(runtime, 'NOTICE'), path.join(payload, 'licenses/Java-runtime-NOTICE'));
      executable = path.join(payload, `runtime/jdk/bin/java${suffix}`);
      args = ['-cp', `${path.join(payload, 'runtime/helper/classes')}${path.delimiter}${path.join(payload, 'runtime/helper/lib/*')}`, 'dev.narrant.java.Main'];
      externalPlatforms.push(...await javaFixturePlatforms([process.env.NARRANT_JAVA_TARGET_8, process.env.NARRANT_JAVA_TARGET_JDK, runtime]));
    } else {
      const sdk = await findDirectory([process.env.NARRANT_LLVM_SDK, '/private/tmp/narrant-clang-native/LLVM-21.1.8-macOS-ARM64'], 'LLVM 21.1.8 SDK');
      if (windows) {
        for (const name of ['NARRANT_MSVC_VERSION', 'NARRANT_VISUAL_STUDIO', 'NARRANT_MSVC_REDIST']) if (!process.env[name]) throw new Error(`Missing fixed Windows toolchain input: ${name}`);
        const source = path.join(repository, 'native-helpers/clang');
        const build = path.join(source, 'build');
        const cmake = process.env.NARRANT_CMAKE || 'cmake';
        execFileSync(cmake, ['-S', source, '-B', build, '-G', 'Visual Studio 17 2022', '-A', 'x64', '-T', `v143,version=${process.env.NARRANT_MSVC_VERSION}`, '-DCMAKE_SYSTEM_VERSION=10.0.26100.0', `-DCMAKE_GENERATOR_INSTANCE=${process.env.NARRANT_VISUAL_STUDIO}`, `-DLLVM_DIR=${path.join(sdk, 'lib/cmake/llvm')}`, `-DClang_DIR=${path.join(sdk, 'lib/cmake/clang')}`], { stdio: 'inherit' });
        // 构建机器可按可用内存设置 CMake 并行度，默认仍为两个编译任务。
        execFileSync(cmake, ['--build', build, '--config', 'Release', '--parallel', process.env.CMAKE_BUILD_PARALLEL_LEVEL || '2'], { stdio: 'inherit' });
        // 运行时只从私有 bin 加载官方 LLVM DLL 与已批准的 VC143 CRT。
        for (const directory of [path.join(sdk, 'bin'), process.env.NARRANT_MSVC_REDIST]) {
          for (const name of await fs.readdir(directory)) if (name.toLowerCase().endsWith('.dll')) await copyToolTree(path.join(directory, name), path.join(payload, 'bin', name));
        }
      } else execFileSync('bash', [path.join(repository, 'native-helpers/clang/build.sh'), sdk], { stdio: 'inherit' });
      executable = path.join(payload, `bin/narrant-clang${suffix}`);
      await fs.copyFile(path.join(repository, `native-helpers/clang/build/narrant-clang${suffix}`), executable);
      await fs.chmod(executable, 0o755);
      await copyToolTree(path.join(sdk, 'lib/clang/21'), path.join(payload, 'runtime/platform/clang-resource'));
      // 动态依赖必须由固定构建脚本落到 build/runtime，不能依赖研发机的绝对库路径。
      if (!windows) await copyToolTree(path.join(repository, 'native-helpers/clang/build/runtime/lib'), path.join(payload, 'runtime/lib'));
      await copyToolTree(path.join(repository, 'native-helpers/clang/LICENSE-LLVM.txt'), path.join(payload, 'licenses/LLVM-LICENSE'));
      await copyToolTree(path.join(repository, 'native-helpers/clang/LICENSE-Clang.txt'), path.join(payload, 'licenses/Clang-LICENSE'));
      platforms.push({ id: 'clang-resource-21', kind: 'clang-resource', version: definition.llvm, root: 'runtime/platform/clang-resource', metadata: {} });
    }
    const control = { schema_version: 1, helper_version: definition.helperVersion, platforms };
    if (id === 'java') control.target_sdk = { kind: 'java' };
    if (id === 'go') control.target_sdk = { kind: 'go' };
    if (id === 'csharp') control.target_sdk = { kind: 'dotnet' };
    if (id === 'c-cpp') control.target_sdk = { kind: windows ? 'windows-msvc' : 'macos' };
    await fs.writeFile(path.join(payload, 'control/native.json'), `${JSON.stringify(control, null, 2)}\n`);
    const helper = id === 'c-cpp' ? 'clang' : id;
    await smokeNativeHelper({ executable, args, id: definition.helperId, projection: definition.projectionRevision, languages: id === 'c-cpp' ? ['c', 'cpp'] : [id], profile: definition.profile });
    const destination = await packageDirectory({ id, version: definition.version, target }, payload, output);
    // 解压归档后再次启动，证明运行环境与可执行权限随支持包保存。
    const extracted = path.join(output, 'smoke-installed', id, definition.version, target);
    await fs.rm(extracted, { recursive: true, force: true });
    await fs.mkdir(extracted, { recursive: true });
    execFileSync('tar', ['-xzf', path.join(destination, 'pack.tar.gz'), '-C', extracted]);
    const relocate = value => value.replaceAll(payload, extracted);
    await smokeNativeHelper({ executable: relocate(executable), args: args.map(relocate), id: `narrant-${helper}`, projection: definition.projectionRevision, languages: id === 'c-cpp' ? ['c', 'cpp'] : [id], profile: definition.profile });
    const configPath = path.join(output, 'native-helpers-config.json');
    let config = {};
    try { config = JSON.parse(await fs.readFile(configPath, 'utf8')); } catch (error) { if (error.code !== 'ENOENT') throw error; }
    const sdkPlatforms = [...platforms.map(platform => ({ ...platform, root: path.join(extracted, platform.root) })), ...externalPlatforms];
    if (id === 'c-cpp') {
      if (windows) {
        const sdkRoot = await findDirectory([process.env.NARRANT_WINDOWS_SDK], 'Windows SDK 10.0.26100.0 Include directory');
        const standard = await findDirectory([process.env.NARRANT_MSVC_ROOT && path.join(process.env.NARRANT_MSVC_ROOT, 'include')], 'Exact MSVC standard headers');
        sdkPlatforms.push({ id: 'msvc-standard-headers', kind: 'clang-system-headers', version: path.basename(path.dirname(standard)), root: standard, metadata: {} });
        sdkPlatforms.push({ id: `windows-sdk-${path.basename(sdkRoot)}`, kind: 'clang-target', version: path.basename(sdkRoot), root: sdkRoot, metadata: { target_triple: 'x86_64-pc-windows-msvc', system_include_directories: ['ucrt', 'shared', 'um', 'winrt', 'cppwinrt'], resource_platform_id: 'clang-resource-21', additional_system_platform_ids: ['msvc-standard-headers'] } });
      } else {
        const sdkRoot = await findDirectory([process.env.NARRANT_MACOS_SDK, '/Library/Developer/CommandLineTools/SDKs/MacOSX.sdk', '/Applications/Xcode.app/Contents/Developer/Platforms/MacOSX.platform/Developer/SDKs/MacOSX.sdk'], 'macOS target SDK');
        const settings = JSON.parse(await fs.readFile(path.join(sdkRoot, 'SDKSettings.json'), 'utf8'));
        const version = settings.Version;
        if (typeof version !== 'string' || !/^\d+(\.\d+)+$/.test(version)) throw new Error('macOS SDK has no version metadata');
        const deploymentTarget = settings.DefaultProperties?.MACOSX_DEPLOYMENT_TARGET ?? version;
        sdkPlatforms.push({ id: `macos-${version}`, kind: 'clang-target', version, root: sdkRoot, metadata: { target_triple: `arm64-apple-macosx${deploymentTarget}`, system_include_directories: ['usr/include/c++/v1', 'usr/include'], resource_platform_id: 'clang-resource-21' } });
      }
    }
    config[helper] = { executable: relocate(executable), args: args.map(relocate), environment: id === 'csharp' ? { DOTNET_ROOT: path.join(extracted, 'bin'), DOTNET_MULTILEVEL_LOOKUP: '0', DOTNET_EnableDiagnostics: '0' } : {}, trusted_roots: [id === 'python' ? path.join(extracted, 'bin') : extracted], platforms: sdkPlatforms };
    await fs.writeFile(configPath, `${JSON.stringify(config, null, 2)}\n`);
    console.log(`Built and smoke-tested local ${id}@${definition.version}: ${destination}`);
    return destination;
  } finally { await fs.rm(temporary, { recursive: true, force: true }); }
}
