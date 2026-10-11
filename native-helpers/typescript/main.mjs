import readline from 'node:readline';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { createRequire } from 'node:module';
import { isMainThread, parentPort, Worker, workerData } from 'node:worker_threads';

const protocol = 'narrant.native-analyzer/1';
// 管道关闭交由 write 回调处理，避免 EPIPE 的 error 事件绕过退出屏障。
process.stdout.on('error', () => {});

/** stdout 专用于严格 NDJSON；等待写入完成后才能关闭协议输出。 */
function send(value) {
  return new Promise((resolve, reject) => process.stdout.write(`${JSON.stringify({ protocol, ...value })}\n`, error => error ? reject(error) : resolve()));
}

/** 原生分析只在一个进程内线程运行；同一份 Program、源码快照与投影不变。 */
async function runAnalysis() {
  try {
    const require = createRequire(import.meta.url);
    const ts = require(path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../typescript/lib/typescript.js'));
    const { analyze } = await import('./analyze.mjs');
    parentPort.postMessage({ type: 'completed', id: workerData.id, outcome: 'ok', ...analyze(ts, workerData) });
  } catch (error) {
    process.stderr.write(`${error.stack}\n`);
    parentPort.postMessage(failed(workerData.id, error));
  } finally { parentPort.close(); }
}

/** 已观察的原生异常保留真实失败，不能被随后到来的取消覆盖。 */
function failed(id, error) {
  return { type: 'completed', id, outcome: 'error', error: { code: 'object_extraction_failed', message: String(error.message), paths: [] } };
}

/** 主线程独占控制输入和唯一终结；回收分析线程后才 flush 并退出。 */
async function runProtocol() {
  await send({ type: 'ready', helper_id: 'narrant-typescript', helper_version: '0.1.0', projection_revision: 'typescript-source-v1', languages: ['typescript', 'javascript'], profiles: ['typescript-static-v1'] });
  const lines = readline.createInterface({ input: process.stdin, crlfDelay: Infinity });
  let request, worker, completion, failure, protocolFailure, stopping;
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
        process.stderr.write(`${protocolFailure.stack}\n`);
        process.exitCode = 1;
      }
      const terminal = stopping === 'EOF' ? null : failure ?? (!protocolFailure && (stopping === 'cancel' ? { type: 'completed', id: request.id, outcome: 'cancelled' } : completion));
      if (terminal) {
        writing = true;
        await Promise.race([send(terminal).then(() => { flushed = true; }), abandoned]);
        writing = false;
      }
      lines.close();
      process.stdin.destroy();
      // Node 的 stdout 不保证 destroy 会关闭尚有背压的标准句柄；Host 已消失时丢弃余量。
      // 此处 worker 已经 await 回收，协议读取也已关闭，主动退出不会遗留后台分析。
      if (outputAbandoned) process.exit(process.exitCode ?? 0);
      if (!process.stdout.destroyed) process.stdout.end();
    } catch (error) {
      process.stderr.write(`${error.stack}\n`);
      process.exitCode = 1;
      lines.close();
      process.stdin.destroy();
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
          completion = result;
          if (result.outcome === 'error') failure = result;
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
    if (writing) { outputAbandoned = true; abandonOutput(); }
    void finish();
  });
}

if (isMainThread) await runProtocol();
else await runAnalysis();
