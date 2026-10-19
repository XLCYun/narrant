import assert from 'node:assert/strict';
import { mkdir, readFile, writeFile } from 'node:fs/promises';
import path from 'node:path';
import { DatabaseSync } from 'node:sqlite';

/** 正式包读取的普通用户设置；不需要测试版应用或额外 IPC。 */
export async function writeUpgradeUserSettings(dataRoot) {
  await mkdir(dataRoot, { recursive: true });
  const settings = {
    version: 1, theme: 'dark', language: 'zh-CN',
    babysit: {
      provider: 'custom', repositoryExploration: false, allNonEmptyLines: false,
      codex: { executable: null, model: null, reasoningEffort: null },
      claudeCode: { executable: null, model: null, reasoningEffort: null },
      custom: { protocol: 'openai_chat_completions', baseUrl: '', model: '', effort: 'provider_default', credentialGeneration: 0 },
    },
  };
  await writeFile(path.join(dataRoot, 'settings.json'), JSON.stringify(settings), { flag: 'wx' });
}

/** 在旧正式应用已创建的数据库中登记临时 Git 项目，沿用该版本的 schema。 */
export function registerUpgradeRepository(dataRoot, repositoryRoot) {
  const database = new DatabaseSync(path.join(dataRoot, 'narrant.sqlite3'), { open: false });
  try {
    database.open();
    database.exec('PRAGMA busy_timeout = 10000');
    database.prepare(`INSERT INTO repositories
      (canonical_path, git_common_directory, created_at_millis, last_opened_at_millis)
      VALUES (?, ?, ?, ?)`).run(repositoryRoot, path.join(repositoryRoot, '.git'), 1, 1);
  } finally { database.close(); }
}

/** 在 candidate 正常启动并退出后核对实际设置与项目记录，数据丢失即停止验收。 */
export async function assertUpgradeUserData(dataRoot, repositoryRoot) {
  const settings = JSON.parse(await readFile(path.join(dataRoot, 'settings.json'), 'utf8'));
  assert.equal(settings.theme, 'dark', 'Upgrade lost theme preference');
  assert.equal(settings.language, 'zh-CN', 'Upgrade lost language preference');
  const database = new DatabaseSync(path.join(dataRoot, 'narrant.sqlite3'), { readOnly: true });
  try {
    const entry = database.prepare('SELECT canonical_path, git_common_directory FROM repositories WHERE canonical_path = ?').get(repositoryRoot);
    assert.equal(entry?.canonical_path, repositoryRoot, 'Upgrade lost registered project');
    assert.equal(entry?.git_common_directory, path.join(repositoryRoot, '.git'), 'Upgrade changed project identity');
  } finally { database.close(); }
}
