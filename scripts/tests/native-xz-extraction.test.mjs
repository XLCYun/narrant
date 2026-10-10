import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import { spawn, execFileSync } from 'node:child_process';
import { PassThrough } from 'node:stream';
import { EventEmitter } from 'node:events';
import { extractTarXz } from '../native-xz-extraction.mjs';

/** 使用实际 Node 子进程模拟解码器与解包器，验证生产连接与退出等待。 */
async function fixture(t, producerCode, consumerCode) {
  const root = await fs.mkdtemp(path.join(os.tmpdir(), 'native xz 测试-'));
  t.after(() => fs.rm(root, { recursive: true, force: true }));
  const children = [], commands = [];
  return { root, children, commands, run: options => extractTarXz({ archive: path.join(root, 'sdk.tar.xz'), destination: path.join(root, 'unpacked'), sevenZip: process.execPath,
    spawnProcess: (executable, args, settings) => {
      commands.push({ executable, args, settings });
      const child = spawn(process.execPath, ['-e', args.includes('-ttar') ? consumerCode : producerCode, root], settings);
      const state = { child, closed: false }; children.push(state);
      child.once('close', () => { state.closed = true; });
      return child;
    }, timeoutMs: 10_000, reapMs: 2_000, ...options }) };
}

/** 大于管道缓冲的二进制数据必须原样传递，同时消费大量诊断输出。 */
test('XZ pipeline preserves bytes and waits for both real processes to close', async t => {
  const f = await fixture(t, `process.stderr.write('x'.repeat(200000)); process.stdout.write(Buffer.from(Array.from({length:2097152},(_,i)=>i%256)));`, `const b=[]; process.stdin.on('data',x=>b.push(x)); process.stdin.on('end',()=>{require('fs').writeFileSync(require('path').join(process.argv[1],'received'),Buffer.concat(b));process.stdout.write('log'.repeat(100000));});`);
  await f.run();
  assert.equal(f.children.every(state => state.closed), true);
  const bytes = await fs.readFile(path.join(f.root, 'received'));
  assert.equal(bytes.length, 2097152);
  assert.equal(bytes.every((value, i) => value === i % 256), true);
  assert.deepEqual((await fs.readdir(f.root)).sort(), ['received'], 'must not create an intermediate tar');
  assert.ok(f.commands.every(command => command.settings.shell === false));
  assert.deepEqual(f.commands[0].args, ['x', '-ttar', '-si', `-o${path.join(f.root, 'unpacked')}`, '-y']);
  assert.deepEqual(f.commands[1].args, ['x', '-txz', '-so', path.join(f.root, 'sdk.tar.xz')]);
});

/** 任一解包器失败后，另一端不能残留或把管道 EOF 误判为成功。 */
for (const side of ['XZ', 'TAR']) test(`${side} failure reaps the other real child before rejecting`, async t => {
  const failing = `process.stderr.write('decoder failure');process.exit(2)`;
  const hanging = `process.stdin.resume();setInterval(()=>{},1000)`;
  const f = await fixture(t, side === 'XZ' ? failing : hanging, side === 'TAR' ? failing : hanging);
  await assert.rejects(f.run(), /failed|premature|EPIPE/);
  assert.equal(f.children.every(state => state.closed), true);
});

/** 超时必须在有限时间内终止两端，不能复现 CI 等待整个 job deadline 的症状。 */
test('hung extraction hits its deadline and reaps both children', async t => {
  const f = await fixture(t, 'setInterval(()=>{},1000)', 'process.stdin.resume();setInterval(()=>{},1000)');
  const start = Date.now();
  await assert.rejects(f.run({ timeoutMs: 200 }), /timed out/);
  assert.equal(f.children.every(state => state.closed), true);
  assert.ok(Date.now() - start < 15_000, 'bounded tree termination must not wait for the job deadline');
});

/** 第二次 spawn 失败时也回收已经启动的消费者，不删除它仍使用的目录。 */
test('spawn failure reaps the already started consumer', async t => {
  const f = await fixture(t, '', '');
  let consumer, closed = false;
  await assert.rejects(f.run({ spawnProcess: (executable, args, settings) => {
    if (consumer) throw new Error('fixture spawn denied');
    consumer = spawn(process.execPath, ['-e', 'process.stdin.resume();setInterval(()=>{},1000)'], settings);
    consumer.once('close', () => { closed = true; });
    return consumer;
  } }), /spawn denied/);
  assert.equal(closed, true, 'the first child must close before extraction rejects');
});

/** 无法确认 close 时暴露保留 scratch 标记，而不是无界等待或伪造回收成功。 */
test('unconfirmed cleanup is bounded and preserves scratch ownership', async () => {
  const children = [];
  await assert.rejects(extractTarXz({ archive: path.resolve('a.tar.xz'), destination: path.resolve('out'), sevenZip: process.execPath, timeoutMs: 20, reapMs: 20,
    spawnProcess: () => {
      const child = new EventEmitter();
      Object.assign(child, { pid: undefined, stdin: new PassThrough(), stdout: new PassThrough(), stderr: new PassThrough(), unref() { this.detached = true; } });
      children.push(child); return child;
    } }), error => error.preserveTemporary === true && /cleanup unconfirmed/.test(error.message));
  assert.equal(children.every(child => child.detached), true);
});

/** Windows runner 必须用已有 7-Zip 真正解开 XZ/TAR；本机无该工具时明确报告未覆盖。 */
test('installed 7-Zip extracts a real small tar.xz with space paths', async t => {
  const sevenZip = process.platform === 'win32' ? path.win32.join(process.env.ProgramFiles ?? 'C:\\Program Files', '7-Zip/7z.exe') : process.env.NARRANT_TEST_SEVENZIP;
  if (!sevenZip) return t.skip('Local 7-Zip unavailable; Windows CI must execute the real fixture');
  await fs.access(sevenZip);
  const root = await fs.mkdtemp(path.join(os.tmpdir(), 'real xz 测试-'));
  t.after(() => fs.rm(root, { recursive: true, force: true }));
  await fs.mkdir(path.join(root, 'sdk'));
  const data = Buffer.from(Array.from({ length: 65536 }, (_, i) => i % 256));
  await fs.writeFile(path.join(root, 'sdk', 'header.bin'), data);
  execFileSync(sevenZip, ['a', '-ttar', 'fixture.tar', 'sdk'], { cwd: root, stdio: 'ignore', timeout: 10_000 });
  execFileSync(sevenZip, ['a', '-txz', 'fixture.tar.xz', 'fixture.tar'], { cwd: root, stdio: 'ignore', timeout: 10_000 });
  await fs.rm(path.join(root, 'fixture.tar'));
  const destination = path.join(root, 'extracted'); await fs.mkdir(destination);
  await extractTarXz({ archive: path.join(root, 'fixture.tar.xz'), destination, sevenZip, timeoutMs: 10_000 });
  assert.deepEqual(await fs.readFile(path.join(destination, 'sdk', 'header.bin')), data);
  assert.deepEqual((await fs.readdir(destination)).sort(), ['sdk']);
  await assert.rejects(fs.stat(path.join(root, 'fixture.tar')), { code: 'ENOENT' });
});
