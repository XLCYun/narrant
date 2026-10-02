import fs from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import { spawn, execFileSync } from 'node:child_process';
import { pathToFileURL, fileURLToPath } from 'node:url';

/** 在目标平台上验证固定包的真实 LSP 初始化、仓库/标准库查询与退出，不运行 Python 源码。 */
export async function verifyPythonRuntime(executable, configurationFile) {
  const temporary = await fs.mkdtemp(path.join(os.tmpdir(), 'narrant-python-native-'));
  const repository = path.join(temporary, 'repository');
  const control = path.join(temporary, 'control');
  const cache = path.join(control, 'cache');
  const prefix = path.join(control, 'python');
  for (const directory of [repository, cache, path.join(control, 'config'), path.join(control, 'bin'), path.join(prefix, 'lib/python3.14/site-packages'), path.join(prefix, 'Lib/site-packages')]) await fs.mkdir(directory, { recursive: true });
  const source = 'from api import value\nvalue()\nlen([])\n';
  const clientFile = path.join(repository, 'client.py');
  await fs.writeFile(clientFile, source);
  await fs.writeFile(path.join(repository, 'api.py'), 'def value():\n    return 1\n');
  const env = Object.fromEntries(Object.entries(process.env).filter(([name]) => /^(SystemRoot|WINDIR|SystemDrive)$/i.test(name)));
  Object.assign(env, { PATH: path.join(control, 'bin'), XDG_CONFIG_HOME: path.join(control, 'config'), XDG_CACHE_HOME: cache, APPDATA: path.join(control, 'config'), LOCALAPPDATA: cache, HOME: control, USERPROFILE: control, TMPDIR: control, TMP: control, TEMP: control });
  const child = spawn(executable, ['server'], { cwd: repository, env, stdio: ['pipe', 'pipe', 'pipe'], detached: process.platform !== 'win32', windowsHide: true });
  const pending = new Map();
  let buffer = Buffer.alloc(0), sequence = 0, fatal, stderr = '';
  const exited = new Promise(resolve => child.once('close', (code, signal) => resolve({ code, signal })));
  /** 协议错误立即结束所有等待，不让损坏响应变成成功空结果。 */
  const fail = error => { fatal = error; for (const item of pending.values()) item.reject(error); pending.clear(); };
  child.once('error', fail);
  child.stderr.on('data', bytes => { stderr = (stderr + bytes.toString()).slice(-4096); });
  /** 发送标准 Content-Length 帧，不依赖任何 Python/Node 包运行时。 */
  const send = message => { const bytes = Buffer.from(JSON.stringify(message)); child.stdin.write(`Content-Length: ${bytes.length}\r\n\r\n`); child.stdin.write(bytes); };
  child.stdin.on('error', fail);
  child.stdout.on('data', bytes => {
    try {
      buffer = Buffer.concat([buffer, bytes]);
      while (true) {
        const boundary = buffer.indexOf('\r\n\r\n');
        if (boundary < 0) break;
        const match = /^Content-Length: (\d+)$/im.exec(buffer.subarray(0, boundary).toString());
        if (!match || Number(match[1]) > 20_000_000) throw new Error('Invalid Python LSP frame');
        const end = boundary + 4 + Number(match[1]);
        if (buffer.length < end) break;
        const message = JSON.parse(buffer.subarray(boundary + 4, end).toString()); buffer = buffer.subarray(end);
        if (message.method && message.id !== undefined) send({ jsonrpc: '2.0', id: message.id, error: { code: -32601, message: 'Unsupported request' } });
        else if (pending.has(message.id)) {
          const item = pending.get(message.id); pending.delete(message.id);
          if (message.error || !Object.hasOwn(message, 'result')) item.reject(new Error(`Python LSP error: ${JSON.stringify(message)}`));
          else item.resolve(message.result);
        }
      }
    } catch (error) { fail(error); }
  });
  child.once('close', () => { if (pending.size) fail(new Error(`Python LSP exited with pending requests: ${stderr}`)); });
  /** 每次请求有固定截止时间，进度通知不会延长它。 */
  const request = async (method, params) => {
    if (fatal) throw fatal;
    const id = ++sequence;
    let timeout;
    try {
      return await new Promise((resolve, reject) => {
        timeout = setTimeout(() => { pending.delete(id); reject(new Error(`Python LSP timed out: ${method}`)); }, 10_000);
        pending.set(id, { resolve, reject }); send({ jsonrpc: '2.0', id, method, params });
      });
    } finally { clearTimeout(timeout); }
  };
  try {
    const uri = pathToFileURL(repository).href;
    const initialized = await request('initialize', { processId: process.pid, rootUri: uri, workspaceFolders: [{ uri, name: 'native-check' }], capabilities: { general: { positionEncodings: ['utf-16'] }, workspace: { configuration: false }, textDocument: { definition: { linkSupport: true } } }, initializationOptions: { untrustedWorkspace: true, experimental: { useUv: 'off' }, diagnosticMode: 'off', configurationFile, configuration: { environment: { root: [repository], 'extra-paths': [], python: prefix, 'python-version': '3.14', 'python-platform': 'all' } } } });
    if (!initialized?.capabilities?.definitionProvider) throw new Error('Python pack lacks definition support');
    send({ jsonrpc: '2.0', method: 'initialized', params: {} });
    send({ jsonrpc: '2.0', method: 'textDocument/didOpen', params: { textDocument: { uri: pathToFileURL(clientFile).href, languageId: 'python', version: 1, text: source } } });
    for (const [line, expected] of [[1, 'repository'], [2, 'stdlib']]) {
      const definitions = await request('textDocument/definition', { textDocument: { uri: pathToFileURL(clientFile).href }, position: { line, character: 0 } });
      if (!Array.isArray(definitions) || definitions.length === 0) throw new Error(`Python pack cannot resolve ${expected}`);
      const root = await fs.realpath(expected === 'repository' ? repository : cache);
      for (const definition of definitions) {
        const target = await fs.realpath(fileURLToPath(definition.uri ?? definition.targetUri));
        const relative = path.relative(root, target);
        if (relative.startsWith('..') || path.isAbsolute(relative)) throw new Error(`Python pack ${expected} target escaped its boundary`);
        if (expected === 'repository' && path.basename(target) !== 'api.py') throw new Error('Python repository query returned an import anchor');
      }
    }
    await request('shutdown', null);
    send({ jsonrpc: '2.0', method: 'exit', params: null });
    let timeout;
    try {
      const result = await Promise.race([exited, new Promise((_, reject) => { timeout = setTimeout(() => reject(new Error('Python LSP did not exit')), 5_000); })]);
      if (result.code !== 0) throw new Error(`Python LSP exit failed: ${JSON.stringify(result)} ${stderr}`);
    } finally { clearTimeout(timeout); }
    console.log('Verified Python LSP repository + stdlib definitions and clean exit');
  } finally {
    if (child.exitCode === null && child.signalCode === null && child.pid) {
      if (process.platform === 'win32') { try { execFileSync('taskkill', ['/pid', String(child.pid), '/T', '/F'], { stdio: 'ignore' }); } catch { child.kill(); } }
      else { try { process.kill(-child.pid, 'SIGKILL'); } catch { child.kill('SIGKILL'); } }
      await exited;
    }
    await fs.rm(temporary, { recursive: true, force: true });
  }
}
