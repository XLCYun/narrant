import fs from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { createHash } from 'node:crypto';
import { createOssutilStorage } from './narrant-oss-stable.mjs';

const repository = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
export const PACK_PREFIX = 'narrant/language-support-packs';
const targets = ['aarch64-apple-darwin', 'x86_64-pc-windows-msvc'];

/** 上传前一次性验证所有固定版本产物，避免不完整目录造成半次发布。 */
export async function readArtifacts(directory) {
  const catalog = JSON.parse(await fs.readFile(path.join(repository, 'resources/language-support-packs.json'), 'utf8'));
  const artifacts = [];
  for (const [id, definition] of Object.entries(catalog.packs)) for (const target of targets) {
    const key = `${PACK_PREFIX}/${id}/${definition.version}/${target}`;
    const root = path.join(directory, id, definition.version, target);
    const body = await fs.readFile(path.join(root, 'manifest.json'));
    const manifest = JSON.parse(body);
    const archive = await fs.readFile(path.join(root, 'pack.tar.gz'));
    if (manifest.schemaVersion !== 1 || manifest.id !== id || manifest.version !== definition.version || manifest.target !== target ||
      manifest.archiveSize !== archive.length || manifest.archiveSha256 !== createHash('sha256').update(archive).digest('hex')) {
      throw new Error(`Invalid language pack artifact: ${key}`);
    }
    artifacts.push({ key, body, archive });
  }
  return artifacts;
}

/** 不覆盖既有版本；归档上传并回读成功后才发布该版本的入口清单。 */
export async function publishArtifacts(artifacts, storage) {
  for (const { key, body, archive } of artifacts) {
    await storage.ensureObject(`${key}/pack.tar.gz`, archive, { cacheControl: 'public,max-age=31536000,immutable', contentType: 'application/gzip' });
    if (!(await storage.readObject(`${key}/pack.tar.gz`)).equals(archive)) throw new Error(`Uploaded archive differs: ${key}`);
    await storage.ensureObject(`${key}/manifest.json`, body, { cacheControl: 'public,max-age=31536000,immutable', contentType: 'application/json' });
    if (!(await storage.readObject(`${key}/manifest.json`)).equals(body)) throw new Error(`Uploaded manifest differs: ${key}`);
    console.log(`Published ${key}`);
  }
}

/** 仅发布受保护流程交付的四个产物，使用独立前缀和短期 STS 凭据。 */
async function main() {
  const artifacts = await readArtifacts(path.resolve(process.argv[2] ?? 'dist/language-support-packs'));
  const storage = createOssutilStorage({ prefix: PACK_PREFIX, bucket: process.env.OSS_BUCKET, region: process.env.OSS_REGION,
    credentials: { accessKeyId: process.env.ALIBABA_CLOUD_ACCESS_KEY_ID, accessKeySecret: process.env.ALIBABA_CLOUD_ACCESS_KEY_SECRET, securityToken: process.env.ALIBABA_CLOUD_SECURITY_TOKEN },
  });
  await publishArtifacts(artifacts, storage);
  // 校验应用实际访问的公开 HTTPS 入口，不能以 OSS 上传成功代替分发成功。
  for (const { key, body, archive } of artifacts) for (const [name, expected] of [['manifest.json', body], ['pack.tar.gz', archive]]) {
    const response = await fetch(`https://updates.cyunlab.com/${key}/${name}`, { redirect: 'error', signal: AbortSignal.timeout(180_000) });
    if (!response.ok || !Buffer.from(await response.arrayBuffer()).equals(expected)) throw new Error(`Public distribution verification failed: ${key}/${name}`);
  }
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) main().catch(error => { console.error(error.message); process.exitCode = 1; });
