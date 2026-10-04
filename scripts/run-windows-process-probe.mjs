import { spawn } from 'node:child_process';
import { stat } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const repository = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');

/** 运行同源所有者回归；保留既有入口名称，不再执行调查专用的重复 Job 实现。 */
export async function runWindowsProcessProbe({ cwd = repository, platform = process.platform } = {}) {
  if (platform !== 'win32') throw new Error('Windows process validation requires a Windows runner');
  const node = process.execPath;
  const exists = (await stat(node)).isFile();
  console.log(`[windows-process-probe-launch] ${JSON.stringify({ node, absolute: path.isAbsolute(node), exists })}`);
  if (!path.isAbsolute(node) || !exists) throw new Error('Current Node executable must be an existing absolute file');
  const child = spawn('bash', ['scripts/cargo-agent.sh', 'test', '--manifest-path', 'native-helpers/windows-process-probe/Cargo.toml', '--locked', '--lib', 'child_process::', '--', '--nocapture', '--test-threads=1'], {
    cwd,
    stdio: 'inherit',
    env: { ...process.env, NARRANT_TEST_NODE: node },
  });
  return await new Promise((resolve, reject) => {
    child.once('error', reject);
    child.once('close', (code, signal) => signal ? reject(new Error(`Windows process probe interrupted: ${signal}`)) : resolve(code ?? 1));
  });
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) process.exitCode = await runWindowsProcessProbe();
