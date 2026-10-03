import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import { runWindowsProcessProbe } from '../run-windows-process-probe.mjs';

// 经真实 bash → Node 边界检查路径不变，并验证 Cargo wrapper 的失败不会被吞掉。
test('probe launcher preserves the native Node path across bash and propagates wrapper status', async () => {
  const directory = await fs.mkdtemp(path.join(os.tmpdir(), 'probe handoff '));
  try {
    await fs.mkdir(path.join(directory, 'scripts'));
    await fs.writeFile(path.join(directory, 'scripts/cargo-agent.sh'), 'set -eu\nnode scripts/check-node.cjs "$@"\n');
    await fs.writeFile(path.join(directory, 'scripts/check-node.cjs'), `
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
assert.equal(process.env.NARRANT_TEST_NODE, process.execPath);
assert.ok(path.isAbsolute(process.env.NARRANT_TEST_NODE));
assert.ok(fs.statSync(process.env.NARRANT_TEST_NODE).isFile());
assert.deepEqual(process.argv.slice(2), ['run', '--manifest-path', 'native-helpers/windows-process-probe/Cargo.toml', '--locked']);
process.exit(7);
`);
    assert.equal(await runWindowsProcessProbe({ cwd: directory }), 7);
  } finally {
    await fs.rm(directory, { recursive: true, force: true });
  }
});
