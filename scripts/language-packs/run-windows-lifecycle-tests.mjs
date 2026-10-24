import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { runWindowsProcessProbe } from './run-windows-process-probe.mjs';

const repository = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../..');

/** 应用共享进程模块验收真实 Windows 行为；拒绝其他平台的零测试结果。 */
export async function runWindowsLifecycleTests({ cwd = repository, platform = process.platform } = {}) {
  if (platform !== 'win32') throw new Error('Windows lifecycle validation requires a Windows runner');
  const code = await runWindowsProcessProbe({ cwd, platform });
  if (code !== 0) throw new Error(`Windows lifecycle validation failed (${code})`);
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) await runWindowsLifecycleTests();
