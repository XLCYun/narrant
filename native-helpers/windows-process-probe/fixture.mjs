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
  if (mode !== 'parent_alive') process.exit(0);
}, 10);
setInterval(() => {}, 1000);
