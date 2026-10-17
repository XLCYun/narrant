import assert from 'node:assert/strict';
import test from 'node:test';
import { mkdtemp, readFile, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { DatabaseSync } from 'node:sqlite';
import { writeUpgradeUserSettings, registerUpgradeRepository, assertUpgradeUserData } from '../update-smoke-user-data.mjs';

test('upgrade evidence detects missing projects and reset settings in actual persisted files', async t => {
  const root = await mkdtemp(path.join(tmpdir(), 'narrant-upgrade-data-'));
  t.after(() => rm(root, { recursive: true, force: true }));
  const project = path.join(root, 'project');
  await writeUpgradeUserSettings(root);
  const database = new DatabaseSync(path.join(root, 'narrant.sqlite3'));
  t.after(() => database.close());
  database.exec('CREATE TABLE repositories (canonical_path TEXT UNIQUE, git_common_directory TEXT, created_at_millis INTEGER, last_opened_at_millis INTEGER)');
  await assert.rejects(assertUpgradeUserData(root, project), /lost registered project/);
  registerUpgradeRepository(root, project);
  await assertUpgradeUserData(root, project);
  const settings = JSON.parse(await readFile(path.join(root, 'settings.json')));
  await writeFile(path.join(root, 'settings.json'), JSON.stringify({ ...settings, theme: 'system' }));
  await assert.rejects(assertUpgradeUserData(root, project), /lost theme/);
  await writeFile(path.join(root, 'settings.json'), JSON.stringify(settings));
  database.exec('DELETE FROM repositories');
  await assert.rejects(assertUpgradeUserData(root, project), /lost registered project/);
});
