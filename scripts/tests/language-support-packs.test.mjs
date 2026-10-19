import { test } from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import { createHash } from 'node:crypto';
import { packageDirectory } from '../language-packs/build-language-support-packs.mjs';

test('pack identity cannot escape its output directory', async () => {
  await assert.rejects(packageDirectory({ id: 'rust', version: '../escape', target: 'aarch64-apple-darwin' }, path.join(os.tmpdir(), 'missing-payload'), path.join(os.tmpdir(), 'missing-output')), /identity/);
});

test('pack artifact binds identity, bytes and executable metadata to the complete payload', async () => {
  const root = await fs.mkdtemp(path.join(os.tmpdir(), 'narrant-pack-test-'));
  try {
    await fs.mkdir(path.join(root, 'input/bin'), { recursive: true });
    await fs.writeFile(path.join(root, 'input/bin/narrant-rust'), 'analyzer fixture');
    const result = await packageDirectory({ id: 'rust', version: '1.0.0', target: 'aarch64-apple-darwin' }, path.join(root, 'input'), path.join(root, 'output'));
    const manifest = JSON.parse(await fs.readFile(path.join(result, 'manifest.json')));
    assert.equal(manifest.id, 'rust');
    assert.equal(manifest.version, '1.0.0');
    assert.deepEqual(manifest.files.map(file => [file.path, file.executable]), [['bin/narrant-rust', true]]);
    const bytes = await fs.readFile(path.join(result, 'pack.tar.gz'));
    assert.equal(manifest.archiveSize, bytes.length);
    assert.equal(manifest.archiveSha256, createHash('sha256').update(bytes).digest('hex'));
    await fs.utimes(path.join(root, 'input/bin/narrant-rust'), new Date(), new Date());
    const second = await packageDirectory({ id: 'rust', version: '1.0.0', target: 'aarch64-apple-darwin' }, path.join(root, 'input'), path.join(root, 'repeat'));
    assert.deepEqual(await fs.readFile(path.join(second, 'pack.tar.gz')), bytes);
  } finally { await fs.rm(root, { recursive: true, force: true }); }
});

/** 两平台最小产物和内存 GitHub，验证发布顺序与不可变性。 */
function publicationFixture() {
  const artifacts = ['aarch64-apple-darwin', 'x86_64-pc-windows-msvc'].map(target => ({ id: 'rust', version: '1.0.0', target, body: Buffer.from('{}'), archive: Buffer.from(target) }));
  let release;
  const objects = new Map();
  const events = [];
  const github = {
    findRelease: async () => release,
    createRelease: async body => { events.push('create'); release = { ...body, id: 42, assets: [] }; return release; },
    uploadAsset: async (_id, name, bytes) => { events.push('upload'); const asset = { id: objects.size + 1, name }; objects.set(name, bytes); release.assets.push(asset); return asset; },
    readAsset: async asset => objects.get(asset.name),
    publishRelease: async (_id, body) => { events.push('publish'); assert.equal(objects.size, 4); release = { ...release, ...body }; return release; },
    readPublishedAsset: async (_tag, name) => objects.get(name),
  };
  return { artifacts, github, events, objects, release: () => release };
}

test('publisher refuses incomplete platforms before remote writes', async () => {
  const { publishArtifacts } = await import('../language-packs/publish-language-support-packs.mjs');
  const fixture = publicationFixture();
  await assert.rejects(publishArtifacts(fixture.artifacts.slice(0, 1), fixture.github), /both platforms/);
  assert.deepEqual(fixture.events, []);
});

test('publisher leaves a Draft if uploaded bytes differ', async () => {
  const { publishArtifacts } = await import('../language-packs/publish-language-support-packs.mjs');
  const fixture = publicationFixture();
  fixture.github.readAsset = async () => Buffer.from('corrupt');
  await assert.rejects(publishArtifacts(fixture.artifacts, fixture.github), /differs/);
  assert.deepEqual(fixture.events, ['create', 'upload']);
  assert.equal(fixture.release().draft, true);
});

test('publisher verifies both platforms, never becomes Latest and reuses immutable versions', async () => {
  const { publishArtifacts } = await import('../language-packs/publish-language-support-packs.mjs');
  const fixture = publicationFixture();
  await publishArtifacts(fixture.artifacts, fixture.github);
  assert.equal(fixture.release().draft, false);
  assert.equal(fixture.release().prerelease, true);
  assert.equal(fixture.release().make_latest, 'false');
  const mutations = [...fixture.events];
  await publishArtifacts(fixture.artifacts, fixture.github);
  assert.deepEqual(fixture.events, mutations);
  fixture.artifacts[0].archive = Buffer.from('changed');
  await assert.rejects(publishArtifacts(fixture.artifacts, fixture.github), /differs/);
  assert.deepEqual(fixture.events, mutations);
  fixture.release().assets.pop();
  await assert.rejects(publishArtifacts(fixture.artifacts, fixture.github), /cannot be changed|differs/);
});

test('Python pack preserves its native executable, fixed control file and standard-library license', async () => {
  const root = await fs.mkdtemp(path.join(os.tmpdir(), 'narrant-python-pack-test-'));
  try {
    for (const directory of ['bin', 'control', 'licenses']) await fs.mkdir(path.join(root, 'payload', directory), { recursive: true });
    await fs.writeFile(path.join(root, 'payload/bin/narrant-python'), 'fixed native Python fixture');
    await fs.writeFile(path.join(root, 'payload/control/native.json'), '{}');
    await fs.writeFile(path.join(root, 'payload/licenses/typeshed-LICENSE'), 'typeshed license fixture');
    const result = await packageDirectory({ id: 'python', version: '1.1.0', target: 'aarch64-apple-darwin' }, path.join(root, 'payload'), path.join(root, 'output'));
    const manifest = JSON.parse(await fs.readFile(path.join(result, 'manifest.json')));
    assert.equal(manifest.id, 'python');
    assert.deepEqual(manifest.files.map(file => [file.path, file.executable]), [['bin/narrant-python', true], ['control/native.json', false], ['licenses/typeshed-LICENSE', false]]);
    assert.equal(manifest.files.find(file => file.path === 'control/native.json').size, 2);
  } finally { await fs.rm(root, { recursive: true, force: true }); }
});
