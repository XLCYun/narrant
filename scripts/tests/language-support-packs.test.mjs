import { test } from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import { createHash } from 'node:crypto';
import { packageDirectory } from '../build-language-support-packs.mjs';

test('pack identity cannot escape its output directory', async () => {
  await assert.rejects(packageDirectory({ id: 'rust', version: '../escape', target: 'aarch64-apple-darwin' }, path.join(os.tmpdir(), 'missing-payload'), path.join(os.tmpdir(), 'missing-output')), /identity/);
});

test('pack artifact binds identity, bytes and executable metadata to the complete payload', async () => {
  const root = await fs.mkdtemp(path.join(os.tmpdir(), 'narrant-pack-test-'));
  try {
    await fs.mkdir(path.join(root, 'input/bin'), { recursive: true });
    await fs.writeFile(path.join(root, 'input/bin/rust-analyzer'), 'analyzer fixture');
    const result = await packageDirectory({ id: 'rust', version: '1.0.0', target: 'aarch64-apple-darwin' }, path.join(root, 'input'), path.join(root, 'output'));
    const manifest = JSON.parse(await fs.readFile(path.join(result, 'manifest.json')));
    assert.equal(manifest.id, 'rust');
    assert.equal(manifest.version, '1.0.0');
    assert.deepEqual(manifest.files.map(file => [file.path, file.executable]), [['bin/rust-analyzer', true]]);
    const bytes = await fs.readFile(path.join(result, 'pack.tar.gz'));
    assert.equal(manifest.archiveSize, bytes.length);
    assert.equal(manifest.archiveSha256, createHash('sha256').update(bytes).digest('hex'));
    await fs.utimes(path.join(root, 'input/bin/rust-analyzer'), new Date(), new Date());
    const second = await packageDirectory({ id: 'rust', version: '1.0.0', target: 'aarch64-apple-darwin' }, path.join(root, 'input'), path.join(root, 'repeat'));
    assert.deepEqual(await fs.readFile(path.join(second, 'pack.tar.gz')), bytes);
  } finally { await fs.rm(root, { recursive: true, force: true }); }
});

test('publisher never exposes a manifest before archive verification succeeds', async () => {
  const { publishArtifacts } = await import('../publish-language-support-packs.mjs');
  const writes = [];
  const storage = { ensureObject: async key => writes.push(key), readObject: async () => Buffer.from('corrupt') };
  await assert.rejects(publishArtifacts([{ key: 'narrant/language-support-packs/rust/1.0.0/aarch64-apple-darwin', archive: Buffer.from('archive'), body: Buffer.from('{}') }], storage), /differs/);
  assert.equal(writes.length, 1);
  assert.ok(writes[0].endsWith('/pack.tar.gz'));
});

test('publisher verifies archive and manifest bytes and can safely reuse identical versions', async () => {
  const { publishArtifacts } = await import('../publish-language-support-packs.mjs');
  const objects = new Map();
  const storage = { ensureObject: async (key, bytes) => { if (objects.has(key)) assert.deepEqual(objects.get(key), bytes); else objects.set(key, bytes); }, readObject: async key => objects.get(key) };
  const artifact = { key: 'narrant/language-support-packs/rust/1.0.0/aarch64-apple-darwin', archive: Buffer.from('archive'), body: Buffer.from('{}') };
  await publishArtifacts([artifact], storage);
  await publishArtifacts([artifact], storage);
  assert.equal(objects.size, 2);
});

test('Python pack preserves its native executable, fixed control file and standard-library license', async () => {
  const root = await fs.mkdtemp(path.join(os.tmpdir(), 'narrant-python-pack-test-'));
  try {
    for (const directory of ['bin', 'control', 'licenses']) await fs.mkdir(path.join(root, 'payload', directory), { recursive: true });
    await fs.writeFile(path.join(root, 'payload/bin/ty'), 'fixed native ty fixture');
    await fs.writeFile(path.join(root, 'payload/control/ty.toml'), '');
    await fs.writeFile(path.join(root, 'payload/licenses/typeshed-LICENSE'), 'typeshed license fixture');
    const result = await packageDirectory({ id: 'python', version: '1.0.0', target: 'aarch64-apple-darwin' }, path.join(root, 'payload'), path.join(root, 'output'));
    const manifest = JSON.parse(await fs.readFile(path.join(result, 'manifest.json')));
    assert.equal(manifest.id, 'python');
    assert.deepEqual(manifest.files.map(file => [file.path, file.executable]), [['bin/ty', true], ['control/ty.toml', false], ['licenses/typeshed-LICENSE', false]]);
    assert.equal(manifest.files.find(file => file.path === 'control/ty.toml').size, 0);
  } finally { await fs.rm(root, { recursive: true, force: true }); }
});
