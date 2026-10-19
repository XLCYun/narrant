import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import { selectMsvcRedist, readWindowsDllVersions } from '../language-packs/select-msvc-redist.mjs';

/** 用受控文件夹模拟 VS 安装元数据，目录 token 与 DLL 版本刻意不同。 */
async function fixture(t, token = '14.44.35000') {
  const root = await fs.realpath(await fs.mkdtemp(path.join(os.tmpdir(), 'VS redist 测试-')));
  t.after(() => fs.rm(root, { recursive: true, force: true }));
  const metadata = path.join(root, 'VC/Auxiliary/Build/Microsoft.VCRedistVersion.default.txt');
  await fs.mkdir(path.dirname(metadata), { recursive: true });
  await fs.writeFile(metadata, `\uFEFF${token}\r\n`);
  const selected = path.join(root, 'VC/Redist/MSVC', token, 'x64/Microsoft.VC143.CRT');
  await fs.mkdir(selected, { recursive: true });
  const names = ['msvcp140.dll', 'vcruntime140.dll', 'vcruntime140_1.dll'];
  for (const name of names) await fs.writeFile(path.join(selected, name), 'fixture');
  const versions = names.map(name => ({ name, runtime_version: '14.44.35211', file_version: '14.44.35211.0' }));
  const messages = [];
  return { root, metadata, selected, versions, messages, options: { visualStudio: root, expectedVersion: '14.44.35211', readVersions: directory => { assert.equal(directory, selected); return versions; }, log: message => messages.push(message) } };
}

/** 默认元数据可以指定数字目录或文档中的 v143，不能把 DLL patch 版本当目录。 */
for (const token of ['14.44.35000', 'v143']) test(`selects the exact metadata directory ${token}, not a newer or DLL-version directory`, async t => {
  const f = await fixture(t, token);
  await fs.mkdir(path.join(f.root, 'VC/Redist/MSVC/99.99.99999'), { recursive: true });
  assert.equal(await selectMsvcRedist(f.options), f.selected);
  assert.ok(f.messages.some(message => message.includes('14.44.35211.0')));
});

/** 元数据缺失或越界时直接失败，现有相邻目录不能成为自动回退来源。 */
test('missing or escaping metadata fails closed and prints bounded directory evidence', async t => {
  const f = await fixture(t);
  for (const token of ['../outside', 'C:\\outside', '14.44.35000\n99.99.99999', 'v144']) {
    await fs.writeFile(f.metadata, token);
    await assert.rejects(selectMsvcRedist(f.options), /metadata token/);
  }
  await fs.rm(f.metadata);
  await assert.rejects(selectMsvcRedist(f.options), { code: 'ENOENT' });
  assert.ok(f.messages.some(message => message.includes('direct directories=["14.44.35000"]')));
});

/** DLL 实际版本不符时保留完整版本诊断，不修改固定 pin 或选择其他版本目录。 */
test('runtime version mismatch or incomplete DLL evidence cannot pass', async t => {
  const f = await fixture(t);
  f.versions[0].runtime_version = '14.51.36247';
  f.versions[0].file_version = '14.51.36247.0';
  await assert.rejects(selectMsvcRedist(f.options), /version mismatch.*14\.51\.36247\.0/);
  assert.ok(f.messages.some(message => message.includes('14.51.36247.0')));
  f.versions.pop();
  await assert.rejects(selectMsvcRedist(f.options), /Incomplete/);
});

/** Windows CI 使用真实系统 DLL 验证版本资源读取链路，本机无 Windows 时明确未覆盖。 */
test('Windows reads DLL version resources without executing the DLL', async t => {
  if (process.platform !== 'win32') return t.skip('Windows version-resource reader requires Windows');
  const root = await fs.mkdtemp(path.join(os.tmpdir(), 'CRT metadata 测试-'));
  t.after(() => fs.rm(root, { recursive: true, force: true }));
  await fs.copyFile(path.join(process.env.SystemRoot, 'System32/version.dll'), path.join(root, 'version.dll'));
  const records = readWindowsDllVersions(root);
  assert.equal(records.length, 1);
  assert.equal(records[0].name, 'version.dll');
  assert.match(records[0].runtime_version, /^\d+\.\d+\.\d+$/);
  assert.match(records[0].file_version, /^\d+\.\d+\.\d+\.\d+$/);
  assert.notEqual(records[0].runtime_version, '0.0.0');
});
