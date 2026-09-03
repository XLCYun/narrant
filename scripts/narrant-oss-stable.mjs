import { spawn } from "node:child_process";
import { Buffer } from "node:buffer";
import { createHash, createHmac } from "node:crypto";
import { chmod, mkdtemp, readFile, readdir, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import path from "node:path";
import process from "node:process";
import { URL } from "node:url";

import { expectedDraftAssetNames } from "./public-draft-assets.mjs";
import { releaseAssetNames } from "./release-assets.mjs";

const TARGETS = ["darwin-aarch64", "windows-x86_64"];

/** 去除对象键首尾斜杠，防止逃离 Narrant OSS 前缀。 */
function normalizePrefix(prefix) {
  const normalized = String(prefix ?? "").replace(/^\/+|\/+$/g, "");
  if (!normalized || normalized.split("/").some((segment) => segment === "." || segment === "..")) {
    throw new Error("需要安全的 OSS 应用前缀");
  }
  return normalized;
}

const OSSUTIL_OUTPUT_BOUND = 128 * 1024;

/** 以无 shell子进程运行 ossutil，并有界保留 stdout/stderr 的真实失败诊断。 */
export function runOssutilCommand(args, options = {}) {
  return new Promise((resolve, reject) => {
    const child = spawn(options.executable ?? 'ossutil', args, {
      env: options.env,
      shell: false,
      stdio: ['ignore', 'pipe', 'pipe'],
      windowsHide: true
    })
    const stdout = []
    const stderr = []
    let outputBytes = 0
    let outputExceeded = false
    /** 累计单个输出块，并在合计超过证据上限时终止子进程。 */
    function capture(target, chunk) {
      outputBytes += chunk.length
      if (outputBytes > OSSUTIL_OUTPUT_BOUND) {
        outputExceeded = true
        child.kill()
      } else {
        target.push(chunk)
      }
    }
    child.stdout.on('data', chunk => capture(stdout, chunk))
    child.stderr.on('data', chunk => capture(stderr, chunk))
    child.once('error', reject)
    child.once('exit', code => {
      const stdoutBody = Buffer.concat(stdout)
      const stderrBody = Buffer.concat(stderr).toString('utf8')
      if (outputExceeded) reject(new Error('ossutil output exceeded the diagnostic bound'))
      else if (code !== 0) {
        const diagnostic = [stderrBody.trim(), stdoutBody.toString('utf8').trim()].filter(Boolean).join('\n').slice(0, 4096)
        reject(new Error(`ossutil exited ${code}: ${diagnostic}`))
      } else resolve({ stdout: stdoutBody, stderr: stderrBody })
    })
  })
}

/** 创建使用短期 STS 环境变量的 OSS 存储适配器。 */
export function createOssutilStorage(options) {
  const runOssutil = options.runOssutil ?? runOssutilCommand
  const credentials = options.credentials
  const prefix = normalizePrefix(options.prefix)
  if (!credentials?.accessKeyId || !credentials.accessKeySecret || !credentials.securityToken) {
    throw new Error('short-lived Alibaba Cloud STS credentials are required')
  }
  const commandEnvironment = { PATH: process.env.PATH, SystemRoot: process.env.SystemRoot }
  const request = options.fetch ?? globalThis.fetch
  /** 为单次 ossutil 调用创建最小权限配置，并在调用结束后销毁。 */
  async function withOssutilConfig(run) {
    const directory = await mkdtemp(path.join(tmpdir(), 'narrant-ossutil-config-'))
    const configFile = path.join(directory, 'ossutilconfig')
    try {
      await chmod(directory, 0o700)
      await writeFile(configFile, [
        '[Credentials]',
        `endpoint=oss-${options.region}.aliyuncs.com`,
        `accessKeyID=${credentials.accessKeyId}`,
        `accessKeySecret=${credentials.accessKeySecret}`,
        `stsToken=${credentials.securityToken}`,
        ''
      ].join('\n'), { mode: 0o600 })
      await chmod(configFile, 0o600)
      return await run(configFile)
    } finally {
      await rm(directory, { recursive: true, force: true })
    }
  }
  /** 构造限定在已配置 bucket 内的 OSS URI。 */
  function objectUri(key) {
    if (key.startsWith('/') || key.split('/').some(segment => segment === '..')) throw new Error('unsafe OSS object key')
    if (!key.startsWith(`${prefix}/`)) throw new Error(`OSS object is outside the configured application prefix: ${key}`)
    return `oss://${options.bucket}/${key}`
  }
  /** 使用 OSS Signature V1 和现有 STS 凭据执行不经 shell 的对象请求。 */
  async function signedObjectRequest(method, key, body = Buffer.alloc(0), query = '') {
    objectUri(key)
    const date = (options.now?.() ?? new Date()).toUTCString()
    const contentType = body.length > 0 ? 'application/json; charset=utf-8' : ''
    const canonicalQuery = query ? `?${query}` : ''
    const canonicalResource = `/${options.bucket}/${key}${canonicalQuery}`
    const canonicalHeaders = `x-oss-security-token:${credentials.securityToken}\n`
    const stringToSign = `${method}\n\n${contentType}\n${date}\n${canonicalHeaders}${canonicalResource}`
    const signature = createHmac('sha1', credentials.accessKeySecret).update(stringToSign).digest('base64')
    const encodedKey = key.split('/').map(encodeURIComponent).join('/')
    return request(`https://${options.bucket}.oss-${options.region}.aliyuncs.com/${encodedKey}${canonicalQuery}`, {
      method,
      body: body.length > 0 ? body : undefined,
      headers: {
        ...(contentType ? { 'Content-Type': contentType } : {}),
        Date: date,
        'x-oss-security-token': credentials.securityToken,
        Authorization: `OSS ${credentials.accessKeyId}:${signature}`
      }
    })
  }
  /** 下载 OSS 对象到隔离临时文件，避免 ossutil 的 stdout 计时文本污染对象字节。 */
  async function readObject(key) {
    const temporaryDirectory = await mkdtemp(path.join(tmpdir(), 'narrant-oss-read-'))
    const destination = path.join(temporaryDirectory, 'object')
    try {
      await chmod(temporaryDirectory, 0o700)
      await withOssutilConfig(configFile => runOssutil(
        ['cp', objectUri(key), destination, '--force', '--config-file', configFile],
        { env: commandEnvironment }
      ))
      await chmod(destination, 0o600)
      return await readFile(destination)
    } finally {
      await rm(temporaryDirectory, { recursive: true, force: true })
    }
  }
  /** 将内容写入临时文件后调用 ossutil cp。 */
  async function copyObject(key, body, metadata, allowOverwrite) {
    const temporaryDirectory = await mkdtemp(path.join(tmpdir(), 'narrant-oss-promotion-'))
    const source = path.join(temporaryDirectory, 'object')
    try {
      await writeFile(source, body, { mode: 0o600 })
      const objectMetadata = [`Cache-Control:${metadata.cacheControl}`]
      if (metadata.contentType) objectMetadata.push(`Content-Type:${metadata.contentType}`)
      if (metadata.downloadFilename) {
        if (!/^[A-Za-z0-9][A-Za-z0-9._-]{0,199}$/.test(metadata.downloadFilename)) {
          throw new Error('download filename must be a safe ASCII basename')
        }
        objectMetadata.push(`Content-Disposition:attachment; filename="${metadata.downloadFilename}"`)
      }
      const args = ['cp', source, objectUri(key)]
      if (allowOverwrite) args.push('--force')
      args.push('--meta', objectMetadata.join('#'))
      await withOssutilConfig(configFile => runOssutil(
        [...args, '--config-file', configFile],
        { env: commandEnvironment }
      ))
    } finally {
      await rm(temporaryDirectory, { recursive: true, force: true })
    }
  }
  return {
    /** 确保 immutable 对象只写一次，并仅复用逐字节相同的已有对象。 */
    async ensureObject(key, body, metadata) {
      try {
        const existing = await readObject(key)
        if (!existing.equals(body)) throw new Error(`immutable OSS object already exists with different bytes: ${key}`)
        return 'reused'
      } catch (error) {
        if (!/NoSuchKey|StatusCode[=: ]+404|\b404\b|object does not exist|not found/i.test(String(error?.message ?? error))) throw error
      }
      await copyObject(key, body, metadata, false)
      return 'uploaded'
    },
    /** 覆盖 Stable manifest，并依赖 bucket versioning 保留历史版本。 */
    async replaceObject(key, body, metadata) { await copyObject(key, body, metadata, true) },
    /** 以 AppendObject position=0 原子获取全局 promotion lock；竞争者由 OSS 拒绝。 */
    async acquirePromotionLock(key, ownerBody) {
      const response = await signedObjectRequest('POST', key, ownerBody, 'append&position=0')
      if (response.status === 409 || response.status === 412) throw new Error('promotion lock acquisition conflict')
      if (!response.ok) throw new Error(`promotion lock acquisition failed with OSS status ${response.status}`)
      const remote = await readObject(key)
      if (!remote.equals(ownerBody)) throw new Error('promotion lock owner bytes differ after acquisition')
    },
    /** 仅在远端 owner bytes 仍匹配时释放 promotion lock；异常路径不调用。 */
    async releasePromotionLock(key, ownerBody) {
      const remote = await readObject(key)
      if (!remote.equals(ownerBody)) throw new Error('promotion lock ownership changed')
      const response = await signedObjectRequest('DELETE', key)
      if (!response.ok) throw new Error(`promotion lock release failed with OSS status ${response.status}`)
    },
    readObject
  }
}

/** 返回浏览器保存人工安装包时使用的稳定、简洁文件名。 */
function installerDownloadFilename(version, target) {
  if (target === "darwin-aarch64") return `Narrant-${version}-macOS-arm64.dmg`;
  if (target === "windows-x86_64") return `Narrant-${version}-Windows-x64.exe`;
  throw new Error(`不支持的 Narrant 安装包 target：${target}`);
}



/** 从清单 URL 提取并严格校验 Narrant 内容寻址对象键。 */
function releaseKey(urlText, version, target, filename, digest) {
  let url;
  try { url = new URL(urlText); } catch { throw new Error(`Narrant ${target} 发布 URL 无效`); }
  const expected = `/narrant/releases/${version}/${target}/${digest}-${encodeURIComponent(filename)}`;
  if (url.protocol !== "https:" || url.hostname !== "updates.cyunlab.com" || url.port || url.search || url.hash || url.pathname !== expected) {
    throw new Error(`Narrant ${target} 发布 URL 与资产字节不匹配`);
  }
  return url.pathname.slice(1);
}

/** 在 GitHub 发布前上传并回读全部不可变 OSS 资产及隔离 candidate manifest。 */
export async function prepareNarrantOssCandidate(options, storage) {
  const prefix = normalizePrefix(options.prefix ?? "narrant");
  if (prefix !== "narrant") throw new Error("Narrant OSS 前缀必须固定为 narrant");
  const version = options.version;
  const names = releaseAssetNames(version);
  const expected = expectedDraftAssetNames(version);
  const actual = await readdir(options.directory);
  if (JSON.stringify([...actual].sort()) !== JSON.stringify([...expected].sort())) {
    throw new Error("Narrant OSS candidate 资产集合不完整");
  }
  const manifestBody = await readFile(path.join(options.directory, names.manifest));
  const manifest = JSON.parse(manifestBody.toString("utf8"));
  if (manifest.version !== version || JSON.stringify(Object.keys(manifest.platforms ?? {})) !== JSON.stringify(TARGETS)) {
    throw new Error("Narrant OSS candidate manifest 身份无效");
  }
  const objects = [];
  const mappings = [
    ["darwin-aarch64", names.updater.darwin, names.installer.darwin],
    ["windows-x86_64", names.updater.windows, names.installer.windows],
  ];
  for (const [target, updaterName, installerName] of mappings) {
    const entry = manifest.platforms[target];
    for (const [field, filename] of [["url", updaterName], ["installer_url", installerName]]) {
      const bytes = await readFile(path.join(options.directory, filename));
      const digest = createHash("sha256").update(bytes).digest("hex");
      objects.push({
        key: releaseKey(entry[field], version, target, filename, digest),
        body: bytes,
        contentType: "application/octet-stream",
        downloadFilename: field === "installer_url" ? installerDownloadFilename(version, target) : undefined,
      });
    }
    const signatureName = `${updaterName}.sig`;
    const signatureBody = await readFile(path.join(options.directory, signatureName));
    const signatureDigest = createHash("sha256").update(signatureBody).digest("hex");
    objects.push({
      key: `${prefix}/releases/${version}/${target}/${signatureDigest}-${signatureName}`,
      body: signatureBody,
      contentType: "text/plain; charset=utf-8",
    });
  }
  const checksumsBody = await readFile(path.join(options.directory, names.checksums));
  objects.push({
    key: `${prefix}/releases/${version}/${names.checksums}`,
    body: checksumsBody,
    contentType: "text/plain; charset=utf-8",
  });
  for (const object of objects) {
    await storage.ensureObject(object.key, object.body, {
      cacheControl: "public, max-age=31536000, immutable",
      contentType: object.contentType,
      downloadFilename: object.downloadFilename,
    });
  }
  for (const object of objects) {
    if (!(await storage.readObject(object.key)).equals(object.body)) {
      throw new Error(`Narrant OSS 对象回读不一致：${object.key}`);
    }
  }
  const manifestSha256 = createHash("sha256").update(manifestBody).digest("hex");
  const candidateKey = `${prefix}/candidates/${version}/${options.candidateCommit}/${manifestSha256}-latest.json`;
  await storage.ensureObject(candidateKey, manifestBody, {
    cacheControl: "public, max-age=31536000, immutable",
    contentType: "application/json; charset=utf-8",
  });
  if (!(await storage.readObject(candidateKey)).equals(manifestBody)) throw new Error("Narrant OSS candidate manifest 回读不一致");
  let previousStableSha256 = null;
  try {
    const previous = await storage.readObject(`${prefix}/channels/stable/latest.json`);
    previousStableSha256 = createHash("sha256").update(previous).digest("hex");
    if (options.mode === "bootstrap") {
      throw new Error("Narrant OSS Stable 已存在，拒绝再次 bootstrap");
    }
    let previousManifest;
    try { previousManifest = JSON.parse(previous.toString("utf8")); } catch { throw new Error("Narrant previous Stable JSON 无效"); }
    if (
      options.mode !== "normal" ||
      previousStableSha256 !== options.previousManifestSha256 ||
      `v${previousManifest?.version}` !== options.previousTag
    ) {
      throw new Error("Narrant previous Stable 与 smoke evidence identity 不匹配");
    }
  } catch (error) {
    const isMissing = /NoSuchKey|not found|不存在/i.test(String(error?.message ?? error));
    if (options.mode !== "bootstrap" || options.bootstrapTag !== options.tag || !isMissing) throw error;
  }
  return Object.freeze({
    schema_version: 1,
    tag: options.tag,
    version,
    candidate_commit: options.candidateCommit,
    manifest_sha256: manifestSha256,
    manifest_key: candidateKey,
    previous_stable_manifest_sha256: previousStableSha256,
  });
}

/** 在 GitHub Release 成为 Latest 后，以原子锁保护 Stable manifest 最终写入。 */
export async function finalizeNarrantOssCandidate(candidate, storage, options) {
  if (candidate?.schema_version !== 1 || !/^[0-9a-f]{40}$/.test(candidate.candidate_commit ?? "")) {
    throw new Error("Narrant OSS candidate identity 无效");
  }
  const prefix = normalizePrefix(options.prefix ?? "narrant");
  const manifestBody = await storage.readObject(candidate.manifest_key);
  if (createHash("sha256").update(manifestBody).digest("hex") !== candidate.manifest_sha256) {
    throw new Error("Narrant OSS candidate manifest 摘要不匹配");
  }
  const stableKey = `${prefix}/channels/stable/latest.json`;
  const lockKey = `${prefix}/channels/stable/promotion.lock`;
  const lockBody = Buffer.from(`${JSON.stringify({
    schema_version: 1,
    owner: options.lockOwner,
    candidate_tag: candidate.tag,
    candidate_commit: candidate.candidate_commit,
    candidate_manifest_sha256: candidate.manifest_sha256,
  }, null, 2)}\n`);
  await storage.acquirePromotionLock(lockKey, lockBody);
  if (candidate.previous_stable_manifest_sha256) {
    const previous = await storage.readObject(stableKey);
    if (createHash("sha256").update(previous).digest("hex") !== candidate.previous_stable_manifest_sha256) {
      throw new Error("Narrant previous Stable 在晋升期间发生变化");
    }
  }
  await storage.replaceObject(stableKey, manifestBody, {
    cacheControl: "no-cache",
    contentType: "application/json; charset=utf-8",
  });
  if (!(await storage.readObject(stableKey)).equals(manifestBody)) throw new Error("Narrant Stable manifest 写后回读不一致");
  await storage.releasePromotionLock(lockKey, lockBody);
  return JSON.parse(manifestBody.toString("utf8"));
}
