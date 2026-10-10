import readline from 'node:readline';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { createRequire } from 'node:module';
import { analyze } from './analyze.mjs';

const protocol = 'narrant.native-analyzer/1';
/** stdout 专用于严格 NDJSON；诊断由公共 Host 的 stderr 限额与进程树负责。 */
function send(value) { process.stdout.write(`${JSON.stringify({ protocol, ...value })}\n`); }
send({ type: 'ready', helper_id: 'narrant-typescript', helper_version: '0.1.0', projection_revision: 'typescript-source-v1', languages: ['typescript', 'javascript'], profiles: ['typescript-static-v1'] });
const lines = readline.createInterface({ input: process.stdin, crlfDelay: Infinity });
let started = false;
for await (const line of lines) {
  let request;
  try {
    request = JSON.parse(line);
    if (request.protocol !== protocol || request.type !== 'analyze' || started) throw new Error('无效或重复分析请求');
    started = true;
    // 固定安装内模块；不使用项目 require、插件或外部服务器。同步工作期间取消由 Host ProcessTree 强制终止。
    const require = createRequire(import.meta.url);
    const ts = require(path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../typescript/lib/typescript.js'));
    const result = analyze(ts, request);
    send({ type: 'completed', id: request.id, outcome: 'ok', ...result });
  } catch (error) {
    send({ type: 'completed', id: request?.id ?? 'analysis-1', outcome: 'error', error: { code: 'object_extraction_failed', message: String(error.message), paths: [] } });
    process.stderr.write(`${error.stack}\n`);
  }
  lines.close(); break;
}
