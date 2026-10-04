import fs from 'node:fs/promises';
import path from 'node:path';
import os from 'node:os';
import { spawn, execFileSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';

const repository = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const nativeIds = ['java', 'go', 'csharp', 'c-cpp'];

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
  } catch (error) { primaryError = error; throw error; }
  finally {
    try { await fs.rm(root, { recursive: true, force: true }); }
    catch (error) {
      if (!primaryError) throw error;
      primaryError.message += `; scratch cleanup failed: ${error.message}`;
    }
  }
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
    if (id === 'go') {
      const sdk = await findDirectory([process.env.NARRANT_GO_SDK, windows ? 'C:\\Program Files\\Go' : '/usr/local/go'], 'Go 1.23.8 SDK');
      if (!(await fs.readFile(path.join(sdk, 'VERSION'), 'utf8')).startsWith(`go${definition.go}\n`)) throw new Error('Go SDK version mismatch');
      executable = path.join(payload, `bin/narrant-go${suffix}`);
      execFileSync(path.join(sdk, `bin/go${suffix}`), ['build', '-trimpath', '-o', executable, '.'], { cwd: path.join(repository, 'native-helpers/go'), stdio: 'inherit', env: { ...process.env, GOTOOLCHAIN: 'local', GOWORK: 'off', GOPROXY: 'off', GOSUMDB: 'off' } });
      const standard = path.join(payload, 'runtime/platform/go');
      await copyToolTree(path.join(sdk, 'src'), path.join(standard, 'src'));
      await fs.copyFile(path.join(sdk, 'VERSION'), path.join(standard, 'VERSION'));
      await fs.copyFile(path.join(sdk, 'LICENSE'), path.join(payload, 'licenses/Go-LICENSE'));
      platforms.push({ id: 'go-sdk-1.23.8', kind: 'go-sdk', version: definition.go, root: 'runtime/platform/go', metadata: { goos: windows ? 'windows' : 'darwin', goarch: windows ? 'amd64' : 'arm64', ...(windows ? { goamd64: 'v1' } : {}) } });
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
      const targetJdk = await findDirectory([process.env.NARRANT_JAVA_TARGET_JDK, path.join(repository, '.tools/java-native/jdk-17.0.20.1+1/Contents/Home')], 'Exact Java 17 target JDK');
      const runtimeRelease = await fs.readFile(path.join(runtime, 'release'), 'utf8');
      const targetRelease = await fs.readFile(path.join(targetJdk, 'release'), 'utf8');
      if (!runtimeRelease.includes(`IMPLEMENTOR_VERSION="Temurin-${definition.runtimeJava}"`)) throw new Error('Java helper runtime version mismatch');
      if (!targetRelease.includes(`JAVA_RUNTIME_VERSION="${definition.targetJdk}"`)) throw new Error('Java target JDK version mismatch');
      execFileSync(process.env.NARRANT_PYTHON ?? process.env.PYTHON ?? (windows ? 'python' : 'python3'), [path.join(repository, 'native-helpers/java/build.py'), runtime, path.join(repository, '.tools/java-native/lib')], { stdio: 'inherit', env: { ...process.env, NARRANT_JAVA_RUNTIME: runtime } });
      await copyToolTree(runtime, path.join(payload, 'runtime/jdk'));
      await copyToolTree(path.join(repository, 'native-helpers/java/build/classes'), path.join(payload, 'runtime/helper/classes'));
      await copyToolTree(path.join(repository, '.tools/java-native/lib'), path.join(payload, 'runtime/helper/lib'));
      await copyToolTree(path.join(repository, 'native-helpers/java/dependencies.lock.json'), path.join(payload, 'licenses/Java-dependencies.json'));
      await copyToolTree(path.join(runtime, 'NOTICE'), path.join(payload, 'licenses/Java-runtime-NOTICE'));
      for (const entry of ['release', 'lib/jrt-fs.jar', 'lib/modules', 'legal']) await copyToolTree(path.join(targetJdk, entry), path.join(payload, 'runtime/platform/java17', entry));
      executable = path.join(payload, `runtime/jdk/bin/java${suffix}`);
      args = ['-cp', `${path.join(payload, 'runtime/helper/classes')}${path.delimiter}${path.join(payload, 'runtime/helper/lib/*')}`, 'dev.narrant.java.Main'];
      platforms.push({ id: 'java17', kind: 'java-jdk', version: definition.targetJdk, root: 'runtime/platform/java17', metadata: { release: '17' } });
    } else {
      const sdk = await findDirectory([process.env.NARRANT_LLVM_SDK, '/private/tmp/narrant-clang-native/LLVM-21.1.8-macOS-ARM64'], 'LLVM 21.1.8 SDK');
      if (windows) {
        for (const name of ['NARRANT_MSVC_VERSION', 'NARRANT_VISUAL_STUDIO', 'NARRANT_MSVC_REDIST']) if (!process.env[name]) throw new Error(`Missing fixed Windows toolchain input: ${name}`);
        const source = path.join(repository, 'native-helpers/clang');
        const build = path.join(source, 'build');
        execFileSync('cmake', ['-S', source, '-B', build, '-G', 'Visual Studio 17 2022', '-A', 'x64', '-T', `v143,version=${process.env.NARRANT_MSVC_VERSION}`, '-DCMAKE_SYSTEM_VERSION=10.0.26100.0', `-DCMAKE_GENERATOR_INSTANCE=${process.env.NARRANT_VISUAL_STUDIO}`, `-DLLVM_DIR=${path.join(sdk, 'lib/cmake/llvm')}`, `-DClang_DIR=${path.join(sdk, 'lib/cmake/clang')}`], { stdio: 'inherit' });
        execFileSync('cmake', ['--build', build, '--config', 'Release', '--parallel', '2'], { stdio: 'inherit' });
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
        sdkPlatforms.push({ id: `windows-sdk-${path.basename(sdkRoot)}`, kind: 'clang-target', version: path.basename(sdkRoot), root: sdkRoot, metadata: { target_triple: 'x86_64-pc-windows-msvc', c_standard: 'c17', cpp_standard: 'c++20', system_include_directories: ['ucrt', 'shared', 'um', 'winrt', 'cppwinrt'], resource_platform_id: 'clang-resource-21', additional_system_platform_ids: ['msvc-standard-headers'] } });
      } else {
        const sdkRoot = await findDirectory([process.env.NARRANT_MACOS_SDK, '/Library/Developer/CommandLineTools/SDKs/MacOSX.sdk', '/Applications/Xcode.app/Contents/Developer/Platforms/MacOSX.platform/Developer/SDKs/MacOSX.sdk'], 'macOS target SDK');
        const settings = JSON.parse(await fs.readFile(path.join(sdkRoot, 'SDKSettings.json'), 'utf8'));
        const version = settings.Version;
        if (typeof version !== 'string' || !/^\d+(\.\d+)+$/.test(version)) throw new Error('macOS SDK has no version metadata');
        const deploymentTarget = settings.DefaultProperties?.MACOSX_DEPLOYMENT_TARGET ?? version;
        sdkPlatforms.push({ id: `macos-${version}`, kind: 'clang-target', version, root: sdkRoot, metadata: { target_triple: `arm64-apple-macosx${deploymentTarget}`, c_standard: 'c17', cpp_standard: 'c++20', system_include_directories: ['usr/include/c++/v1', 'usr/include'], resource_platform_id: 'clang-resource-21' } });
      }
    }
    config[helper] = { executable: relocate(executable), args: args.map(relocate), environment: id === 'csharp' ? { DOTNET_ROOT: path.join(extracted, 'bin'), DOTNET_MULTILEVEL_LOOKUP: '0', DOTNET_EnableDiagnostics: '0' } : {}, trusted_roots: [extracted], platforms: sdkPlatforms };
    await fs.writeFile(configPath, `${JSON.stringify(config, null, 2)}\n`);
    console.log(`Built and smoke-tested local ${id}@${definition.version}: ${destination}`);
    return destination;
  } finally { await fs.rm(temporary, { recursive: true, force: true }); }
}
