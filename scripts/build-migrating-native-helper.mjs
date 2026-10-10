import fs from 'node:fs/promises';
import path from 'node:path';
import { execFileSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';

const repository = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');

/** 固定迁移入口，不能用调用参数注入任意 crate、命令或输出程序名。 */
export function migratingHelperEntry(id, windows = process.platform === 'win32') {
  if (id === 'typescript-javascript') return { source: 'native-helpers/typescript', entry: 'runtime/helper/main.mjs', program: windows ? 'bin/node.exe' : 'bin/node' };
  if (id === 'rust' || id === 'python') return { source: `native-helpers/${id}`, entry: `bin/narrant-${id}${windows ? '.exe' : ''}`, binary: `narrant-${id}` };
  throw new Error(`Unknown migrating native helper: ${id}`);
}

/** 构建完成的协议入口到暂存目录；只有核心库时明确失败，绝不生成支持包或改版本。 */
export async function buildMigratingNativeHelper(id, output) {
  const definition = migratingHelperEntry(id);
  const source = path.join(repository, definition.source);
  const payload = path.resolve(output);
  if (id === 'typescript-javascript') {
    // Node 与固定 TypeScript 声明仍由现有归档构建器提供，本入口只暂存协议实现。
    await fs.access(path.join(source, 'main.mjs')).catch(() => { throw new Error('TypeScript protocol helper is not implemented'); });
    await fs.mkdir(path.dirname(path.join(payload, definition.entry)), { recursive: true });
    for (const name of ['main.mjs', 'analyze.mjs', 'projection.mjs', 'workspace.mjs', 'config.mjs']) await fs.copyFile(path.join(source, name), path.join(payload, 'runtime/helper', name));
  } else {
    const manifest = await fs.readFile(path.join(source, 'Cargo.toml'), 'utf8');
    if (!manifest.includes(`name = "${definition.binary}"`)) throw new Error(`${id} has only a core proof; protocol binary ${definition.binary} is not implemented`);
    execFileSync('bash', [path.join(repository, 'scripts/cargo-agent.sh'), 'build', '--locked', '--release', '--bin', definition.binary], { cwd: source, stdio: 'inherit' });
    await fs.mkdir(path.dirname(path.join(payload, definition.entry)), { recursive: true });
    await fs.copyFile(path.join(source, 'target/release', path.basename(definition.entry)), path.join(payload, definition.entry));
    await fs.chmod(path.join(payload, definition.entry), 0o755);
  }
  return definition;
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  if (!process.argv[2] || !process.argv[3]) throw new Error('Usage: build-migrating-native-helper.mjs <language> <staging-directory>');
  await buildMigratingNativeHelper(process.argv[2], process.argv[3]);
}
