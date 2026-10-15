import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { open } from 'node:fs/promises';
import path from 'node:path';

// 共用现有 runner；在大型构建或系统变更前确认任务确实处于可用的桌面会话。
assert.equal(process.env.RUNNER_ENVIRONMENT, 'self-hosted', 'Desktop CI requires a self-hosted runner');
if (process.platform === 'darwin') {
  assert.equal(process.arch, 'arm64');
  assert.equal(execFileSync('/bin/launchctl', ['managername'], { encoding: 'utf8' }).trim(), 'Aqua',
    'Start the existing runner in a logged-in macOS Aqua session; its LaunchDaemon cannot provide desktop UI');
} else {
  assert.equal(process.platform, 'win32');
  assert.equal(process.arch, 'x64');
  execFileSync('pwsh', ['-NoProfile', '-NonInteractive', '-Command',
    `if ((Get-Process -Id ${process.pid}).SessionId -eq 0 -or -not [Environment]::UserInteractive) { throw 'Start the existing runner in a logged-in Windows session; its Session 0 service cannot provide desktop UI' }`],
  { stdio: 'inherit' });
}

if (process.argv.includes('--update-smoke')) {
  if (process.platform === 'darwin') {
    execFileSync('/usr/bin/sudo', ['-n', 'true'], { stdio: 'inherit' });
  } else {
    // 只打开文件确认写权限，不改变任何 hosts 内容。
    const hosts = await open(path.join(process.env.SystemRoot, 'System32/drivers/etc/hosts'), 'r+');
    await hosts.close();
  }
}
console.log('Self-hosted desktop prerequisites passed; the actual UI/lifecycle tests still follow.');
