import fs from 'node:fs/promises';
import { createReadStream, createWriteStream } from 'node:fs';
import path from 'node:path';
import os from 'node:os';
import { createHash } from 'node:crypto';
import { pipeline } from 'node:stream/promises';
import { Readable } from 'node:stream';
import { execFileSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';

const repository = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
export const nativeTargets = { 'darwin-arm64': 'aarch64-apple-darwin', 'win32-x64': 'x86_64-pc-windows-msvc' };

/** 校验固定清单，禁止 latest、非 HTTPS、越界目录和缺失摘要进入安装流程。 */
export function validateToolchainManifest(manifest) {
  if (manifest.schema_version !== 1) throw new Error('Unsupported native toolchain manifest');
  for (const target of Object.values(nativeTargets)) {
    const entries = manifest.targets?.[target];
    if (!Array.isArray(entries) || entries.length !== 6 || new Set(entries.map(item => item.name)).size !== 6) throw new Error(`Incomplete toolchains: ${target}`);
    for (const item of entries) {
      if (!['go', 'dotnet', 'dotnet-ref8', 'jdk25', 'jdk17', 'llvm'].includes(item.name) || !/^[\w.+-]+$/.test(item.version)) throw new Error('Invalid tool identity');
      const url = new URL(item.url);
      if (url.protocol !== 'https:' || !['go.dev', 'builds.dotnet.microsoft.com', 'github.com', 'api.nuget.org'].includes(url.hostname) || /latest/i.test(item.url)) throw new Error('Unpinned download source');
      if (item.name === 'dotnet-ref8' && (item.version !== '8.0.0' || item.url !== 'https://api.nuget.org/v3-flatcontainer/microsoft.netcore.app.ref/8.0.0/microsoft.netcore.app.ref.8.0.0.nupkg' || item.algorithm !== 'sha512' || item.environment !== 'NARRANT_DOTNET_REF8_ROOT' || item.home !== '.')) throw new Error('Unpinned .NET reference pack');
      if (url.hostname === 'api.nuget.org' && item.name !== 'dotnet-ref8') throw new Error('Unapproved NuGet toolchain');
      const size = { sha256: 64, sha512: 128 }[item.algorithm];
      if (!size || !new RegExp(`^[0-9a-f]{${size}}$`).test(item.digest)) throw new Error('Missing fixed digest');
      if (typeof item.home !== 'string' || item.home.startsWith('/') || item.home.includes('\\') || item.home.split('/').includes('..') || item.home.includes(':')) throw new Error('Tool home escapes isolated directory');
      if (!/^NARRANT_[A-Z0-9_]+$/.test(item.environment)) throw new Error('Invalid environment variable');
    }
  }
  return manifest;
}

/** 流式校验下载文件，LLVM 大归档不会整份进入内存。 */
export async function verifyArchive(file, entry) {
  const hash = createHash(entry.algorithm);
  for await (const chunk of createReadStream(file)) hash.update(chunk);
  if (hash.digest('hex') !== entry.digest) throw new Error(`Checksum mismatch: ${entry.name}`);
}

/** 下载并核验固定官方构件；只在验证成功后允许解包。 */
async function download(entry, archive) {
  const started = Date.now();
  const response = await fetch(entry.url, { signal: AbortSignal.timeout(30 * 60 * 1000) });
  if (!response.ok || !response.body) throw new Error(`Download failed: ${entry.url} (${response.status})`);
  await pipeline(Readable.fromWeb(response.body), createWriteStream(archive, { flags: 'wx' }));
  console.log(`Downloaded ${entry.name} in ${((Date.now() - started) / 1000).toFixed(1)}s`);
  await verifyArchive(archive, entry);
  console.log(`Verified ${entry.name} ${entry.algorithm}`);
}

/** 安装到调用者指定的私有目录，失败时清理临时解包结果，绝不运行系统安装器。 */
export async function provisionToolchains(output, target = nativeTargets[`${process.platform}-${process.arch}`]) {
  const manifest = validateToolchainManifest(JSON.parse(await fs.readFile(path.join(repository, 'resources/native-toolchains.json'), 'utf8')));
  if (!target || target !== nativeTargets[`${process.platform}-${process.arch}`]) throw new Error('Toolchains must be provisioned on their native OS/CPU');
  output = path.resolve(output);
  await fs.mkdir(output, { recursive: true });
  const environment = {};
  for (const entry of manifest.targets[target]) {
    const destination = path.join(output, `${entry.name}-${entry.version}`);
    // 已存在目录不盲信旧内容；一次 CI 安装只接受新建的目标。
    try { await fs.access(destination); throw new Error(`Tool directory already exists: ${destination}`); } catch (error) { if (error.code !== 'ENOENT') throw error; }
    const temporary = await fs.mkdtemp(path.join(output, '.install-'));
    try {
      const archive = path.join(temporary, decodeURIComponent(new URL(entry.url).pathname.split('/').at(-1)));
      const extracted = path.join(temporary, 'unpacked');
      await fs.mkdir(extracted);
      console.log(`Installing fixed ${entry.name} ${entry.version} for ${target}`);
      await download(entry, archive);
      // 两平台的 bsdtar 支持 tar/zip；引用专用 nupkg 也是 zip，不运行其构建脚本。
      const extractionStarted = Date.now();
      execFileSync(process.platform === 'win32' ? path.join(process.env.SystemRoot, 'System32/tar.exe') : '/usr/bin/tar', ['-xf', archive, '-C', extracted], { stdio: 'inherit' });
      console.log(`Extracted ${entry.name} in ${((Date.now() - extractionStarted) / 1000).toFixed(1)}s`);
      const home = path.join(extracted, entry.home);
      if (!(await fs.stat(home)).isDirectory()) throw new Error(`Missing tool home: ${entry.name}`);
      await fs.rename(extracted, destination);
      environment[entry.environment] = path.join(destination, entry.home);
    } finally { await fs.rm(temporary, { recursive: true, force: true }); }
  }
  environment.DOTNET_ROOT = environment.NARRANT_DOTNET_SDK;
  environment.DOTNET_CLI_HOME = path.join(output, 'dotnet-home');
  environment.DOTNET_CLI_TELEMETRY_OPTOUT = '1';
  environment.DOTNET_SKIP_FIRST_TIME_EXPERIENCE = '1';
  environment.DOTNET_GENERATE_ASPNET_CERTIFICATE = 'false';
  environment.GOTOOLCHAIN = 'local';
  environment.GOPROXY = 'off';
  environment.GOSUMDB = 'off';
  await fs.writeFile(path.join(output, 'environment.json'), `${JSON.stringify(environment, null, 2)}\n`);
  if (process.env.GITHUB_ENV) await fs.appendFile(process.env.GITHUB_ENV, Object.entries(environment).map(([name, value]) => `${name}=${value}\n`).join(''));
  return environment;
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  await provisionToolchains(process.argv[2] ?? path.join(os.tmpdir(), 'narrant-native-toolchains'));
}
