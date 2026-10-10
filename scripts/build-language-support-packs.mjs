import { migratedNativePacks } from './native-migration.mjs';
import fs from 'node:fs/promises';
import path from 'node:path';
import { createHash } from 'node:crypto';
import { execFileSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import { gunzipSync } from 'node:zlib';
import { create as createTar } from 'tar';
import { verifyPythonRuntime } from './python-language-support-smoke.mjs';
import { validateArchive } from './setup-typescript-analyzer.mjs';

const repository = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const targets = ['aarch64-apple-darwin', 'x86_64-pc-windows-msvc'];

/** 产物键只由受支持的包、版本和平台组成。 */
function validateIdentity({ id, version, target }) {
  if (!['rust', 'typescript-javascript', 'python', 'java', 'go', 'csharp', 'c-cpp'].includes(id) || !/^\d+\.\d+\.\d+$/.test(version) || !targets.includes(target)) throw new Error('Invalid language support pack identity');
}

/** 将准备完成的普通文件目录打包，清单只记录实际产物的完整身份和摘要。 */
export async function packageDirectory(identity, payload, output) {
  validateIdentity(identity);
  const destination = path.join(output, identity.id, identity.version, identity.target);
  await fs.mkdir(destination, { recursive: true });
  const files = [];
  /** 按稳定顺序记录文件，拒绝符号链接和跨平台不安全的文件名。 */
  async function visit(relative = '') {
    for (const name of (await fs.readdir(path.join(payload, relative))).sort()) {
      if (/[\\:]/.test(name) || [...name].some(char => char.charCodeAt(0) < 32 || char.charCodeAt(0) === 127) || name === '..') throw new Error('支持包文件名不安全');
      const file = path.posix.join(relative, name);
      const metadata = await fs.lstat(path.join(payload, file));
      if (metadata.isDirectory()) await visit(file);
      else if (metadata.isFile()) {
        const bytes = await fs.readFile(path.join(payload, file));
        // Windows 运行入口由目标平台和 .exe 后缀确定；POSIX 仍保留真实执行权限。
        const runtimeExecutable = file.startsWith('runtime/') && (identity.target === 'x86_64-pc-windows-msvc' ? file.toLowerCase().endsWith('.exe') : (metadata.mode & 0o111) !== 0);
        files.push({ path: file, size: bytes.length, sha256: createHash('sha256').update(bytes).digest('hex'), executable: file.startsWith('bin/') || runtimeExecutable });
      } else throw new Error('支持包只能包含普通文件和目录');
    }
  }
  await visit();
  // 只打包已登记文件；固定时间与权限，不携带宿主扩展属性或用户信息。
  await createTar({ file: path.join(destination, 'pack.tar.gz'), cwd: payload, gzip: true, portable: true, mtime: new Date(0),
    onWriteEntry: entry => { entry.mode = files.find(file => file.path === entry.path)?.executable ? 0o755 : 0o644; },
  }, files.map(file => file.path));
  const bytes = await fs.readFile(path.join(destination, 'pack.tar.gz'));
  const manifest = { schemaVersion: 1, ...identity, archiveSize: bytes.length, archiveSha256: createHash('sha256').update(bytes).digest('hex'), files };
  await fs.writeFile(path.join(destination, 'manifest.json'), `${JSON.stringify(manifest, null, Object.hasOwn(migratedNativePacks, identity.id) ? undefined : 2)}\n`);
  return destination;
}

/** 固定上游版本和摘要；缓存命中也重新验证，不运行任何包安装脚本。 */
async function download(url, integrity, cache) {
  const separator = integrity.indexOf('-');
  const algorithm = integrity.slice(0, separator);
  const expected = integrity.slice(separator + 1);
  const matches = bytes => createHash(algorithm).update(bytes).digest(algorithm === 'sha512' ? 'base64' : 'hex') === expected;
  const cached = path.join(cache, createHash('sha256').update(url).digest('hex'));
  try { const bytes = await fs.readFile(cached); if (matches(bytes)) return bytes; } catch (error) { if (error.code !== 'ENOENT') throw error; }
  const response = await fetch(url, { signal: AbortSignal.timeout(180_000) });
  if (!response.ok) throw new Error(`Upstream download failed: ${response.status} ${url}`);
  const bytes = Buffer.from(await response.arrayBuffer());
  if (!matches(bytes)) throw new Error(`Upstream checksum mismatch: ${url}`);
  await fs.mkdir(cache, { recursive: true });
  await fs.writeFile(cached, bytes);
  return bytes;
}

/** 从官方固定归档组装一个平台的自包含工具包，并保留许可证文件。 */
export async function buildPack({ id, target, output, cache = path.join(repository, '.tools/language-pack-downloads') }) {
  const catalog = JSON.parse(await fs.readFile(path.join(repository, 'resources/language-support-packs.json'), 'utf8'));
  const definition = catalog.packs[id];
  const identity = { id, version: definition?.version, target };
  validateIdentity(identity);
  if (Object.hasOwn(migratedNativePacks, id)) {
    const { buildNativePack } = await import('./native-language-support-packs.mjs');
    return buildNativePack({ id, target, output, definition, packageDirectory });
  }
  output = path.resolve(output);
  await fs.mkdir(output, { recursive: true });
  const temporary = await fs.mkdtemp(path.join(output, '.build-'));
  const payload = path.join(temporary, 'payload');
  const windows = target === 'x86_64-pc-windows-msvc';
  try {
    await fs.mkdir(path.join(payload, 'bin'), { recursive: true });
    await fs.mkdir(path.join(payload, 'licenses'), { recursive: true });
    if (id === 'rust') {
      const filename = `rust-analyzer-${target}.${windows ? 'zip' : 'gz'}`;
      const bytes = await download(`https://github.com/rust-lang/rust-analyzer/releases/download/${definition.rustAnalyzer}/${filename}`, `sha256-${definition.upstreamSha256[target]}`, cache);
      if (windows) {
        const archive = path.join(temporary, filename);
        await fs.writeFile(archive, bytes);
        execFileSync('tar', ['-xf', archive, '-C', path.join(payload, 'bin'), 'rust-analyzer.exe']);
      } else await fs.writeFile(path.join(payload, 'bin/rust-analyzer'), gunzipSync(bytes));
      for (const name of ['LICENSE-MIT', 'LICENSE-APACHE']) {
        const response = await fetch(`https://raw.githubusercontent.com/rust-lang/rust-analyzer/${definition.sourceCommit}/${name}`, { signal: AbortSignal.timeout(30_000) });
        if (!response.ok) throw new Error(`Unable to fetch pinned rust-analyzer ${name}`);
        await fs.writeFile(path.join(payload, 'licenses', name), Buffer.from(await response.arrayBuffer()));
      }
    } else if (id === 'python') {
      const upstreamRoot = `ty-${target}`;
      const filename = `${upstreamRoot}.${windows ? 'zip' : 'tar.gz'}`;
      const bytes = await download(`https://github.com/astral-sh/ty/releases/download/${definition.ty}/${filename}`, `sha256-${definition.upstreamSha256[target]}`, cache);
      const archive = path.join(temporary, filename);
      await fs.writeFile(archive, bytes);
      const binary = `ty${windows ? '.exe' : ''}`;
      const archiveBinary = windows ? binary : `${upstreamRoot}/${binary}`;
      execFileSync('tar', ['-xf', archive, '-C', temporary, archiveBinary]);
      await fs.copyFile(path.join(temporary, archiveBinary), path.join(payload, 'bin', binary));
      await fs.mkdir(path.join(payload, 'control'));
      await fs.writeFile(path.join(payload, 'control/ty.toml'), '');
      // 标准库直接封装在固定 ty 二进制中；来源与许可证仍独立记录。
      for (const license of definition.licenses) {
        await fs.writeFile(path.join(payload, 'licenses', license.name), await download(license.url, `sha256-${license.sha256}`, cache));
      }
    } else {
      const nodeRoot = `node-v${definition.node}-${windows ? 'win-x64' : 'darwin-arm64'}`;
      const nodeArchive = `${nodeRoot}.${windows ? 'zip' : 'tar.gz'}`;
      const bytes = await download(`https://nodejs.org/dist/v${definition.node}/${nodeArchive}`, `sha256-${definition.upstreamSha256[target]}`, cache);
      const archive = path.join(temporary, nodeArchive);
      await fs.writeFile(archive, bytes);
      const executable = windows ? 'node.exe' : 'bin/node';
      execFileSync('tar', ['-xf', archive, '-C', temporary, `${nodeRoot}/${executable}`, `${nodeRoot}/LICENSE`]);
      await fs.copyFile(path.join(temporary, nodeRoot, executable), path.join(payload, 'bin', windows ? 'node.exe' : 'node'));
      await fs.copyFile(path.join(temporary, nodeRoot, 'LICENSE'), path.join(payload, 'licenses/Node-LICENSE'));
      for (const { name, version, integrity } of definition.packages) {
        const basename = name.split('/').at(-1);
        const bytes = await download(`https://registry.npmjs.org/${name}/-/${basename}-${version}.tgz`, integrity, cache);
        const archive = path.join(temporary, `${basename}.tgz`);
        await fs.writeFile(archive, bytes);
        validateArchive(execFileSync('tar', ['-tzf', archive], { encoding: 'utf8' }), execFileSync('tar', ['-tvzf', archive], { encoding: 'utf8' }));
        const destination = path.join(payload, 'runtime', name);
        await fs.mkdir(destination, { recursive: true });
        execFileSync('tar', ['-xzf', archive, '-C', destination, '--strip-components=1']);
        const metadata = JSON.parse(await fs.readFile(path.join(destination, 'package.json'), 'utf8'));
        if (metadata.name !== name || metadata.version !== version) throw new Error(`Package identity mismatch: ${name}`);
      }
    }
    for (const name of await fs.readdir(path.join(payload, 'bin'))) await fs.chmod(path.join(payload, 'bin', name), 0o755);
    const destination = await packageDirectory(identity, payload, output);
    // 在目标宿主上执行实际工具入口；跨平台产物必须由对应 CI runner 再验证。
    if ((windows && process.platform === 'win32' && process.arch === 'x64') || (!windows && process.platform === 'darwin' && process.arch === 'arm64')) {
      const executable = path.join(payload, 'bin', `${({ rust: 'rust-analyzer', python: 'ty', 'typescript-javascript': 'node' })[id]}${windows ? '.exe' : ''}`);
      const runOptions = { encoding: 'utf8', timeout: 30_000, env: { ...process.env, NODE_OPTIONS: '', NODE_PATH: '' } };
      const version = execFileSync(executable, ['--version'], runOptions).trim();
      console.log(version);
      if (id === 'python') {
        if (!version.startsWith(`ty ${definition.ty} `) && version !== `ty ${definition.ty}`) throw new Error('Python pack ty version mismatch');
        await verifyPythonRuntime(executable, path.join(payload, 'control/ty.toml'));
      }
      if (id === 'typescript-javascript') {
        console.log(execFileSync(executable, [path.join(payload, 'runtime/typescript-language-server/lib/cli.mjs'), '--version'], runOptions).trim());
        execFileSync(executable, ['-e', `const ts = require(process.argv[1]); if (!ts.transpileModule('const n: number = 1', {}).outputText.includes('1')) process.exit(1)`, path.join(payload, 'runtime/typescript/lib/typescript.js')], runOptions);
      }
    }
    console.log(`Built ${id}@${identity.version} for ${target}: ${destination}`);
    return destination;
  } finally { await fs.rm(temporary, { recursive: true, force: true }); }
}

/** 显式构建全部包或某个平台的包；此脚本不会上传到生产分发源。 */
async function main() {
  const args = process.argv.slice(2);
  const option = name => { const at = args.indexOf(name); return at < 0 ? undefined : args[at + 1]; };
  const output = option('--output') ?? path.join(repository, 'dist/language-support-packs');
  const selectedTargets = args.includes('--all') ? targets : [option('--target') ?? (process.platform === 'win32' ? targets[1] : targets[0])];
  if (args.includes('--native') && selectedTargets.some(target => target !== ({ 'darwin-arm64': targets[0], 'win32-x64': targets[1] })[`${process.platform}-${process.arch}`])) throw new Error('Native verification requires a matching OS and CPU');
  const ids = option('--id') ? [option('--id')] : ['rust', 'typescript-javascript', 'python'];
  for (const target of selectedTargets) for (const id of ids) await buildPack({ id, target, output });
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) main().catch(error => { console.error(error.message); process.exitCode = 1; });
