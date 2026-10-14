import assert from 'node:assert/strict';
import fs from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';

// 在下载和编译前验证服务身份能创建真实文件、目录符号链接，不能用 junction 代替。
if (process.platform !== 'win32') throw new Error('Windows runner verification requires Windows');
const root = await fs.mkdtemp(path.join(os.tmpdir(), 'narrant-runner-links-'));
try {
  await fs.mkdir(path.join(root, 'target'));
  await fs.writeFile(path.join(root, 'target', 'file.txt'), 'runner link probe');
  await fs.symlink(path.join('target', 'file.txt'), path.join(root, 'file-link'), 'file');
  await fs.symlink('target', path.join(root, 'directory-link'), 'dir');
  for (const link of ['file-link', 'directory-link']) {
    assert.equal((await fs.lstat(path.join(root, link))).isSymbolicLink(), true);
  }
  for (const link of ['file-link', path.join('directory-link', 'file.txt')]) {
    assert.equal(await fs.readFile(path.join(root, link), 'utf8'), 'runner link probe');
  }
  console.log('Windows runner can create and read file and directory symbolic links.');
} catch (cause) {
  throw new Error('Windows runner symbolic-link prerequisite failed. Check the runner service account\'s Create symbolic links right and restart the service after granting it.', { cause });
} finally {
  await fs.rm(root, { recursive: true, force: true });
}
