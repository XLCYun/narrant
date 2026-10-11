import assert from 'node:assert/strict';
import fs from 'node:fs/promises';
import path from 'node:path';
import os from 'node:os';
import { spawn, execFile } from 'node:child_process';
import { promisify } from 'node:util';
import { setTimeout as delay } from 'node:timers/promises';
import { test } from 'node:test';
import { typescriptConfiguration } from '../../../scripts/typescript-native-smoke.mjs';

const protocol = 'narrant.native-analyzer/1';
if (!process.env.NARRANT_TYPESCRIPT_PACK) throw new Error('NARRANT_TYPESCRIPT_PACK required');
const config = await typescriptConfiguration(path.resolve(process.env.NARRANT_TYPESCRIPT_PACK));
const execute = promisify(execFile);

/** 测试真实安装入口；kill 只在断言失败后的清理阶段兜底，不计入自停证据。 */
async function launch(t, configuration = config) {
  const child = spawn(configuration.executable, configuration.args, { env: process.platform === 'win32' ? { SystemRoot: process.env.SystemRoot } : {}, stdio: ['pipe', 'pipe', 'pipe'] });
  const state = { child, messages: [], stdout: '', stderr: '', exited: false, pending: '', parseError: null };
  child.stdout.setEncoding('utf8'); child.stderr.setEncoding('utf8');
  child.stdout.on('data', text => {
    state.stdout += text; state.pending += text;
    let boundary;
    while ((boundary = state.pending.indexOf('\n')) >= 0) {
      const line = state.pending.slice(0, boundary); state.pending = state.pending.slice(boundary + 1);
      try { state.messages.push(JSON.parse(line)); } catch (error) { state.parseError = error; }
    }
  });
  child.stderr.on('data', text => { state.stderr += text; });
  child.stdin.on('error', () => {});
  state.closed = new Promise((resolve, reject) => { child.once('error', reject); child.once('close', (code, signal) => { state.exited = true; resolve({ code, signal }); }); });
  t.after(async () => { if (!state.exited) { child.kill('SIGKILL'); await state.closed; } });
  await until(() => state.messages.length || state.exited, 10000);
  assert.equal(state.messages[0]?.type, 'ready', state.stderr);
  return state;
}

/** 使用实际源码和声明 catalog，不替换 analyzer 或添加生产测试钩子。 */
async function request(t, source) {
  const root = await fs.mkdtemp(path.join(os.tmpdir(), 'ts-lifecycle-'));
  t.after(() => fs.rm(root, { recursive: true, force: true }));
  await fs.writeFile(path.join(root, 'main.ts'), source);
  return { protocol, type: 'analyze', id: 'analysis-1', input: { repository_root: root, files: ['main.ts'], include_tests: false, profile: 'typescript-static-v1' }, environment: { scratch_root: root, platforms: config.platforms }, limits: { analysis_timeout_ms: 30000, max_result_bytes: 67108864, max_diagnostic_bytes: 262144 } };
}

/** 有限轮询只检查观察值；退出宽限内绝不调用 kill。 */
async function until(predicate, milliseconds) {
  const deadline = performance.now() + milliseconds;
  while (!await predicate()) { assert.ok(performance.now() < deadline, 'helper did not reach lifecycle boundary'); await delay(25); }
}

/** OS CPU 累计时间证明实际 helper 正在消耗分析计算，而非仅完成 spawn/ready。 */
async function cpuMilliseconds(pid) {
  if (process.platform === 'win32') {
    const { stdout } = await execute('powershell.exe', ['-NoProfile', '-Command', `(Get-Process -Id ${pid}).TotalProcessorTime.TotalMilliseconds`]);
    return Number(stdout.trim());
  }
  const { stdout } = await execute('/bin/ps', ['-p', String(pid), '-o', 'time=']);
  return stdout.trim().split(':').reduce((total, part) => total * 60 + Number(part), 0) * 1000;
}

/** 与原有大源码取消夹具同类，等待真实 CPU 活动且尚无 completed 后再施加控制。 */
async function running(t) {
  const input = await request(t, '');
  input.input.files = Array.from({ length: 40 }, (_, i) => `part${i}.ts`);
  await fs.writeFile(path.join(input.input.repository_root, 'tsconfig.json'), JSON.stringify({ compilerOptions: { target: 'ES2022', module: 'Preserve', moduleResolution: 'Bundler', types: [] }, files: input.input.files }));
  for (const [i, file] of input.input.files.entries()) {
    const source = Array.from({ length: 100 }, (_, j) => `export function f${i}_${j}(){return ${j ? `f${i}_${j - 1}()` : '1'}}`).join('\n');
    await fs.writeFile(path.join(input.input.repository_root, file), source);
  }
  const state = await launch(t);
  state.child.stdin.write(`${JSON.stringify(input)}\n`);
  let cpu = 0;
  await until(async () => { assert.equal(state.exited, false, state.stderr); cpu = await cpuMilliseconds(state.child.pid); return cpu >= 500; }, 15000);
  assert.equal(state.messages.length, 1, 'analysis completed before the running cancellation boundary');
  t.diagnostic(`real helper CPU before control: ${Math.round(cpu)}ms`);
  return state;
}

for (const control of ['cancel', 'EOF']) test(`actual executable stops running analysis on ${control}`, { timeout: 25000 }, async t => {
  const state = await running(t);
  if (control === 'cancel') state.child.stdin.write(`${JSON.stringify({ protocol, type: 'cancel', id: 'analysis-1' })}\n`);
  else state.child.stdin.end();
  await exited(state);
  if (control === 'cancel') {
    assert.deepEqual(state.messages.map(message => message.type === 'completed' ? message.outcome : message.type), ['ready', 'cancelled']);
    assert.deepEqual(state.messages[1], { protocol, type: 'completed', id: 'analysis-1', outcome: 'cancelled' });
  } else assert.equal(state.messages.length, 1, 'premature EOF must not produce a successful graph');
});

/** 无 kill 的退出屏障同时验证完整行与正常退出；stdin 可以保持打开。 */
async function exited(state, code = 0) {
  await until(() => state.exited, 5000);
  assert.deepEqual(await state.closed, { code, signal: null }, state.stderr);
  assert.equal(state.parseError, null);
  assert.equal(state.pending, '', 'stdout must finish on an NDJSON boundary');
  assert.equal(state.messages.filter(message => message.type === 'ready').length, 1);
  assert.ok(state.messages.filter(message => message.type === 'completed').length <= 1);
  for (const message of state.messages) assert.equal(message.protocol, protocol);
}

test('actual executable flushes one nonempty terminal and exits with stdin still open', { timeout: 15000 }, async t => {
  const input = await request(t, 'export function caller(){return target()} export function target(){return 1}');
  const state = await launch(t);
  state.child.stdin.write(`${JSON.stringify(input)}\n`);
  await exited(state);
  assert.equal(state.messages.length, 2);
  const result = state.messages[1];
  assert.equal(result.id, input.id);
  assert.equal(result.outcome, 'ok');
  const target = result.graph.nodes.find(node => node.name === 'target');
  assert.ok(target);
  assert.ok(result.graph.nodes.find(node => node.name === 'caller')?.references.some(reference => reference.target.id === target.id));
  // 普通终结后的 stdin EOF 不得再生成错误或第二条 completed。
  state.child.stdin.end();
});

test('actual native extraction error survives a later cancel and ordinary EOF', { timeout: 15000 }, async t => {
  const input = await request(t, 'return;');
  const state = await launch(t);
  state.child.stdin.write(`${JSON.stringify(input)}\n`);
  await until(() => state.messages.length > 1 || state.exited, 10000);
  assert.equal(state.messages[1]?.outcome, 'error', state.stderr);
  state.child.stdin.end(`${JSON.stringify({ protocol, type: 'cancel', id: input.id })}\n`);
  await exited(state);
  assert.equal(state.messages.length, 2);
  assert.equal(state.messages[1].error.code, 'object_extraction_failed');
  assert.match(state.messages[1].error.message, /源码无法解析/);
});

test('worker startup failure is a real error rather than cancellation', { timeout: 15000 }, async t => {
  const input = await request(t, 'export const value = 1;');
  // 使用真实 Node 权限拒绝线程创建；没有假 analyzer、额外入口或生产测试钩子。
  const state = await launch(t, { ...config, args: ['--permission', '--allow-fs-read=*', ...config.args] });
  state.child.stdin.write(`${JSON.stringify(input)}\n${JSON.stringify({ protocol, type: 'cancel', id: input.id })}\n`);
  await exited(state);
  assert.equal(state.messages.length, 2);
  assert.equal(state.messages[1].outcome, 'error');
  assert.match(state.messages[1].error.message, /Access to this API has been restricted/);
});

test('malformed running control takes priority over queued cancel', { timeout: 25000 }, async t => {
  const state = await running(t);
  state.child.stdin.write(`{malformed\n${JSON.stringify({ protocol, type: 'cancel', id: 'analysis-1' })}\n`);
  await exited(state, 1);
  assert.equal(state.messages.length, 1);
  assert.match(state.stderr, /SyntaxError/);
});

for (const control of [{ protocol, type: 'cancel', id: 'wrong-id' }, { protocol, type: 'analyze', id: 'analysis-1' }]) test(`invalid ${control.type} control cannot produce cancelled`, { timeout: 15000 }, async t => {
  const input = await request(t, 'export const value = 1;');
  const state = await launch(t);
  state.child.stdin.write(`${JSON.stringify(input)}\n${JSON.stringify(control)}\n${JSON.stringify({ protocol, type: 'cancel', id: input.id })}\n`);
  await exited(state, 1);
  assert.equal(state.messages.length, 1);
  assert.match(state.stderr, /无效或重复分析控制/);
});

test('invalid initial request exits without fabricating a terminal ID', { timeout: 15000 }, async t => {
  const state = await launch(t);
  state.child.stdin.write(`${JSON.stringify({ protocol, type: 'cancel', id: 'analysis-1' })}\n`);
  await exited(state, 1);
  assert.equal(state.messages.length, 1);
});

test('premature EOF also stops a completed graph blocked on stdout backpressure', { timeout: 25000 }, async t => {
  const state = await running(t);
  state.child.stdout.pause();
  // 真实大图开始写入，但消费端暂停，完整 NDJSON 不可能穿过有限 OS 管道。
  await until(() => state.child.stdout.readableLength > 0 || state.exited, 15000);
  assert.equal(state.exited, false, 'fixture must reach actual stdout backpressure before EOF');
  await delay(100);
  assert.equal(state.exited, false, 'helper must still be blocked on output before EOF');
  state.child.stdin.end();
  // close 事件须等读取结束；先观察 exit，确认解除背压并非 resume 所致。
  const exit = new Promise(resolve => state.child.once('exit', (code, signal) => resolve({ code, signal })));
  let stopped = false;
  void exit.then(() => { stopped = true; });
  await until(() => stopped, 5000);
  assert.deepEqual(await exit, { code: 0, signal: null }, state.stderr);
  state.child.stdout.resume();
  await until(() => state.exited, 5000);
  assert.equal(state.parseError, null);
  assert.equal(state.messages.length, 1, 'abandoned partial output must not become a successful terminal');
});
