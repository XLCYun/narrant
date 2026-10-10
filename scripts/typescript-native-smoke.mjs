import fs from 'node:fs/promises';
import path from 'node:path';
import os from 'node:os';
import { execFileSync } from 'node:child_process';

/** 固定声明 catalog 从控制文件读取，绝不发现项目 node_modules 的运行时代码。 */
export async function typescriptConfiguration(root) {
  const control = JSON.parse(await fs.readFile(path.join(root, 'control/native.json'), 'utf8'));
  return { executable: path.join(root, process.platform === 'win32' ? 'bin/node.exe' : 'bin/node'), args: ['--disable-proto=throw', path.join(root, 'runtime/helper/main.mjs')], environment: {}, trusted_roots: [root], platforms: control.platforms.map(p => ({ ...p, root: path.join(root, p.root) })) };
}
/** 归档前后均验证真实跨 TS/JS 调用、标准声明与文件覆盖，不以 Node 启动作为验收。 */
export async function smokeTypeScript(root) {
  const config = await typescriptConfiguration(root);
  const scratch = await fs.mkdtemp(path.join(os.tmpdir(), 'narrant-ts-native-'));
  try {
    await fs.writeFile(path.join(scratch, 'main.ts'), "import {target} from './target.js'; export function caller() { target(); Promise.resolve(1); }");
    await fs.writeFile(path.join(scratch, 'target.js'), 'export function target() { return 1; }');
    const request = { protocol: 'narrant.native-analyzer/1', type: 'analyze', id: 'analysis-1', input: { repository_root: scratch, files: ['main.ts', 'target.js'], include_tests: false, profile: 'typescript-static-v1' }, environment: { scratch_root: scratch, platforms: config.platforms }, limits: { analysis_timeout_ms: 30000, max_result_bytes: 16777216, max_diagnostic_bytes: 262144 } };
    const lines = execFileSync(config.executable, config.args, { input: `${JSON.stringify(request)}\n`, encoding: 'utf8', timeout: 30000, env: { ...(process.platform === 'win32' ? { SystemRoot: process.env.SystemRoot } : {}) } }).trim().split('\n').map(line => JSON.parse(line));
    const [ready, result] = lines;
    if (ready.helper_id !== 'narrant-typescript' || result.outcome !== 'ok' || result.evidence.files.length !== 2 || result.graph.nodes.length !== result.evidence.origins.length) throw new Error(`TypeScript native graph smoke failed: ${JSON.stringify(result)}`);
    const caller = result.graph.nodes.find(n => n.name === 'caller'), target = result.graph.nodes.find(n => n.name === 'target');
    if (!caller?.references.some(r => r.semantic === 'code.calls' && r.target.id === target?.id) || !caller.references.some(r => r.target.kind === 'external') || target.language !== 'javascript') throw new Error('TypeScript native graph smoke lacks concrete cross-language/standard calls');
  } finally { await fs.rm(scratch, { recursive: true, force: true }); }
}
