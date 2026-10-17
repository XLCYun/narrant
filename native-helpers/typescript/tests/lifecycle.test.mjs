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
  if (control === 'cancel') {
    closeOnCompleted(state);
    state.child.stdin.write(`${JSON.stringify({ protocol, type: 'cancel', id: 'analysis-1' })}\n`);
  }
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

/** 与真实 Host 一样在读到完整终结的同一轮关闭 stdin，不等 helper 的写回调或轮询间隔。 */
function closeOnCompleted(state) {
  state.child.stdout.on('data', () => {
    if (state.messages.some(message => message.type === 'completed')) state.child.stdin.end();
  });
}

for (const outcome of ['ok', 'error']) test(`immediate Host EOF after completed(${outcome}) retains full flush and exit zero`, { timeout: 15000 }, async t => {
  const input = await request(t, outcome === 'ok' ? 'export const value = 1;' : 'return;');
  const state = await launch(t);
  closeOnCompleted(state);
  state.child.stdin.write(`${JSON.stringify(input)}\n`);
  await exited(state);
  assert.equal(state.messages.length, 2);
  assert.equal(state.messages[1].outcome, outcome);
  if (outcome === 'error') assert.ok(state.stderr.endsWith('\n'));
});

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

/** EOF 丢弃分支必须在恢复消费前退出；Windows 自终止报告原生退出码 1，普通终结仍要求 0。 */
async function abandonedExit(state, stream) {
  const exit = new Promise(resolve => state.child.once('exit', (code, signal) => resolve({ code, signal })));
  state.child.stdin.end();
  let stopped = false;
  void exit.then(() => { stopped = true; });
  await until(() => stopped, 5000);
  assert.deepEqual(await exit, { code: process.platform === 'win32' ? 1 : 0, signal: null }, state.stderr);
  stream.resume();
  await until(() => state.exited, 5000);
}

test('premature EOF also stops a completed graph blocked on stdout backpressure', { timeout: 25000 }, async t => {
  const state = await running(t);
  state.child.stdout.pause();
  // 真实大图开始写入，但消费端暂停，完整 NDJSON 不可能穿过有限 OS 管道。
  await until(() => state.child.stdout.readableLength > 0 || state.exited, 15000);
  assert.equal(state.exited, false, 'fixture must reach actual stdout backpressure before EOF');
  await delay(100);
  assert.equal(state.exited, false, 'helper must still be blocked on output before EOF');
  // close 事件须等读取结束；先观察 exit，确认解除背压并非 resume 所致。
  await abandonedExit(state, state.child.stdout);
  assert.equal(state.parseError, null);
  assert.equal(state.messages.length, 1, 'abandoned partial output must not become a successful terminal');
});

for (const control of ['drain', 'EOF']) test(`native error with stderr backpressure preserves failure on ${control}`, { timeout: 20000 }, async t => {
  const input = await request(t, 'export const value = 1;');
  // 真实文件系统拒绝过长路径；原生错误和堆栈都足够填满管道，不替换分析器。
  input.input.files = [`${'x'.repeat(512 * 1024)}.ts`];
  const state = await launch(t);
  state.child.stderr.pause();
  state.child.stdin.write(`${JSON.stringify(input)}\n`);
  await until(() => state.child.stderr.readableLength > 0 || state.exited, 10000);
  assert.equal(state.exited, false);
  await delay(100);
  assert.equal(state.exited, false, 'normal error must wait for diagnostic flush');
  assert.equal(state.messages.length, 1, 'diagnostics must flush before the Host can receive completed and close stdin');
  if (control === 'EOF') {
    await abandonedExit(state, state.child.stderr);
    assert.equal(state.messages.length, 1, 'preterminal EOF must abandon the pending terminal');
  }
  else {
    state.child.stdin.write(`${JSON.stringify({ protocol, type: 'cancel', id: input.id })}\n`);
    state.child.stderr.resume();
    await exited(state);
    assert.equal(state.messages.length, 2);
    assert.equal(state.messages[1]?.outcome, 'error', 'observed native failure must survive cancel during diagnostic flush');
    assert.equal(state.messages[1]?.error.code, 'object_extraction_failed');
    assert.equal(Object.hasOwn(state.messages[1], 'diagnostic'), false, 'worker diagnostic must not enter the protocol');
    assert.ok(Buffer.byteLength(state.stderr) > 512 * 1024);
    assert.ok(state.stderr.endsWith('\n'), 'diagnostic must flush completely');
  }
  assert.equal(state.parseError, null);
  assert.equal(state.pending, '');
});

test('stdout write failure exits nonzero after flushing its diagnostic', { timeout: 15000 }, async t => {
  const input = await request(t, 'export const value = 1;');
  const state = await launch(t);
  state.child.stdout.destroy();
  state.child.stdin.write(`${JSON.stringify(input)}\n`);
  await exited(state, 1);
  assert.equal(state.messages.length, 1);
  assert.match(state.stderr, /EPIPE|EIO|EBADF/);
});

test('stderr write failure retains the native error terminal and nonzero exit', { timeout: 15000 }, async t => {
  const input = await request(t, 'return;');
  const state = await launch(t);
  state.child.stderr.destroy();
  state.child.stdin.write(`${JSON.stringify(input)}\n`);
  await exited(state, 1);
  assert.equal(state.messages.length, 2);
  assert.equal(state.messages[1].outcome, 'error');
  assert.equal(state.messages[1].error.code, 'object_extraction_failed');
});
