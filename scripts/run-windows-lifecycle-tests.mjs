import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { runVerificationCommands } from './verify-native-analyzers.mjs';

const repository = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');

/** 同源轻量 crate 验收真实 Windows 行为；拒绝其他平台的零测试结果。 */
export async function runWindowsLifecycleTests({ cwd = repository, platform = process.platform } = {}) {
  if (platform !== 'win32') throw new Error('Windows lifecycle validation requires a Windows runner');
  const manifest = ['--manifest-path', 'native-helpers/windows-process-probe/Cargo.toml', '--locked'];
  const commands = [
    { executable: 'bash', args: ['scripts/cargo-agent.sh', 'test', ...manifest, '--lib', 'child_process::', '--', '--nocapture', '--test-threads=1'], cwd },
  ];
  // 直接运行仍在生产使用的共享所有者；真实 helper 协议由完整原生验收覆盖。
  await runVerificationCommands(commands, { ...process.env, NARRANT_TEST_NODE: process.execPath });
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) await runWindowsLifecycleTests();
