// 只模拟一次性公共消息与进程树，不包含语言分析实现。
import { fileURLToPath } from 'node:url';
import fs from 'node:fs';
import { spawn } from 'node:child_process';
const scenario = process.env.CODEGRAPH_TEST_SCENARIO;
if (process.argv[2] === 'descendant') {
  const child = spawn(process.execPath, ['-e', 'setInterval(()=>{},1000)'], { detached: true, stdio: 'ignore' });
  const marker = process.env.CODEGRAPH_TEST_PIDS;
  fs.writeFileSync(`${marker}.tmp`, `${process.pid}\n${child.pid}`);
  fs.renameSync(`${marker}.tmp`, marker);
  setInterval(() => {}, 1000);
} else {
  if (JSON.stringify(process.argv.slice(2)) !== JSON.stringify(['analyze'])) throw new Error('Public analyzer accepts only analyze');
  if (process.env.CODEGRAPH_TEST_SCRATCH) fs.writeFileSync(process.env.CODEGRAPH_TEST_SCRATCH, process.cwd());
  const protocol = 'codegraph.analyzer/1';
  const emit = value => process.stdout.write(`${JSON.stringify(value)}\n`);
  if (scenario === 'silent' || scenario === 'unread-input') {
    process.stdin.pause();
    setInterval(() => {}, 1000);
  } else {
    const chunks = [];
    process.stdin.on('data', chunk => chunks.push(chunk));
    // 必须读到正常 EOF 才能分析；旧 ready 或行模式不能满足此夹具。
    process.stdin.on('end', () => {
      const request = JSON.parse(Buffer.concat(chunks).toString('utf8'));
      const graph = { root_contains: ['typescript:run'], nodes: [{ category: 'callable', id: 'typescript:run', language: 'typescript', native_kind: 'function', name: 'run', signature: 'run()', code: 'return 1;', source: 'function run(){return 1;}', contains: [], references: [], extensions: { 'typescript.custom': { preserved: true } } }], external_symbols: [] };
      const result = { protocol: scenario === 'protocol' ? 'codegraph.analyzer/2' : protocol, engine_version: scenario === 'version' ? '9.0.0' : '0.1.1', pack_id: scenario === 'pack' ? 'python' : 'typescript-javascript', type: 'completed', id: scenario === 'id' ? 'other' : request.id, outcome: 'ok', graph };
      if (scenario === 'hang-after-input') { setInterval(() => {}, 1000); return; }
      if (['nested','outer-death','normal-orphan','normal-held-pipe','exit-held-pipe','abnormal-held-pipe'].includes(scenario)) {
        spawn(process.execPath, [fileURLToPath(import.meta.url), 'descendant'], { detached: true, stdio: scenario.includes('held-pipe') ? ['ignore','inherit','inherit'] : 'ignore', env: process.env });
        if (scenario === 'nested') { setInterval(() => {}, 1000); return; }
        // 保存后代身份后才放行根退出，避免把已消失的 PID 当成清理证据。
        const handshake = setInterval(() => {
          if (!fs.existsSync(process.env.CODEGRAPH_TEST_SAVED)) return;
          clearInterval(handshake);
          setTimeout(() => {
            if (scenario.startsWith('normal-')) emit(result);
            process.exit(['outer-death','abnormal-held-pipe'].includes(scenario) ? 1 : 0);
          }, 50);
        }, 5);
        return;
      }
      if (scenario.startsWith('error')) {
        delete result.graph;
        result.outcome = 'error';
        result.error = { code: ['error-null','error-request-id'].includes(scenario) ? 'invalid_request' : scenario === 'error-protocol-null' ? 'incompatible_protocol' : 'analysis_failed', message: 'fixture failure' };
        if (['error-null','error-protocol-null','error-analysis-null'].includes(scenario)) result.id = null;
        if (scenario === 'error-id') result.id = 'other';
        if (scenario === 'error-graph') result.graph = graph;
        if (scenario === 'error-version') result.engine_version = '9.0.0';
      }
      if (scenario === 'ready') { emit({ protocol, type: 'ready', engine_version: '0.1.1', pack_id: 'typescript-javascript' }); return; }
      if (scenario === 'cancelled') { delete result.graph; result.outcome = 'cancelled'; }
      if (scenario === 'graph') graph.root_contains = ['missing'];
      if (scenario === 'missing') delete graph.nodes[0].source;
      if (scenario === 'unexpected') result.private_config = {};
      if (scenario === 'stderr') process.stderr.write('x'.repeat(4096));
      if (scenario === 'empty-diagnostic') { process.stderr.end('fixture diagnostic', () => setTimeout(() => process.exit(0), 50)); return; }
      if (scenario === 'empty' || scenario === 'crash') { process.exitCode = scenario === 'crash' ? 1 : 0; return; }
      if (scenario === 'oversize') process.stdout.write('x'.repeat(4096));
      else if (scenario === 'truncated') process.stdout.write(JSON.stringify(result));
      else if (scenario === 'utf8') process.stdout.write(Buffer.from([0xff, 0x0a]));
      else if (scenario === 'duplicate') process.stdout.write(JSON.stringify(result).replace('"outcome":"ok"', '"outcome":"ok","outcome":"ok"') + '\n');
      else if (scenario === 'nested-duplicate') process.stdout.write(JSON.stringify(result).replace('"preserved":true', '"preserved":true,"preserved":false') + '\n');
      else emit(result);
      if (scenario === 'extra') emit(result);
      if (scenario === 'error-cancel') { setTimeout(() => process.exit(1), 350); return; }
      process.exitCode = scenario === 'exit' || scenario.startsWith('error') ? 1 : 0;
    });
  }
}
