import fs from 'node:fs/promises';
import path from 'node:path';
import os from 'node:os';
import { spawn } from 'node:child_process';
import { extract } from 'tar';

const target = ({ 'darwin-arm64': 'aarch64-apple-darwin', 'win32-x64': 'x86_64-pc-windows-msvc' })[`${process.platform}-${process.arch}`];
if (!target) throw new Error('Python 原生验收需要 macOS ARM64 或 Windows x64');
const artifacts = path.resolve(process.argv[2] ?? 'dist/language-support-packs');
const versions = JSON.parse(await fs.readFile('resources/language-support-packs.json', 'utf8'));
const pack = path.join(artifacts, 'python', versions.packs.python.version, target);
const temporary = await fs.mkdtemp(path.join(os.tmpdir(), 'narrant-python-native-'));

/** 每条 Cargo 命令都经仓库锁；完成当前用例后才启动下一组。 */
async function verify(filter) {
  const child = spawn('bash', ['scripts/cargo-agent.sh', 'test', '--manifest-path', 'src-tauri/Cargo.toml', '--lib', filter, '--', '--include-ignored', '--nocapture', '--test-threads=4'], {
    stdio: 'inherit', env: { ...process.env, ...(process.platform === 'win32' ? { NARRANT_WINDOWS_TEST_MANIFEST: '1' } : {}), NARRANT_TY_PATH: path.join(temporary, 'bin', process.platform === 'win32' ? 'ty.exe' : 'ty'), NARRANT_LANGUAGE_PACK_FIXTURE: artifacts },
  });
  await new Promise((resolve, reject) => {
    child.once('error', reject);
    child.once('close', code => code === 0 ? resolve() : reject(new Error(`Python 原生验收失败：${filter} (${code})`)));
  });
}
try {
  // 只解开本地构建产物；实际安装测试另经生产的摘要、入口和租约验证。
  await extract({ file: path.join(pack, 'pack.tar.gz'), cwd: temporary, strict: true });
  for (const filter of ['python_analyzer::', 'language_support::python_tests::', 'application::tests::python::']) await verify(filter);
} finally { await fs.rm(temporary, { recursive: true, force: true }); }
