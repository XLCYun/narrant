import assert from 'node:assert/strict';
import fs from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import { spawnSync } from 'node:child_process';
import { typescriptConfiguration } from '../../../scripts/typescript-native-smoke.mjs';

// 可复现的一次性 helper 样本；macOS time 同时提供完整子进程的墙钟与峰值 RSS。
if (process.platform !== 'darwin') throw new Error('此性能样本使用 macOS /usr/bin/time -l');
const pack = process.env.NARRANT_TYPESCRIPT_PACK;
if (!pack) throw new Error('NARRANT_TYPESCRIPT_PACK required');
const config = await typescriptConfiguration(path.resolve(pack));
const root = await fs.mkdtemp(path.join(os.tmpdir(), 'narrant-ts-benchmark-'));
try {
  const files = Array.from({ length: 40 }, (_, i) => `part${i}.ts`);
  await fs.writeFile(path.join(root, 'tsconfig.json'), JSON.stringify({ compilerOptions: { target: 'ES2022', module: 'Preserve', moduleResolution: 'Bundler', types: [] }, files }));
  for (let i = 0; i < files.length; i++) {
    const functions = Array.from({ length: 10 }, (_, j) => `export function f${i}_${j}() { return ${j ? `f${i}_${j - 1}()` : 'previous()'}; }`).join('\n');
    await fs.writeFile(path.join(root, files[i]), `import { f${(i + 39) % 40}_9 as previous } from './part${(i + 39) % 40}';\n${functions}`);
  }
  const request = { protocol: 'narrant.native-analyzer/1', type: 'analyze', id: 'analysis-1', input: { repository_root: root, files, include_tests: false, profile: 'typescript-static-v1' }, environment: { scratch_root: root, platforms: config.platforms }, limits: { analysis_timeout_ms: 30000, max_result_bytes: 16777216, max_diagnostic_bytes: 262144 } };
  for (let run = 1; run <= 3; run++) {
    const started = performance.now();
    const result = spawnSync('/usr/bin/time', ['-l', config.executable, ...config.args], { input: `${JSON.stringify(request)}\n`, encoding: 'utf8', env: {}, timeout: 30000, maxBuffer: 16777216 });
    assert.equal(result.status, 0, result.stderr);
    const completed = JSON.parse(result.stdout.trim().split('\n')[1]);
    assert.equal(completed.outcome, 'ok');
    assert.equal(completed.graph.nodes.length, 440);
    assert.equal(completed.graph.nodes.flatMap(n => n.references).filter(r => r.target.kind === 'node').length, 400);
    assert.equal(completed.evidence.files.length, 40);
    console.log(JSON.stringify({ run, files: 40, nodes: 440, resolved_relations: 400, wall_ms: Math.round(performance.now() - started), peak_rss_bytes: Number(result.stderr.match(/(\d+)\s+maximum resident set size/)[1]) }));
  }
} finally { await fs.rm(root, { recursive: true, force: true }); }
