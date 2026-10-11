import readline from 'node:readline';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { createRequire } from 'node:module';
import { isMainThread, parentPort, Worker, workerData } from 'node:worker_threads';

const protocol = 'narrant.native-analyzer/1';

/** Windows 标准管道的 stream.write 会同步阻塞；回调式 fs.write 在 libuv 工作线程写入。 */
export async function writeWindowsPipe(fd, text) {
  const buffer = Buffer.from(text);
  let offset = 0;
  while (offset < buffer.length) {
    const written = await new Promise((resolve, reject) => {
      fs.write(fd, buffer, offset, buffer.length - offset, null, (error, bytes) => error ? reject(error) : resolve(bytes));
    });
    if (written === 0) throw new Error('标准管道写入未取得进展');
    offset += written;
  }
}

/** POSIX 保留 stream 的非阻塞背压处理；Windows 按字节补齐短写，两个平台都等待 flush。 */
function writeOutput(stream, text) {
  if (process.platform === 'win32') return writeWindowsPipe(stream.fd, text);
  return new Promise((resolve, reject) => stream.write(text, error => error ? reject(error) : resolve()));
}

/** 仅 Host 提前断开且分析线程已回收时丢弃余量；Windows exit 会等待仍阻塞的 libuv 写线程。 */
function stopAfterAbandonedOutput() {
  if (process.platform === 'win32') process.kill(process.pid, 'SIGKILL');
  else process.exit(process.exitCode ?? 0);
}

/** stdout 专用于严格 NDJSON；等待写入完成后才能关闭协议输出。 */
function send(value) {
  return writeOutput(process.stdout, `${JSON.stringify({ protocol, ...value })}\n`);
}

/** 原生分析只在一个进程内线程运行；同一份 Program、源码快照与投影不变。 */
async function runAnalysis() {
  try {
    const require = createRequire(import.meta.url);
    const ts = require(path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../typescript/lib/typescript.js'));
    const { analyze } = await import('./analyze.mjs');
    parentPort.postMessage({ type: 'completed', id: workerData.id, outcome: 'ok', ...analyze(ts, workerData) });
  } catch (error) {
    // 默认 Worker stderr 会转发到主线程的同步管道；先报告真实失败，再由控制循环异步写诊断。
    parentPort.postMessage({ ...failed(workerData.id, error), diagnostic: `${error.stack}\n` });
  } finally { parentPort.close(); }
}

/** 已观察的原生异常保留真实失败，不能被随后到来的取消覆盖。 */
function failed(id, error) {
  return { type: 'completed', id, outcome: 'error', error: { code: 'object_extraction_failed', message: String(error.message), paths: [] } };
}

/** 主线程独占控制输入和唯一终结；回收分析线程后才 flush 并退出。 */
async function runProtocol() {
  // 管道关闭交由写入回调处理，避免 EPIPE 的 error 事件绕过退出屏障。
  process.stdout.on('error', () => {});
  process.stderr.on('error', () => {});
  await send({ type: 'ready', helper_id: 'narrant-typescript', helper_version: '0.1.0', projection_revision: 'typescript-source-v1', languages: ['typescript', 'javascript'], profiles: ['typescript-static-v1'] });
  const lines = readline.createInterface({ input: process.stdin, crlfDelay: Infinity });
  let request, worker, completion, failure, protocolFailure, stopping, diagnostic;
  let finishing = false, writing = false, flushed = false, outputAbandoned = false;
  let abandonOutput;
  const abandoned = new Promise(resolve => { abandonOutput = resolve; });

  /** 线程退出屏障同时覆盖完成、取消、Host 消失与异常控制。 */
  async function finish() {
    if (finishing) return;
    finishing = true;
    try {
      if (worker) await worker.terminate();
      if (protocolFailure) {
        diagnostic = `${protocolFailure.stack}\n`;
        process.exitCode = 1;
      }
      const terminal = stopping === 'EOF' ? null : failure ?? (!protocolFailure && (stopping === 'cancel' ? { type: 'completed', id: request.id, outcome: 'cancelled' } : completion));
      if (stopping !== 'EOF' && (terminal || diagnostic)) {
        writing = true;
        await Promise.race([(async () => {
          // Host 收到终结即可关闭 stdin；先完成诊断，避免把普通关闭误判成背压期间的提前 EOF。
          // 诊断管道损坏仍要交付已观察的结构化错误，同时保留 I/O 故障的非零退出。
          if (diagnostic) await writeOutput(process.stderr, diagnostic).catch(() => { process.exitCode = 1; });
          if (terminal && stopping !== 'EOF') await send(terminal);
          flushed = true;
        })(), abandoned]);
        writing = false;
      }
      lines.close();
      process.stdin.destroy();
      // 此处 worker 已经 await 回收，协议读取也已关闭；普通终结始终完整 flush 并正常退出。
      if (outputAbandoned) stopAfterAbandonedOutput();
      if (!process.stdout.destroyed) process.stdout.end();
    } catch (error) {
      process.exitCode = 1;
      // 输出故障的诊断也不能阻塞 EOF；诊断管道本身失败时保留非零退出，不递归记录。
      writing = true;
      if (stopping !== 'EOF') await Promise.race([writeOutput(process.stderr, `${error.stack}\n`).catch(() => {}), abandoned]);
      writing = false;
      lines.close();
      process.stdin.destroy();
      if (outputAbandoned) stopAfterAbandonedOutput();
      process.stdout.destroy();
    }
  }

  lines.on('line', line => {
    if (writing || flushed) return;
    try {
      const message = JSON.parse(line);
      if (message?.protocol !== protocol) throw new Error('无效分析协议');
      if (!request) {
        if (message.type !== 'analyze' || typeof message.id !== 'string' || !message.id) throw new Error('无效分析请求');
        request = message;
        // 单次 worker thread 属于当前 helper 进程，不增加服务器、线程池或外部进程所有者。
        try { worker = new Worker(new URL(import.meta.url), { workerData: request }); }
        catch (error) { failure = failed(request.id, error); void finish(); return; }
        worker.on('message', result => {
          const { diagnostic: detail, ...terminal } = result;
          diagnostic = detail;
          completion = terminal;
          if (terminal.outcome === 'error') failure = terminal;
          void finish();
        });
        worker.on('error', error => { failure = failed(request.id, error); void finish(); });
        worker.on('exit', code => {
          if (!finishing) {
            failure = failed(request.id, new Error(`分析线程未报告终结：exit ${code}`));
            void finish();
          }
        });
      } else {
        if (message.type !== 'cancel' || message.id !== request.id) throw new Error('无效或重复分析控制');
        stopping = 'cancel';
        void finish();
      }
    } catch (error) { protocolFailure = error; void finish(); }
  });
  lines.on('close', () => {
    if (flushed) return;
    stopping = 'EOF';
    if (writing) {
      /** 给已被 Host 完整读取、但尚未派发的 Windows 文件写入回调一个有限收尾窗口。 */
      const abandonPending = () => {
        if (!flushed) { outputAbandoned = true; abandonOutput(); }
      };
      if (process.platform === 'win32') setTimeout(abandonPending, 100).unref();
      else abandonPending();
    }
    void finish();
  });
}

// 固定 Node 24 的公开入口标识允许独立测试写入函数；实际主入口和 Worker 入口仍各运行一次。
if (import.meta.main) {
  if (isMainThread) await runProtocol();
  else await runAnalysis();
}
