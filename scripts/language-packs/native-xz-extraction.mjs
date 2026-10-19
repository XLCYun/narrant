import path from 'node:path';
import { spawn, execFileSync } from 'node:child_process';
import { pipeline } from 'node:stream/promises';

/** Windows 强制回收当前解包进程树；其他系统仅供同一管道的本地回归测试。 */
function terminate(child) {
  if (!child.pid) return;
  if (process.platform === 'win32') {
    execFileSync(path.win32.join(process.env.SystemRoot, 'System32/taskkill.exe'), ['/PID', String(child.pid), '/T', '/F'], { timeout: 5_000, stdio: 'ignore', windowsHide: true });
  } else child.kill('SIGKILL');
}

/** 直接连接 XZ 与 TAR 的二进制流，不落地巨型中间 TAR，失败后有界等待两端回收。 */
export async function extractTarXz({ archive, destination, sevenZip, timeoutMs = 30 * 60 * 1000, reapMs = 10_000, spawnProcess = spawn }) {
  if (![archive, destination, sevenZip].every(value => path.isAbsolute(value))) throw new Error('XZ extraction requires absolute paths');
  if (![timeoutMs, reapMs].every(value => Number.isFinite(value) && value > 0)) throw new Error('XZ extraction requires finite positive deadlines');
  const processes = [];
  let fail, timer;
  const failure = new Promise((_, reject) => { fail = reject; });
  // 在 spawn 同步失败时也消费 failure，避免留下未处理的拒绝。
  failure.catch(() => {});
  /** 即使 error 先到也等待 close；stderr 只保留末尾，避免填满管道或无限缓存。 */
  function start(label, args, input) {
    const child = spawnProcess(sevenZip, args, { shell: false, windowsHide: true, stdio: [input, 'pipe', 'pipe'] });
    const state = { child, closed: false, stderr: '' };
    state.done = new Promise(resolve => {
      child.once('error', error => fail(new Error(`${label} spawn failed: ${error.message}`)));
      child.once('close', (code, signal) => { state.closed = true; if (code !== 0) fail(new Error(`${label} failed (${code}/${signal}): ${state.stderr}`)); resolve(); });
    });
    child.stderr.on('data', chunk => { state.stderr = (state.stderr + chunk.toString()).slice(-8192); });
    processes.push(state);
    return child;
  }
  try {
    const consumer = start('7-Zip TAR', ['x', '-ttar', '-si', `-o${destination}`, '-y'], 'pipe');
    consumer.stdout.resume();
    const producer = start('7-Zip XZ', ['x', '-txz', '-so', archive], 'ignore');
    const transfer = pipeline(producer.stdout, consumer.stdin);
    const completed = Promise.all([transfer, ...processes.map(state => state.done)]);
    timer = setTimeout(() => fail(new Error(`7-Zip XZ/TAR extraction timed out after ${timeoutMs}ms`)), timeoutMs);
    await Promise.race([completed, failure]);
  } catch (error) {
    clearTimeout(timer);
    for (const { child, closed } of processes) {
      child.stdin?.destroy();
      child.stdout?.destroy();
      if (!closed) { try { terminate(child); } catch { /* 终止结果仍以 close 确认，保留原始失败原因。 */ } }
    }
    let reapTimer;
    await Promise.race([Promise.all(processes.map(state => state.done)), new Promise(resolve => { reapTimer = setTimeout(resolve, reapMs); })]);
    clearTimeout(reapTimer);
    if (processes.some(state => !state.closed)) {
      // 回收未确认时不能删除仍在使用的 scratch，也不能让 stdio 保活造成第二次无限等待。
      for (const { child } of processes) { child.stdin?.destroy(); child.stdout?.destroy(); child.stderr?.destroy(); child.unref(); }
      const incomplete = new Error(`${error.message}; extraction process cleanup unconfirmed; preserve temporary directory`, { cause: error });
      incomplete.preserveTemporary = true;
      throw incomplete;
    }
    throw error;
  } finally { clearTimeout(timer); }
}
