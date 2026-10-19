import assert from 'node:assert/strict';
import fs from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import { spawn } from 'node:child_process';
import { test } from 'node:test';
import { typescriptConfiguration } from '../../../scripts/language-packs/typescript-native-smoke.mjs';

const protocol = 'narrant.native-analyzer/1';
const config = await typescriptConfiguration(path.resolve(process.env.NARRANT_TYPESCRIPT_PACK));

/** 真正执行支持包入口；stdin 保持打开直到 helper 自己退出，不伪造生命周期。 */
async function exchange(t, lines) {
  const child = spawn(config.executable, config.args, { env: process.platform === 'win32' ? { SystemRoot: process.env.SystemRoot } : {}, stdio: ['pipe', 'pipe', 'pipe'] });
  let stdout = '', stderr = '';
  child.stdout.on('data', bytes => { stdout += bytes; });
  child.stderr.on('data', bytes => { stderr += bytes; });
  child.stdin.on('error', () => {});
  const timer = setTimeout(() => child.kill('SIGKILL'), 10000);
  t.after(() => clearTimeout(timer));
  const exit = new Promise((resolve, reject) => { child.once('error', reject); child.once('close', (code, signal) => resolve({ code, signal })); });
  child.stdin.write(`${lines.join('\n')}\n`);
  const result = await exit;
  clearTimeout(timer);
  return { ...result, stderr, messages: stdout.trim().split('\n').map(line => JSON.parse(line)) };
}

/** 完整输入使用真实固定声明及临时仓库；空选集仍是有效 analyze。 */
async function input(t) {
  const root = await fs.mkdtemp(path.join(os.tmpdir(), 'ts-protocol-'));
  t.after(() => fs.rm(root, { recursive: true, force: true }));
  return { protocol, type: 'analyze', id: 'review', input: { repository_root: root, files: [], include_tests: false, profile: 'typescript-static-v1' }, environment: { scratch_root: root, platforms: config.platforms }, limits: { analysis_timeout_ms: 30000, max_result_bytes: 16777216, max_diagnostic_bytes: 262144 } };
}

for (const control of [false, true]) test(`real protocol accepts valid ${control ? 'cancel' : 'analyze'}`, async t => {
  const request = await input(t);
  const lines = [JSON.stringify(request)];
  if (control) lines.push(JSON.stringify({ protocol, type: 'cancel', id: request.id }));
  const result = await exchange(t, lines);
  assert.equal(result.code, 0, result.stderr);
  assert.equal(result.signal, null);
  assert.equal(result.stderr, '');
  assert.equal(result.messages.length, 2);
  assert.equal(result.messages[1].id, 'review');
  assert.equal(result.messages[1].outcome, control ? 'cancelled' : 'ok');
});

// 每层 required/unknown/type/enum 都经真实入口拒绝，包含审查报告的全部触发输入。
const mutations = [
  ['unknown envelope', r => { r.extra = true; }],
  ['missing protocol', r => { delete r.protocol; }], ['missing type', r => { delete r.type; }],
  ['numeric boolean', r => { r.input.include_tests = 0; }], ['wrong package type', r => { r.environment.platforms[0].metadata.package = 1; }],
  ['missing id', r => { delete r.id; }], ['wrong id', r => { r.id = 1; }], ['empty id', r => { r.id = ''; }],
  ['wrong protocol', r => { r.protocol = 'other'; }], ['unknown type', r => { r.type = 'batch'; }],
  ['missing input', r => { delete r.input; }], ['null input', r => { r.input = null; }],
  ['string boolean', r => { r.input.include_tests = 'false'; }], ['unknown profile', r => { r.input.profile = 'other'; }],
  ['wrong repository', r => { r.input.repository_root = 1; }], ['wrong files', r => { r.input.files = ''; }], ['wrong file member', r => { r.input.files = [1]; }],
  ['missing environment', r => { delete r.environment; }], ['array environment', r => { r.environment = []; }],
  ['wrong scratch', r => { r.environment.scratch_root = false; }], ['wrong platforms', r => { r.environment.platforms = {}; }],
  ['null platform', r => { r.environment.platforms = [null]; }], ['unknown platform kind', r => { r.environment.platforms[0].kind = 'sdk'; }],
  ['wrong platform id', r => { r.environment.platforms[0].id = 1; }], ['wrong platform version', r => { r.environment.platforms[0].version = 1; }],
  ['wrong platform root', r => { r.environment.platforms[0].root = false; }], ['array metadata', r => { r.environment.platforms[0].metadata = []; }],
  ['unknown package', r => { r.environment.platforms[0].metadata.package = 'business'; }],
  ['missing limits', r => { delete r.limits; }], ['array limits', r => { r.limits = []; }],
];
for (const [object, keys] of [['input', ['repository_root', 'files', 'include_tests', 'profile']], ['environment', ['scratch_root', 'platforms']], ['limits', ['analysis_timeout_ms', 'max_result_bytes', 'max_diagnostic_bytes']]]) {
  mutations.push([`unknown ${object}`, r => { r[object].extra = true; }]);
  for (const key of keys) mutations.push([`missing ${object}.${key}`, r => { delete r[object][key]; }]);
}
for (const key of ['id', 'kind', 'version', 'root', 'metadata']) mutations.push([`missing platform.${key}`, r => { delete r.environment.platforms[0][key]; }]);
mutations.push(['unknown platform', r => { r.environment.platforms[0].extra = 1; }], ['missing package', r => { delete r.environment.platforms[0].metadata.package; }], ['unknown metadata', r => { r.environment.platforms[0].metadata.extra = 1; }]);
for (const key of ['analysis_timeout_ms', 'max_result_bytes', 'max_diagnostic_bytes']) for (const value of ['1', 0, -1, 1.5, null]) mutations.push([`wrong limit ${key}=${value}`, r => { r.limits[key] = value; }]);
for (const [name, mutate] of mutations) test(`real protocol rejects ${name}`, async t => {
  const request = structuredClone(await input(t));
  mutate(request);
  const result = await exchange(t, [JSON.stringify(request)]);
  assert.equal(result.code, 1, result.stderr);
  assert.equal(result.signal, null);
  assert.equal(result.messages.length, 1, 'invalid analyze cannot fabricate completed');
  assert.ok(result.stderr.length > 0);
});

for (const [name, replace] of [
  ['id', s => s.replace('"id":"review"', '"id":"discarded","id":"review"')],
  ['escaped id', s => s.replace('"id":"review"', '"i\\u0064":"discarded","id":"review"')],
  ['input', s => s.replace('"include_tests":false', '"include_tests":true,"include_tests":false')],
  ['environment', s => s.replace('"platforms":', '"platforms":[],"platforms":')],
  ['platform', s => s.replace('"kind":"typescript-declarations"', '"kind":"other","kind":"typescript-declarations"')],
  ['metadata', s => s.replace('"package":"typescript"', '"package":"business","package":"typescript"')],
  ['limits', s => s.replace('"max_result_bytes":16777216', '"max_result_bytes":1,"max_result_bytes":16777216')],
]) test(`real protocol rejects duplicate ${name}`, async t => {
  const result = await exchange(t, [replace(JSON.stringify(await input(t)))]);
  assert.equal(result.code, 1);
  assert.equal(result.messages.length, 1);
  assert.match(result.stderr, /重复 JSON 键/);
});

for (const [name, control] of [
  ['unknown field', JSON.stringify({ protocol, type: 'cancel', id: 'review', extra: true })],
  ['missing id', JSON.stringify({ protocol, type: 'cancel' })],
  ['wrong id type', JSON.stringify({ protocol, type: 'cancel', id: 1 })],
  ['duplicate id', `{"protocol":"${protocol}","type":"cancel","id":"wrong","id":"review"}`],
  ['wrong protocol', JSON.stringify({ protocol: 'other', type: 'cancel', id: 'review' })],
]) test(`real protocol rejects cancel ${name}`, async t => {
  const result = await exchange(t, [JSON.stringify(await input(t)), control]);
  assert.equal(result.code, 1, result.stderr);
  assert.equal(result.signal, null);
  assert.equal(result.messages.length, 1, 'invalid cancel cannot become successful or cancelled');
});
