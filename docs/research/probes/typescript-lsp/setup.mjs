import fs from 'node:fs/promises';
import path from 'node:path';
import { createHash } from 'node:crypto';
import { execFileSync } from 'node:child_process';

const root = path.resolve(process.env.NARRANT_TS_PROBE_DIR ?? '.tools/typescript-code-graph-probe');
const packages = [
  { name: 'typescript-language-server', version: '6.0.0', dir: 'tls6', integrity: 'sha512-LXtzY3UZGfghWA5eRU6/T5j1+YiGRgy14mR3GOKyTKlE1op1TYKQnLVxwBsmnXeDhGLuvzZyIHBAqvrekAITYQ==' },
  { name: 'typescript', version: '6.0.3', dir: 'ts6', integrity: 'sha512-y2TvuxSZPDyQakkFRPZHKFm+KKVqIisdg9/CZwm9ftvKXLP8NRWj38/ODjNbr43SsoXqNuAisEf1GdCxqWcdBw==' },
];
await fs.mkdir(root, { recursive: true });

/** 仅下载并解包固定分析工具；校验完整性，不执行 npm 安装脚本。 */
async function download(entry) {
  const url = `https://registry.npmjs.org/${entry.name}/-/${entry.name}-${entry.version}.tgz`;
  const response = await fetch(url);
  if (!response.ok) throw new Error(`${response.status}: ${url}`);
  const bytes = Buffer.from(await response.arrayBuffer());
  const digest = `sha512-${createHash('sha512').update(bytes).digest('base64')}`;
  if (digest !== entry.integrity) throw new Error(`完整性校验失败: ${entry.name}`);
  const archive = path.join(root, `${entry.dir}.tgz`);
  await fs.writeFile(archive, bytes);
  const entries = execFileSync('tar', ['-tzf', archive], { encoding: 'utf8' }).trim().split('\n');
  if (entries.some(file => !file.startsWith('package/') || file.split('/').includes('..'))) throw new Error('不合法的归档路径');
  const destination = path.join(root, entry.dir);
  await fs.mkdir(destination, { recursive: true });
  execFileSync('tar', ['-xzf', archive, '-C', destination, '--strip-components=1']);
  return { ...entry, url, bytes: bytes.length, files: entries.length };
}

const results = await Promise.allSettled(packages.map(download));
await fs.writeFile(path.join(root, 'packages.json'), JSON.stringify(results, null, 2) + '\n');
for (const result of results) {
  if (result.status === 'rejected') { console.error(result.reason); process.exitCode = 1; }
  else console.log(`${result.value.name}@${result.value.version}: integrity verified`);
}
