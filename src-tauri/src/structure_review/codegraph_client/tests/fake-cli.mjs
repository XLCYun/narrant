// 只模拟公开协议与进程树；不包含语言 helper 实现。
import { fileURLToPath } from 'node:url';
import readline from 'node:readline';
import fs from 'node:fs';
import { spawn } from 'node:child_process';
const scenario = process.env.CODEGRAPH_TEST_SCENARIO;
if (process.argv[2] === 'descendant') {
  const child = spawn(process.execPath, ['-e', 'setInterval(()=>{},1000)'], { detached: true, stdio: 'ignore' });
  fs.writeFileSync(process.env.CODEGRAPH_TEST_PIDS, `${process.pid}\n${child.pid}`);
  setInterval(() => {}, 1000);
} else {
  const pack = process.argv[process.argv.indexOf('--pack') + 1];
  const protocol = 'codegraph.analyzer/1';
  const emit = value => process.stdout.write(`${JSON.stringify(value)}\n`);
  if (scenario === 'silent') setInterval(() => {}, 1000);
  else {
    emit({ protocol: scenario === 'protocol' ? 'codegraph.analyzer/2' : protocol, type: 'ready', engine_version: scenario === 'version' ? '9.0.0' : '0.1.0', pack_id: scenario === 'pack' ? 'python' : pack });
    if (scenario === 'unread-input') { process.stdin.pause(); setInterval(() => {}, 1000); }
    else {
    const lines = readline.createInterface({ input: process.stdin });
    lines.on('line', line => {
      const request = JSON.parse(line);
      if (request.type === 'cancel') {
        if (scenario === 'cooperate') { emit({ protocol, type: 'completed', id: request.id, outcome: 'cancelled' }); process.exitCode = 130; lines.close(); process.stdin.destroy(); }
        return;
      }
      if (scenario === 'nested' || scenario === 'outer-death') {
        spawn(process.execPath, [fileURLToPath(import.meta.url), 'descendant'], { detached: true, stdio: 'ignore', env: process.env });
        if (scenario === 'outer-death') setTimeout(() => process.exit(1), 350);
        return;
      }
      if (scenario === 'cooperate') return;
      const graph = { root_contains: ['typescript:run'], nodes: [{ category: 'callable', id: 'typescript:run', language: 'typescript', native_kind: 'function', name: 'run', signature: 'run()', code: 'return 1;', source: 'function run(){return 1;}', contains: [], references: [], extensions: { 'typescript.custom': { preserved: true } } }], external_symbols: [] };
      const result = { protocol, type: 'completed', id: scenario === 'id' ? 'other' : request.id, outcome: 'ok', graph };
      if (scenario === 'error' || scenario === 'error-cancel' || scenario === 'error-null') { delete result.graph; result.outcome = 'error'; result.error = { code: 'analysis_failed', message: 'fixture failure' }; }
      if (scenario === 'error-null') result.id = null;
      if (scenario === 'cancelled') { delete result.graph; result.outcome = 'cancelled'; }
      if (scenario === 'graph') graph.root_contains = ['missing'];
      if (scenario === 'missing') delete graph.nodes[0].source;
      if (scenario === 'unexpected') result.private_config = {};
      if (scenario === 'stderr') process.stderr.write('x'.repeat(4096));
      if (scenario === 'oversize') process.stdout.write('x'.repeat(4096));
      else if (scenario === 'truncated') process.stdout.write(JSON.stringify(result));
      else if (scenario === 'duplicate') process.stdout.write(JSON.stringify(result).replace('"outcome":"ok"', '"outcome":"ok","outcome":"ok"') + '\n');
      else emit(result);
      if (scenario === 'extra') emit(result);
      if (scenario === 'error-cancel') { setTimeout(() => process.exit(1), 350); return; }
      process.exitCode = scenario === 'exit' || scenario === 'error' || scenario === 'error-cancel' ? 1 : 0;
      lines.close(); process.stdin.destroy();
    });
    }
  }
}
