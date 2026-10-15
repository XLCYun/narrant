import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import fs from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';

// 在下载和编译前验证服务身份能创建真实文件、目录符号链接，不能用 junction 代替。
if (process.platform !== 'win32') throw new Error('Windows runner verification requires Windows');

// 自托管服务须实际加载小容量缓存；只检查非敏感配置，不输出整个服务环境。
if (process.env.RUNNER_ENVIRONMENT === 'self-hosted') {
  assert.equal(process.env.SCCACHE_CACHE_SIZE, '2G', 'Self-hosted Windows requires a 2 GiB compiler cache');
  assert.equal(process.env.RUSTC_WRAPPER, 'sccache', 'Self-hosted Windows must use its local compiler cache');
  assert.equal(process.env.CARGO_INCREMENTAL, '0', 'Keep the existing CI incremental setting');
  const statistics = JSON.parse(execFileSync('sccache', ['--show-stats', '--stats-format=json'], {
    encoding: 'utf8', timeout: 15_000,
  }));
  assert.equal(statistics.max_cache_size, 2 * 1024 ** 3, 'The running sccache server must enforce the 2 GiB limit');
  console.log('Self-hosted Windows compiler cache limit: 2 GiB.');
}

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
