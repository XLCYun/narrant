import { spawn } from 'node:child_process';
import { stat } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const repository = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');

/** 使用当前 Node 的单一原生路径交接给 Cargo，避免 PowerShell 命令发现结果参与环境拼接。 */
export async function runWindowsProcessProbe({ cwd = repository } = {}) {
  const node = process.execPath;
  const exists = (await stat(node)).isFile();
  console.log(`[windows-process-probe-launch] ${JSON.stringify({ node, absolute: path.isAbsolute(node), exists })}`);
  if (!path.isAbsolute(node) || !exists) throw new Error('Current Node executable must be an existing absolute file');
  const child = spawn('bash', ['scripts/cargo-agent.sh', 'run', '--manifest-path', 'native-helpers/windows-process-probe/Cargo.toml', '--locked'], {
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
