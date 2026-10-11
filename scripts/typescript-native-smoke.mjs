import fs from 'node:fs/promises';
import path from 'node:path';
import os from 'node:os';
import { spawn } from 'node:child_process';

/** 固定声明 catalog 从控制文件读取，绝不发现项目 node_modules 的运行时代码。 */
export async function typescriptConfiguration(root) {
  const control = JSON.parse(await fs.readFile(path.join(root, 'control/native.json'), 'utf8'));
  return { executable: path.join(root, process.platform === 'win32' ? 'bin/node.exe' : 'bin/node'), args: ['--disable-proto=throw', path.join(root, 'runtime/helper/main.mjs')], environment: {}, trusted_roots: [root], platforms: control.platforms.map(p => ({ ...p, root: path.join(root, p.root) })) };
}
/** 保持控制管道直到 completed；完整终结、输出 EOF 和 exit 0 缺一不可。 */
export async function exchangeTypeScript(config, request) {
  const child = spawn(config.executable, config.args, { env: process.platform === 'win32' ? { SystemRoot: process.env.SystemRoot } : {}, stdio: ['pipe', 'pipe', 'pipe'] });
  let stdout = '', stderr = '', pending = '', parseError;
  const messages = [];
  child.stdout.setEncoding('utf8'); child.stderr.setEncoding('utf8');
  child.stdout.on('data', text => {
    stdout += text; pending += text;
    let boundary;
    while ((boundary = pending.indexOf('\n')) >= 0) {
      const line = pending.slice(0, boundary); pending = pending.slice(boundary + 1);
      try {
        const message = JSON.parse(line); messages.push(message);
        if (message.type === 'completed') child.stdin.end();
      } catch (error) { parseError = error; child.stdin.end(); child.kill(); }
    }
  });
  child.stderr.on('data', text => { stderr += text; });
  child.stdin.on('error', error => { parseError ??= error; });
  const timeout = setTimeout(() => { parseError = new Error('TypeScript helper timed out'); child.stdin.end(); child.kill(); }, 30000);
  try {
    const exited = new Promise((resolve, reject) => { child.once('error', reject); child.once('close', (code, signal) => resolve({ code, signal })); });
    child.stdin.write(`${JSON.stringify(request)}\n`);
    const { code, signal } = await exited;
    if (parseError) throw parseError;
    if (code !== 0 || signal || pending || messages.length !== 2 || messages[0].type !== 'ready' || messages[1].type !== 'completed' || messages.some(message => message.protocol !== request.protocol) || messages[1].id !== request.id) throw new Error(`Incomplete TypeScript helper exchange: ${code}/${signal} ${stderr}`);
    return { messages, stdout, stderr };
  } finally { clearTimeout(timeout); }
}
/** 归档前后均验证真实跨 TS/JS 调用、标准声明与文件覆盖，不以 Node 启动作为验收。 */
export async function smokeTypeScript(root) {
  const config = await typescriptConfiguration(root);
  const scratch = await fs.mkdtemp(path.join(os.tmpdir(), 'narrant-ts-native-'));
  try {
    await fs.writeFile(path.join(scratch, 'main.ts'), "import {target} from './target.js'; export function caller() { target(); Promise.resolve(1); }");
    await fs.writeFile(path.join(scratch, 'target.js'), 'export function target() { return 1; }');
    const request = { protocol: 'narrant.native-analyzer/1', type: 'analyze', id: 'analysis-1', input: { repository_root: scratch, files: ['main.ts', 'target.js'], include_tests: false, profile: 'typescript-static-v1' }, environment: { scratch_root: scratch, platforms: config.platforms }, limits: { analysis_timeout_ms: 30000, max_result_bytes: 16777216, max_diagnostic_bytes: 262144 } };
    const { messages: lines } = await exchangeTypeScript(config, request);
    const [ready, result] = lines;
    if (ready.helper_id !== 'narrant-typescript' || result.outcome !== 'ok' || result.evidence.files.length !== 2 || result.graph.nodes.length !== result.evidence.origins.length) throw new Error(`TypeScript native graph smoke failed: ${JSON.stringify(result)}`);
    const caller = result.graph.nodes.find(n => n.name === 'caller'), target = result.graph.nodes.find(n => n.name === 'target');
    if (!caller?.references.some(r => r.semantic === 'code.calls' && r.target.id === target?.id) || !caller.references.some(r => r.target.kind === 'external') || target.language !== 'javascript') throw new Error('TypeScript native graph smoke lacks concrete cross-language/standard calls');
  } finally { await fs.rm(scratch, { recursive: true, force: true }); }
}
