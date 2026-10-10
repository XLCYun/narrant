import fs from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { createHash } from 'node:crypto';
import { execFileSync } from 'node:child_process';

const repository = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const packages = [
  ['typescript-language-server', '6.0.0', 'sha512-LXtzY3UZGfghWA5eRU6/T5j1+YiGRgy14mR3GOKyTKlE1op1TYKQnLVxwBsmnXeDhGLuvzZyIHBAqvrekAITYQ=='],
  ['typescript', '6.0.3', 'sha512-y2TvuxSZPDyQakkFRPZHKFm+KKVqIisdg9/CZwm9ftvKXLP8NRWj38/ODjNbr43SsoXqNuAisEf1GdCxqWcdBw=='],
  ['@types/node', '22.20.1', 'sha512-EANqOCF9QFyra+4pfxUcX9STKJpCLjMbObVzljIJomAWSnuSIEAvyzEU53GaajbXJEgdh0iEcPL+DGvpUd4k1Q=='],
  ['undici-types', '6.21.0', 'sha512-iwDZqg0QAGrg9Rav5H4n0M64c3mkR59cJ6wQp+7C4nI0gsmExaedaYLNO44eT4AtBBwjbTiGPMlt2Md0T9H9JQ=='],
];

/** 检查 TLS 的最低 Node 版本；不使用应用 node_modules 中的 semver。 */
export function supportedNode(version) {
  version = version.replace(/^v/, '');
  if (!/^\d+\.\d+\.\d+$/.test(version)) return false;
  const parts = version.split('.').map(Number);
  return parts.length === 3 && parts.every(Number.isInteger)
    && (parts[0] > 22 || (parts[0] === 22 && (parts[1] > 22 || (parts[1] === 22 && parts[2] >= 2))));
}

export { validateArchive } from './archive-validation.mjs';
import { validateArchive } from './archive-validation.mjs';

/** 从固定 npm tarball 安装到新目录，验证 SHA-512，绝不调用 npm/pnpm 安装脚本。 */
async function download(destination, [name, version, integrity]) {
  const basename = name.split('/').at(-1);
  const url = `https://registry.npmjs.org/${name}/-/${basename}-${version}.tgz`;
  const response = await fetch(url, { signal: AbortSignal.timeout(120_000) });
  if (!response.ok) throw new Error(`下载 ${name} 失败：HTTP ${response.status}`);
  const bytes = Buffer.from(await response.arrayBuffer());
  if (`sha512-${createHash('sha512').update(bytes).digest('base64')}` !== integrity) {
    throw new Error(`${name}@${version} 完整性校验失败`);
  }
  const archive = path.join(destination, `${basename}.tgz`);
  await fs.writeFile(archive, bytes);
  validateArchive(execFileSync('tar', ['-tzf', archive], { encoding: 'utf8' }),
    execFileSync('tar', ['-tvzf', archive], { encoding: 'utf8' }));
  const target = path.join(destination, name);
  await fs.mkdir(target, { recursive: true });
  execFileSync('tar', ['-xzf', archive, '-C', target, '--strip-components=1']);
  await fs.rm(archive);
  const metadata = JSON.parse(await fs.readFile(path.join(target, 'package.json'), 'utf8'));
  if (metadata.name !== name || metadata.version !== version) throw new Error(`${name} 版本不匹配`);
  console.log(`${name}@${version}: SHA-512 verified; no install scripts`);
}

/** 显式准备开发工具，保留旧版本直至新目录完整；运行分析时不会调用本脚本。 */
async function main() {
  if (!supportedNode(process.versions.node)) throw new Error('TypeScript 分析工具需要 Node >=22.22.2');
  const runtime = path.resolve(process.env.NARRANT_TYPESCRIPT_RUNTIME ?? path.join(repository, '.tools/typescript-analyzer'));
  const node = path.resolve(process.env.NARRANT_TYPESCRIPT_NODE ?? path.join(repository, '.tools', process.platform === 'win32' ? 'node.exe' : 'node'));
  if (node === runtime || node.startsWith(`${runtime}${path.sep}`)) throw new Error('Node 工具路径不能位于运行时目录内');
  await fs.mkdir(path.dirname(runtime), { recursive: true });
  const staging = await fs.mkdtemp(`${runtime}.prepare-`);
  const backup = `${staging}.previous`;
  let replaced = false;
  try {
    for (const entry of packages) await download(staging, entry);
    try { await fs.rename(runtime, backup); replaced = true; } catch (error) { if (error.code !== 'ENOENT') throw error; }
    try { await fs.rename(staging, runtime); } catch (error) { if (replaced) await fs.rename(backup, runtime); throw error; }
    await fs.mkdir(path.dirname(node), { recursive: true });
    // 复制当前受支持的二进制，不依赖开发者的 nvm 目录在以后仍然存在。
    const temporaryNode = `${node}.prepare-${process.pid}`;
    await fs.copyFile(process.execPath, temporaryNode);
    await fs.chmod(temporaryNode, 0o755);
    await fs.rename(temporaryNode, node);
    if (replaced) await fs.rm(backup, { recursive: true, force: true });
    console.log(`Node ${process.versions.node}: ${node}\nRuntime: ${runtime}`);
  } finally {
    await fs.rm(staging, { recursive: true, force: true });
  }
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  main().catch(error => { console.error(error.message); process.exitCode = 1; });
}
