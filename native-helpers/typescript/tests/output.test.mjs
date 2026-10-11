import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { test } from 'node:test';
import { writeWindowsPipe } from '../main.mjs';

/** 使用真实临时描述符，既检查异步写入的字节内容，也让用例覆盖 Windows 分支的同一个函数。 */
function destination(t) {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'ts-output-'));
  const file = path.join(root, 'output');
  const fd = fs.openSync(file, 'w');
  t.after(() => { fs.closeSync(fd); fs.rmSync(root, { recursive: true, force: true }); });
  return { fd, file };
}

test('Windows writer waits for every native short write and retains UTF-8 byte boundaries', async t => {
  const { fd, file } = destination(t);
  const original = fs.write;
  const offsets = [];
  let active = false;
  t.mock.method(fs, 'write', (descriptor, buffer, offset, length, position, callback) => {
    assert.equal(active, false, 'descriptor writes must never overlap');
    assert.equal(position, null, 'pipe writes must not seek');
    active = true;
    offsets.push(offset);
    original(descriptor, buffer, offset, Math.min(length, 2), position, (error, bytes) => {
      active = false;
      callback(error, bytes);
    });
  });
  const text = '中文🙂\n';
  let completed = false;
  const pending = writeWindowsPipe(fd, text).then(() => { completed = true; });
  assert.equal(completed, false, 'native callback must be the flush barrier');
  await pending;
  assert.equal(active, false);
  assert.deepEqual(offsets, [0, 2, 4, 6, 8, 10]);
  assert.deepEqual(fs.readFileSync(file), Buffer.from(text));
});

test('Windows writer propagates an error after a real partial write without retrying it', async t => {
  const { fd, file } = destination(t);
  const original = fs.write;
  const failure = Object.assign(new Error('closed output pipe'), { code: 'EPIPE' });
  let calls = 0;
  t.mock.method(fs, 'write', (descriptor, buffer, offset, length, position, callback) => {
    if (++calls === 1) original(descriptor, buffer, offset, 2, position, callback);
    else setImmediate(() => callback(failure, 0));
  });
  await assert.rejects(writeWindowsPipe(fd, 'partial'), error => error === failure);
  assert.equal(calls, 2);
  assert.equal(fs.readFileSync(file, 'utf8'), 'pa');
});

test('Windows writer rejects zero progress instead of looping', async t => {
  const write = t.mock.method(fs, 'write', (fd, buffer, offset, length, position, callback) => setImmediate(() => callback(null, 0)));
  await assert.rejects(writeWindowsPipe(1, 'pending'), /未取得进展/);
  assert.equal(write.mock.callCount(), 1);
});

test('Windows writer preserves native descriptor errors', async t => {
  const { file } = destination(t);
  const closed = fs.openSync(file, 'w');
  fs.closeSync(closed);
  await assert.rejects(writeWindowsPipe(closed, 'pending'), { code: 'EBADF' });
});
