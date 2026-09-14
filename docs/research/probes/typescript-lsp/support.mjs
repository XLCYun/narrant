import fs from 'node:fs/promises';
import path from 'node:path';
import { spawn, execFileSync } from 'node:child_process';
import { pathToFileURL } from 'node:url';

export const here = path.resolve(process.env.NARRANT_TS_PROBE_DIR ?? '.tools/typescript-code-graph-probe');
export const uri = file => pathToFileURL(file).href;

/** 创建单次实验文件，不运行被分析的工程代码。 */
export async function writeFiles(root, files) {
  for (const [name, text] of Object.entries(files)) {
    const file = path.join(root, name);
    await fs.mkdir(path.dirname(file), { recursive: true });
    await fs.writeFile(file, typeof text === 'string' ? text : JSON.stringify(text, null, 2));
  }
}

/** 使用字符串自身的 UTF-16 偏移找到查询位置。 */
export function position(text, marker, offset = 0) {
  const index = text.indexOf(marker);
  if (index < 0) throw new Error(`missing marker: ${marker}`);
  const before = text.slice(0, index + offset);
  const lines = before.split('\n');
  return { line: lines.length - 1, character: lines.at(-1).length };
}

/** 只记录本探针进程组，方便确认服务退出没有留下子进程。 */
function groupMembers(pgid) {
  return execFileSync('ps', ['-axo', 'pid=,pgid=,command='], { encoding: 'utf8' })
    .trim().split('\n').flatMap(line => {
      const match = line.trim().match(/^(\d+)\s+(\d+)\s+(.*)$/);
      return match && Number(match[2]) === pgid ? [{ pid: Number(match[1]), command: match[3] }] : [];
    });
}

/** 最小 LSP 客户端，保留完整响应，给每条请求设置超时。 */
export class LspProbe {
  /** 启动固定版本 TLS，收集输出并绑定退出事件。 */
  constructor(root, tsserverPath) {
    this.root = root;
    this.tsserverPath = tsserverPath;
    this.sequence = 0;
    this.pending = new Map();
    this.buffer = Buffer.alloc(0);
    this.events = [];
    this.stderr = '';
    this.proc = spawn(process.execPath, [path.join(here, 'tls6/lib/cli.mjs'), '--stdio'], {
      cwd: root, detached: true, stdio: ['pipe', 'pipe', 'pipe'],
      env: { ...process.env, NODE_OPTIONS: '', TSS_LOG: '' },
    });
    this.proc.stdout.on('data', chunk => this.consume(chunk));
    this.proc.stderr.on('data', chunk => { this.stderr += chunk.toString(); });
    this.closed = new Promise(resolve => this.proc.on('close', (code, signal) => {
      for (const pending of this.pending.values()) {
        clearTimeout(pending.timer);
        pending.reject(new Error(`LSP exited: ${code}/${signal}`));
      }
      this.pending.clear();
      resolve({ code, signal });
    }));
  }

  /** 按 Content-Length 读取，响应服务端的无副作用客户端请求。 */
  consume(chunk) {
    this.buffer = Buffer.concat([this.buffer, chunk]);
    while (true) {
      const end = this.buffer.indexOf('\r\n\r\n');
      if (end < 0) return;
      const length = Number(this.buffer.subarray(0, end).toString().match(/Content-Length:\s*(\d+)/i)?.[1]);
      if (!Number.isFinite(length)) throw new Error('invalid LSP frame');
      if (this.buffer.length < end + 4 + length) return;
      const message = JSON.parse(this.buffer.subarray(end + 4, end + 4 + length).toString());
      this.buffer = this.buffer.subarray(end + 4 + length);
      if (message.method) {
        this.events.push(message);
        if (message.id !== undefined) {
          const result = message.method === 'workspace/configuration'
            ? message.params.items.map(() => ({}))
            : message.method === 'workspace/workspaceFolders'
              ? [{ uri: uri(this.root), name: path.basename(this.root) }] : null;
          this.send({ jsonrpc: '2.0', id: message.id, result });
        }
      } else {
        const pending = this.pending.get(message.id);
        if (!pending) continue;
        this.pending.delete(message.id);
        clearTimeout(pending.timer);
        message.error ? pending.reject(new Error(JSON.stringify(message.error))) : pending.resolve(message.result);
      }
    }
  }

  /** 发送完整 JSON-RPC 帧。 */
  send(message) {
    const bytes = Buffer.from(JSON.stringify(message));
    this.proc.stdin.write(`Content-Length: ${bytes.length}\r\n\r\n`);
    this.proc.stdin.write(bytes);
  }

  /** 发送带截止时间的请求并等待结果。 */
  request(method, params, timeoutMs = 15000) {
    const id = ++this.sequence;
    return new Promise((resolve, reject) => {
      const timer = setTimeout(() => {
        this.pending.delete(id);
        reject(new Error(`timeout: ${method}`));
      }, timeoutMs);
      this.pending.set(id, { resolve, reject, timer });
      this.send({ jsonrpc: '2.0', id, method, ...(params === undefined ? {} : { params }) });
    });
  }

  /** 发送无需响应的 LSP 通知。 */
  notify(method, params) {
    this.send({ jsonrpc: '2.0', method, ...(params === undefined ? {} : { params }) });
  }

  /** 固定语义服务路径，关闭 ATA 与独立 syntax server。 */
  async initialize() {
    const result = await this.request('initialize', {
      processId: process.pid,
      rootUri: uri(this.root),
      workspaceFolders: [{ uri: uri(this.root), name: path.basename(this.root) }],
      capabilities: {
        general: { positionEncodings: ['utf-16'] },
        textDocument: { definition: { linkSupport: true }, callHierarchy: { dynamicRegistration: false } },
        workspace: { configuration: true, workspaceFolders: true },
      },
      initializationOptions: {
        hostInfo: 'narrant-code-graph-probe',
        disableAutomaticTypingAcquisition: true,
        plugins: [],
        tsserver: { path: this.tsserverPath, useSyntaxServer: 'never', logVerbosity: 'off' },
        preferences: { includePackageJsonAutoImports: 'off' },
      },
    });
    this.notify('initialized', {});
    return result;
  }

  /** 通过 TLS 公开命令传递项目控制请求。 */
  custom(command, args, config = {}) {
    return this.request('workspace/executeCommand', {
      command: 'typescript.tsserverRequest', arguments: [command, args, config],
    });
  }

  /** 把实验源码原样作为打开文档同步给服务。 */
  async open(file) {
    const text = await fs.readFile(file, 'utf8');
    const ext = path.extname(file);
    const languageId = ext === '.tsx' ? 'typescriptreact' : ext === '.jsx' ? 'javascriptreact'
      : /\.(?:[mc]?js)$/.test(ext) ? 'javascript' : 'typescript';
    this.notify('textDocument/didOpen', { textDocument: { uri: uri(file), languageId, version: 1, text } });
  }

  /** 从指定源码 occurrence 请求标准 LSP 定义。 */
  async definition(file, marker, offset = 0) {
    const text = await fs.readFile(file, 'utf8');
    return this.request('textDocument/definition', { textDocument: { uri: uri(file) }, position: position(text, marker, offset) });
  }

  /** 读取指定 Callable 的标准 LSP outgoing hierarchy。 */
  async hierarchy(file, marker) {
    const text = await fs.readFile(file, 'utf8');
    const items = await this.request('textDocument/prepareCallHierarchy', {
      textDocument: { uri: uri(file) }, position: position(text, marker),
    });
    return { items, outgoing: await Promise.all((items ?? []).map(item => this.request('callHierarchy/outgoingCalls', { item }))) };
  }

  /** 正常关闭后有界等待整个探针进程组退出。 */
  async stop() {
    const before = groupMembers(this.proc.pid);
    let shutdown, shutdownError;
    try { shutdown = await this.request('shutdown', null, 5000); } catch (error) { shutdownError = error.message; }
    if (this.proc.exitCode === null && this.proc.signalCode === null) this.notify('exit');
    let timer;
    let exit = await Promise.race([this.closed, new Promise(resolve => { timer = setTimeout(() => resolve(null), 5000); })]);
    clearTimeout(timer);
    if (!exit) {
      try { process.kill(-this.proc.pid, 'SIGTERM'); } catch {}
      exit = await this.closed;
    }
    const immediatelyRemaining = groupMembers(this.proc.pid);
    let remaining = immediatelyRemaining;
    for (let attempt = 0; remaining.length && attempt < 20; attempt++) {
      await new Promise(resolve => setTimeout(resolve, 50));
      remaining = groupMembers(this.proc.pid);
    }
    if (remaining.length) { try { process.kill(-this.proc.pid, 'SIGTERM'); } catch {} }
    return { shutdown, shutdownError, exit, before, immediatelyRemaining, remaining, stderr: this.stderr };
  }
}
