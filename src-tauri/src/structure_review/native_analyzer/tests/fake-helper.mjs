// 独立 Node 假 helper，便于 macOS 与 Windows CI 使用同一套生命周期场景。
import fs from 'node:fs';
import path from 'node:path';
import readline from 'node:readline';
import { spawn } from 'node:child_process';
import { randomBytes } from 'node:crypto';

const [control, mode] = process.argv.slice(2);
fs.writeFileSync(path.join(control, 'pid'), String(process.pid));

/** 按原字节输出夹具，可逐字节验证 UTF-8 极小分片。 */
function output(name, fragmented = false) {
  const bytes = fs.readFileSync(path.join(control, name));
  if (fragmented) {
    for (let offset = 0; offset < bytes.length; offset++) fs.writeSync(1, bytes.subarray(offset, offset + 1));
  } else fs.writeSync(1, bytes);
}

/** helper 主动结束时关闭自身；遗留子进程由 Host 所有权测试回收。 */
function complete() {
  output('result', mode === 'fragment');
  if (mode === 'double') output('result');
  if (mode === 'nonzero') process.exit(7);
  if (mode === 'exit_stall') return;
  process.exit(0);
}

const keepAlive = setInterval(() => {}, 1000);
process.stdin.on('end', () => {
  if (mode === 'exit_stall') return;
  fs.writeFileSync(path.join(control, 'host-eof'), 'stopped');
  clearInterval(keepAlive);
  process.exit(0);
});
if (mode === 'startup_stderr_first') {
  // 反向 EOF 顺序也必须保留诊断，即使进程返回零。
  process.stderr.end('unique-runtime-initialization-failure', () => setTimeout(() => {
    process.stdout.end(() => process.exit(0));
  }, 50));
} else if (mode === 'startup_diagnostic') {
  // 先关 stdout，再延迟写入启动错误，验证 Host 等待 stderr/exit。
  process.stdout.end(() => setTimeout(() => {
    fs.writeSync(2, 'unique-runtime-initialization-failure');
    process.exit(7);
  }, 50));
} else if (mode === 'no_read') {
  output('ready');
} else if (mode === 'startup_stall') {
  process.stdin.resume();
} else if (mode === 'premature') {
  output('result');
  process.exit(0);
} else {
  output('ready', mode === 'fragment');
  const lines = readline.createInterface({ input: process.stdin });
  let requested = false;
  lines.on('line', (line) => {
    if (requested) {
      if (mode === 'cancel') {
        fs.writeFileSync(path.join(control, 'cancel'), line);
        complete();
      }
      return;
    }
    requested = true;
    fs.writeFileSync(path.join(control, 'request'), line);
    if (mode === 'runtime_environment') {
      const request = JSON.parse(line);
      // 仅输出固定测试条件的布尔结果，不采集、转储任何宿主环境值。
      fs.writeFileSync(path.join(control, 'runtime-environment-status'), JSON.stringify({
        crypto_ready: randomBytes(32).length === 32,
        explicit_marker_received: process.env.NARRANT_NATIVE_ENV_SENTINEL === 'explicit-fixture-value',
        system_root_present: Boolean(process.env.SystemRoot || process.env.SYSTEMROOT),
        runtime_injection_absent: ['PATH', 'NODE_OPTIONS', 'NODE_PATH', 'OPENSSL_CONF', 'OPENSSL_MODULES', 'JAVA_TOOL_OPTIONS', 'CLASSPATH', 'PYTHONPATH'].every(key => process.env[key] === undefined),
        scratch_matches_request: ['TMPDIR', 'TMP', 'TEMP'].every(key => process.env[key] === request.environment.scratch_root),
      }));
    }
    if (mode === 'stall' || mode === 'cancel') return;
    if (mode === 'stderr' || mode === 'stderr_ok') fs.writeSync(2, Buffer.alloc(mode === 'stderr' ? 524288 : 131072));
    if (mode === 'eof') process.exit(0);
    if (mode === 'half') { fs.writeSync(1, '{"protocol":'); process.exit(0); }
    if (mode === 'duplicate_ready') output('ready');
    if (mode === 'descendant' || mode === 'descendant_closed') {
      const marker = path.join(control, 'descendant');
      spawn(process.execPath, ['-e', 'setInterval(() => {}, 1000); const fs = require("node:fs"); const marker = process.argv[1]; fs.writeFileSync(marker + ".tmp", String(process.pid)); fs.renameSync(marker + ".tmp", marker);', marker], {
        stdio: mode === 'descendant' ? ['ignore', 'inherit', 'inherit'] : 'ignore',
        // Windows detached 仅绕过 Node 自己的随父退出 Job，未请求逃离 Host 的外层 Job。
        // Unix 必须保持原进程组，让 Host 仍能完整回收后代。
        detached: process.platform === 'win32',
      });
      // 后代原子发布完整 PID，证明运行时和继承管道已就绪，再让父进程退出。
      const ready = setInterval(() => {
        if (!fs.existsSync(marker)) return;
        clearInterval(ready);
        complete();
      }, 10);
      return;
    }
    complete();
  });
}
