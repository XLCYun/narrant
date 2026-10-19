import fs from 'node:fs/promises';
import { createReadStream, createWriteStream } from 'node:fs';
import path from 'node:path';
import { createHash } from 'node:crypto';
import { fileURLToPath } from 'node:url';
import { Readable, Transform } from 'node:stream';
import { pipeline } from 'node:stream/promises';
import * as tar from 'tar';

export const targets = { 'darwin-arm64': 'aarch64-apple-darwin', 'win32-x64': 'x86_64-pc-windows-msvc' };
export const ids = ['rust', 'typescript-javascript', 'python', 'java', 'go', 'csharp', 'c-cpp'];
export const limits = { manifest: 2 * 1024 * 1024, archive: 256 * 1024 * 1024, unpacked: 1024 * 1024 * 1024 };
const repository = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../..');
const origin = 'https://github.com/XLCYun/CodeGraph/releases/download/v0.1.0';

/** 固定发布元数据不能把消费者切换到浮动 tag 或另一个项目。 */
export function validateCatalog(value) {
  if (value.repository !== 'XLCYun/CodeGraph' || value.tag !== 'v0.1.0' || value.engineVersion !== '0.1.0' || value.protocol !== 'codegraph.analyzer/1' || Object.keys(value.packs ?? {}).sort().join() !== [...ids].sort().join()) throw new Error('CodeGraph catalog identity mismatch');
  for (const id of ids) if (value.packs[id].version !== '0.1.0' || value.packs[id].published !== true) throw new Error(`CodeGraph ${id} 0.1.0 is unpublished; real release is required`);
  return value;
}

/** 同时拒绝 Unix 路径穿越与 Windows 设备、数据流和尾部规范化冲突。 */
export function safePath(value) {
  return typeof value === 'string' && value.length > 0 && !/[\\:]/.test(value) && ![...value].some(character => character.codePointAt(0) < 32 || character.codePointAt(0) === 127) && value.split('/').every(part => part && part !== '.' && part !== '..' && !/[. ]$/.test(part) && !/^(CON|PRN|AUX|NUL|COM[1-9]|LPT[1-9])(?:\.|$)/i.test(part));
}

/** 公开 manifest 严格核对身份、来源与每个文件；不要求 helper 私有布局。 */
export function validateManifest(bytes, id, target) {
  if (bytes.length > limits.manifest) throw new Error('Manifest too large');
  const manifest = JSON.parse(bytes.toString('utf8'));
  if (!Buffer.from(JSON.stringify(manifest) + '\n').equals(bytes)) throw new Error('Manifest must be canonical JSON without duplicate keys');
  const fields = ['schemaVersion','id','version','target','engineVersion','protocol','sourceCommit','sourceDirty','archiveSize','archiveSha256','files'];
  if (Object.keys(manifest).sort().join() !== fields.sort().join() || manifest.schemaVersion !== 1 || manifest.id !== id || manifest.version !== '0.1.0' || manifest.engineVersion !== '0.1.0' || manifest.protocol !== 'codegraph.analyzer/1' || manifest.target !== target || manifest.sourceDirty !== false || !/^[0-9a-f]{40}$/.test(manifest.sourceCommit) || !Number.isSafeInteger(manifest.archiveSize) || manifest.archiveSize <= 0 || manifest.archiveSize > limits.archive || !/^[0-9a-f]{64}$/.test(manifest.archiveSha256) || !Array.isArray(manifest.files) || !manifest.files.length || manifest.files.length > 20000) throw new Error('Invalid CodeGraph manifest identity or limits');
  let size = 0;
  const names = new Set();
  for (const file of manifest.files) {
    if (Object.keys(file).sort().join() !== ['path','size','sha256','executable'].sort().join() || !safePath(file.path) || !/^(bin|control|runtime|licenses)\//.test(file.path) || names.has(file.path.toLowerCase()) || !Number.isSafeInteger(file.size) || file.size < 0 || !/^[0-9a-f]{64}$/.test(file.sha256) || typeof file.executable !== 'boolean' || (size += file.size) > limits.unpacked) throw new Error('Invalid CodeGraph file inventory');
    names.add(file.path.toLowerCase());
  }
  const cli = target.startsWith('x86_64-pc-windows') ? 'bin/codegraph.exe' : 'bin/codegraph';
  if (!manifest.files.some(file => file.path === cli && file.executable) || !manifest.files.some(file => file.path === 'control/codegraph.json')) throw new Error('Missing public CLI or descriptor');
  return manifest;
}

/** 单个 SHA256SUMS 只接受规范摘要与唯一资产名。 */
export function checksums(bytes) {
  const values = new Map();
  for (const line of bytes.toString('utf8').trimEnd().split('\n')) {
    const found = /^([0-9a-f]{64}) {2}([^/\\\r\n]+)$/.exec(line);
    if (!found || values.has(found[2])) throw new Error('Invalid or duplicate release checksum');
    values.set(found[2], found[1]);
  }
  return values;
}

/** 计算磁盘文件摘要，归档不会整份常驻内存。 */
export async function digest(file) {
  const hash = createHash('sha256');
  for await (const chunk of createReadStream(file)) hash.update(chunk);
  return hash.digest('hex');
}

/** 跟随官方资产重定向并流式写入有界下载，失败后由事务目录负责清理。 */
async function download(url, destination, limit) {
  let response;
  for (let hop = 0; hop < 6; hop++) {
    response = await fetch(url, { redirect: 'manual', signal: AbortSignal.timeout(10 * 60 * 1000) });
    if (response.status < 300 || response.status >= 400) break;
    const next = new URL(response.headers.get('location'), url);
    await response.body?.cancel();
    if (next.protocol !== 'https:' || next.username || next.password || next.port || !['release-assets.githubusercontent.com','objects.githubusercontent.com'].includes(next.hostname)) throw new Error('Untrusted CodeGraph asset redirect');
    url = next.href;
  }
  if (!response?.ok || !response.body) throw new Error(`CodeGraph download failed: ${response?.status} ${url}`);
  let received = 0;
  await pipeline(Readable.fromWeb(response.body), new Transform({ transform(chunk, _encoding, callback) { received += chunk.length; callback(received > limit ? new Error('CodeGraph download exceeds limit') : null, chunk); } }), createWriteStream(destination, { flags: 'wx' }));
}

/** 解包前核对条目，解包后核对全文件摘要及公开描述符，绝不合并旧目录。 */
export async function verifyAndExtract(manifest, archive, root) {
  if ((await fs.stat(archive)).size !== manifest.archiveSize || await digest(archive) !== manifest.archiveSha256) throw new Error('CodeGraph archive checksum mismatch');
  const files = new Map(manifest.files.map(file => [file.path, file]));
  const seen = new Set();
  let invalid;
  await tar.t({ file: archive, strict: true, onReadEntry(entry) {
    const name = entry.path.replace(/^\.\//, '').replace(/\/$/, '');
    if (entry.type === 'Directory' && (name === '.' || name === '')) return;
    if (!safePath(name)) { invalid = 'Unsafe archive path'; return; }
    if (entry.type === 'Directory') return;
    const file = files.get(name);
    if (entry.type !== 'File' || !file || seen.has(name) || entry.size !== file.size || (entry.mode & 0o777) !== (file.executable ? 0o755 : 0o644)) invalid = 'Archive entries differ from manifest';
    seen.add(name);
  } });
  if (invalid || seen.size !== files.size) throw new Error(invalid ?? 'Missing archive files');
  await fs.mkdir(root, { recursive: false });
  await tar.x({ file: archive, cwd: root, strict: true, preservePaths: false });
  for (const file of manifest.files) {
    const destination = path.join(root, file.path);
    if (!(await fs.lstat(destination)).isFile() || await digest(destination) !== file.sha256) throw new Error('Extracted file checksum mismatch');
    if (process.platform !== 'win32') await fs.chmod(destination, file.executable ? 0o755 : 0o644);
  }
  const descriptorPath = path.join(root, 'control/codegraph.json');
  if ((await fs.stat(descriptorPath)).size > 1024 * 1024) throw new Error('Public descriptor exceeds limit');
  const descriptorBytes = await fs.readFile(descriptorPath);
  const expected = { schema_version:1, pack_id:manifest.id, pack_version:manifest.version, engine_version:manifest.engineVersion, protocol:manifest.protocol };
  if (descriptorBytes.length > 1024 * 1024) throw new Error('Public descriptor exceeds limit');
  const descriptorText = new TextDecoder('utf-8', { fatal:true }).decode(descriptorBytes);
  const descriptor = JSON.parse(descriptorText);
  // 描述符只允许五个标量字段；逐个解码原始键，JSON.parse 覆盖重复键不能绕过检查。
  const keys = [...descriptorText.matchAll(/("(?:\\.|[^"\\])*")\s*:/g)].map(match => JSON.parse(match[1]));
  if (keys.length !== 5 || new Set(keys).size !== 5) throw new Error('Duplicate or invalid public descriptor fields');
  if (Object.keys(descriptor).sort().join() !== Object.keys(expected).sort().join() || Object.entries(expected).some(([key,value]) => descriptor[key] !== value)) throw new Error('Public descriptor identity mismatch');
}

/** 只获取目录声明的已发布固定版本；未发布和缺失 release 均使 CI 硬失败。 */
export async function downloadPacks(output) {
  validateCatalog(JSON.parse(await fs.readFile(path.join(repository, 'resources/language-support-packs.json'))));
  const target = targets[`${process.platform}-${process.arch}`];
  if (!target) throw new Error('Unsupported CodeGraph consumer target');
  output = path.resolve(output);
  await fs.mkdir(output, { recursive: true });
  const temporary = await fs.mkdtemp(path.join(output, '.download-'));
  const candidates = [];
  try {
    const sumfile = path.join(temporary, 'SHA256SUMS');
    await download(`${origin}/SHA256SUMS`, sumfile, 64 * 1024);
    const sums = checksums(await fs.readFile(sumfile));
    let commit;
    for (const id of ids) {
      const packageRoot = path.join(output, id, '0.1.0', target);
      await fs.mkdir(packageRoot, { recursive: true });
      for (const name of ['manifest.json', 'pack.tar.gz']) {
        const asset = `${id}-${target}-${name}`;
        const destination = path.join(packageRoot, name);
        await download(`${origin}/${asset}`, destination, name === 'manifest.json' ? limits.manifest : limits.archive);
        if (await digest(destination) !== sums.get(asset)) throw new Error(`Release checksum mismatch: ${asset}`);
      }
      const manifest = validateManifest(await fs.readFile(path.join(packageRoot, 'manifest.json')), id, target);
      if (commit && commit !== manifest.sourceCommit) throw new Error('Release packages have inconsistent source commits');
      commit = manifest.sourceCommit;
      const root = path.join(output, 'installed', id);
      await fs.mkdir(path.dirname(root), { recursive: true });
      await verifyAndExtract(manifest, path.join(packageRoot, 'pack.tar.gz'), root);
      candidates.push({ id, version: manifest.version, engineVersion: manifest.engineVersion, protocol: manifest.protocol, target, root });
    }
    await fs.writeFile(path.join(output, 'candidates.json'), JSON.stringify(candidates, null, 2) + '\n');
  } finally { await fs.rm(temporary, { recursive: true, force: true }); }
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) await downloadPacks(process.argv[2] ?? 'codegraph-packs');
