import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { open } from 'node:fs/promises';
import path from 'node:path';

// 桌面标签只负责调度；在大型构建或系统变更前核对真实会话与更新验收的隔离声明。
assert.equal(process.env.RUNNER_ENVIRONMENT, 'self-hosted', 'Desktop CI requires a self-hosted runner');
if (process.platform === 'darwin') {
  assert.equal(process.arch, 'arm64');
  assert.equal(execFileSync('/bin/launchctl', ['managername'], { encoding: 'utf8' }).trim(), 'Aqua',
    'Start the desktop runner from the dedicated logged-in macOS session, not a LaunchDaemon');
} else {
  assert.equal(process.platform, 'win32');
  assert.equal(process.arch, 'x64');
  execFileSync('pwsh', ['-NoProfile', '-NonInteractive', '-Command',
    `if ((Get-Process -Id ${process.pid}).SessionId -eq 0 -or -not [Environment]::UserInteractive) { throw 'Start the desktop runner in a logged-in Windows session, not a service' }`],
  { stdio: 'inherit' });
}

if (process.argv.includes('--update-smoke')) {
  assert.equal(process.env.NARRANT_UPDATE_SMOKE_ISOLATED, '1',
    'Update smoke needs an isolated test system: it changes hosts, trust stores and installed app state');
  if (process.platform === 'darwin') {
    execFileSync('/usr/bin/sudo', ['-n', 'true'], { stdio: 'inherit' });
  } else {
    // 只打开文件确认写权限，不改变任何 hosts 内容。
    const hosts = await open(path.join(process.env.SystemRoot, 'System32/drivers/etc/hosts'), 'r+');
    await hosts.close();
  }
}
console.log('Self-hosted desktop prerequisites passed; the actual UI/lifecycle tests still follow.');
