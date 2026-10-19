import fs from 'node:fs/promises';
import path from 'node:path';
import process from 'node:process';
import { fileURLToPath } from 'node:url';
import { Buffer } from 'node:buffer';
import { createHash } from 'node:crypto';
import { readGitHubReleaseAsset } from '../release/public-draft-assets.mjs';

const repositoryRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../..');
const REPOSITORY = 'XLCYun/narrant';
const TARGETS = ['aarch64-apple-darwin', 'x86_64-pc-windows-msvc'];

/** 上传前验证完整双平台产物；一个语言版本对应一个独立 Release。 */
export async function readArtifacts(directory) {
  const catalog = JSON.parse(await fs.readFile(path.join(repositoryRoot, 'resources/language-support-packs.json'), 'utf8'));
  const artifacts = [];
  for (const [id, definition] of Object.entries(catalog.packs).filter(([, entry]) => entry.published !== false)) {
    for (const target of TARGETS) {
      const root = path.join(directory, id, definition.version, target);
      const body = await fs.readFile(path.join(root, 'manifest.json'));
      const manifest = JSON.parse(body);
      const archive = await fs.readFile(path.join(root, 'pack.tar.gz'));
      if (manifest.schemaVersion !== 1 || manifest.id !== id || manifest.version !== definition.version || manifest.target !== target ||
          manifest.archiveSize !== archive.length || manifest.archiveSha256 !== createHash('sha256').update(archive).digest('hex')) {
        throw new Error(`Invalid language pack artifact: ${id}/${definition.version}/${target}`);
      }
      artifacts.push({ id, version: definition.version, target, body, archive });
    }
  }
  return artifacts;
}

/** 先在 Draft 中上传并逐字节复核两平台，再公开为不会成为应用 Latest 的 prerelease。 */
export async function publishArtifacts(artifacts, github) {
  const groups = new Map();
  for (const artifact of artifacts) {
    if (!/^[a-z][a-z0-9-]*$/.test(artifact.id) || !/^\d+\.\d+\.\d+$/.test(artifact.version) || !TARGETS.includes(artifact.target)) {
      throw new Error('Invalid language pack release identity');
    }
    const tag = `language-pack-${artifact.id}-v${artifact.version}`;
    const entries = groups.get(tag) ?? [];
    entries.push(artifact);
    groups.set(tag, entries);
  }
  // 在任何远端写入前拒绝缺少平台或重复平台的整批输入。
  for (const entries of groups.values()) {
    if (entries.length !== TARGETS.length || new Set(entries.map(entry => entry.target)).size !== TARGETS.length) {
      throw new Error('Language pack release requires exactly both platforms');
    }
  }
  for (const [tag, entries] of groups) {
    const assets = entries.flatMap(({ target, body, archive }) => [
      { name: `${target}-pack.tar.gz`, bytes: archive },
      { name: `${target}-manifest.json`, bytes: body },
    ]);
    let release = await github.findRelease(tag);
    if (!release) release = await github.createRelease({ tag_name: tag, name: tag, body: 'Narrant language support pack. This is not an application update.', draft: true, prerelease: true, make_latest: 'false' });
    if (release.tag_name !== tag || release.prerelease !== true || !Number.isSafeInteger(release.id) || release.id <= 0 || typeof release.draft !== 'boolean' || !Array.isArray(release.assets)) throw new Error('Language pack Release identity differs');
    const existing = new Map(release.assets.map(asset => [asset.name, asset]));
    if (existing.size !== release.assets.length || [...existing.keys()].some(name => !assets.some(asset => asset.name === name))) {
      throw new Error('Language pack Release has unexpected assets');
    }
    for (const { name, bytes } of assets) {
      let asset = existing.get(name);
      if (!asset) {
        if (!release.draft) throw new Error('Published language pack assets cannot be changed');
        asset = await github.uploadAsset(release.id, name, bytes);
      }
      if (asset.name !== name) throw new Error('Uploaded language pack asset name differs');
      if (!(await github.readAsset(asset)).equals(bytes)) throw new Error(`Uploaded asset differs: ${tag}/${name}`);
    }
    if (release.draft) {
      const releaseId = release.id;
      release = await github.publishRelease(release.id, { draft: false, prerelease: true, make_latest: 'false' });
      if (release.id !== releaseId || release.tag_name !== tag || release.draft !== false || release.prerelease !== true) throw new Error('Language pack publication was not confirmed');
    }
    for (const { name, bytes } of assets) {
      if (!(await github.readPublishedAsset(tag, name)).equals(bytes)) throw new Error(`Public distribution differs: ${tag}/${name}`);
    }
  }
}

/** 固定仓库的 GitHub adapter；令牌只发给 API，资产重定向沿用公共下载校验。 */
export function createGitHubPublisher(token, fetchImpl = globalThis.fetch) {
  if (!token || token.trim() !== token) throw new Error('Missing GH_TOKEN');
  const apiBase = `https://api.github.com/repos/${REPOSITORY}`;
  /** 发送发布 API 请求，错误只包含操作和 HTTP 状态。 */
  async function request(url, method = 'GET', body, binary = false) {
    const response = await fetchImpl(url, {
      method, redirect: 'error', signal: AbortSignal.timeout(180_000),
      headers: { Accept: 'application/vnd.github+json', Authorization: `Bearer ${token}`, 'X-GitHub-Api-Version': '2022-11-28', 'Content-Type': binary ? 'application/octet-stream' : 'application/json' },
      ...(body === undefined ? {} : { body: binary ? body : JSON.stringify(body) }),
    });
    if (!response.ok) throw new Error(`GitHub language pack ${method} failed: HTTP ${response.status}`);
    return response.json();
  }
  return {
    /** 列表查询也包含授权可见的 Draft，避免重试时重复创建。 */
    async findRelease(tag) {
      for (let page = 1; page <= 100; page++) {
        const releases = await request(`${apiBase}/releases?per_page=100&page=${page}`);
        if (!Array.isArray(releases)) throw new Error('Invalid GitHub release list');
        const matches = releases.filter(release => release.tag_name === tag);
        if (matches.length > 1) throw new Error('Duplicate language pack releases');
        if (matches.length) return matches[0];
        if (releases.length < 100) return undefined;
      }
      throw new Error('Release list exceeds supported pagination');
    },
    /** 在当前提交上创建不可成为 Latest 的语言包候选。 */
    createRelease: body => request(`${apiBase}/releases`, 'POST', { ...body, target_commitish: process.env.GITHUB_SHA ?? 'main' }),
    /** 按固定 Release ID 上传一个命名资产。 */
    uploadAsset: (id, name, bytes) => request(`https://uploads.github.com/repos/${REPOSITORY}/releases/${id}/assets?name=${encodeURIComponent(name)}`, 'POST', bytes, true),
    /** 使用官方 API 回读已上传字节。 */
    readAsset: asset => readGitHubReleaseAsset(asset, token, fetchImpl),
    /** 公开已验证的同一个语言包 Release。 */
    publishRelease: (id, body) => request(`${apiBase}/releases/${id}`, 'PATCH', body),
    /** 发布后匿名回读，证明普通客户端无需任何仓库凭据。 */
    async readPublishedAsset(tag, name) {
      const response = await fetchImpl(`https://github.com/${REPOSITORY}/releases/download/${tag}/${name}`, { redirect: 'follow', signal: AbortSignal.timeout(180_000) });
      const url = new URL(response.url);
      if (!response.ok || url.protocol !== 'https:' || !['github.com', 'release-assets.githubusercontent.com', 'objects.githubusercontent.com'].includes(url.hostname)) throw new Error('Public GitHub language pack download failed');
      return Buffer.from(await response.arrayBuffer());
    },
  };
}

/** 受保护 workflow 发布已启用版本；未启用候选不会自动变成公开支持包。 */
async function main() {
  const artifacts = await readArtifacts(path.resolve(process.argv[2] ?? 'dist/language-support-packs'));
  if (artifacts.length === 0) { console.log('No published language pack versions are enabled.'); return; }
  await publishArtifacts(artifacts, createGitHubPublisher(process.env.GH_TOKEN));
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) main().catch(error => { console.error(error.message); process.exitCode = 1; });
