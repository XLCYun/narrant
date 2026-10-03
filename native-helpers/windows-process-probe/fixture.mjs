import fs from 'node:fs';
import path from 'node:path';
import { spawn } from 'node:child_process';

const [directory, mode] = process.argv.slice(2);
const marker = path.join(directory, 'descendant');
// Windows detached 跳过 libuv 内部 Job；不请求 CREATE_BREAKAWAY_FROM_JOB。
spawn(process.execPath, ['-e', 'setInterval(() => {}, 1000); const fs = require("node:fs"); const marker = process.argv[1]; fs.writeFileSync(marker + ".tmp", String(process.pid)); fs.renameSync(marker + ".tmp", marker);', marker], {
  detached: true,
  stdio: mode === 'parent_exited_closed' ? 'ignore' : ['ignore', 'inherit', 'inherit'],
});
// 仅完整 PID 标记表示后代已启动；让 Host 在 cleanup 前持有其真实进程句柄。
const ready = setInterval(() => {
  if (!fs.existsSync(marker)) return;
  clearInterval(ready);
  fs.writeFileSync(path.join(directory, 'ready'), 'ready');
  if (mode !== 'parent_alive' && mode !== 'admission_gate') process.exit(0);
}, 10);
// 只有隔离 gate 场景在 Rust 明确封闭 Job 入口后尝试再创建一个进程。
if (mode === 'admission_gate') {
  const attempt = setInterval(() => {
    if (!fs.existsSync(path.join(directory, 'attempt-spawn'))) return;
    clearInterval(attempt);
    /** 只记录固定阶段名，用于区分进入创建前、同步抛错与异步事件。 */
    const stage = name => fs.appendFileSync(path.join(directory, 'attempt-trace'), `${name}\n`);
    /** 原子发布结果，防止观察方读取尚未完成的标记内容。 */
    const publish = result => {
      const marker = path.join(directory, 'attempt-result');
      fs.writeFileSync(marker + '.tmp', result);
      fs.renameSync(marker + '.tmp', marker);
    };
    stage('before-spawn');
    try {
      const child = spawn(process.execPath, ['-e', 'process.exit(0)'], { detached: true, stdio: 'ignore' });
      stage('spawn-returned');
      child.once('error', error => {
        stage('error-event');
        publish(`rejected:${error.code}`);
      });
      child.once('spawn', () => {
        stage('spawn-event');
        publish('accepted');
      });
    } catch (error) {
      stage('synchronous-throw');
      // 同步异常单独报告，本轮不把未知异常当成 gate 通过。
      publish(`thrown:${error.code}:${String(error.message).slice(0, 1024)}`);
    }
  }, 5);
}
setInterval(() => {}, 1000);
