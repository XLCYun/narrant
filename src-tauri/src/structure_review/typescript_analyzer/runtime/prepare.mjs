import fs from 'node:fs';
import path from 'node:path';
import { createRequire } from 'node:module';
import { fileURLToPath } from 'node:url';
import { createWorkspace, checkedWorkspacePath, sourcePattern } from './workspace.mjs';
import { prepareConfigurations, inferredOptions } from './config.mjs';

/** 从原始 Git workspace 读取文件与配置，只输出路径和项目描述，不复制仓库文件。 */
export function prepare(input, ts) {
  const context = createWorkspace(input);
  const selected = input.selected.map(file => {
    checkedWorkspacePath(context.root, file);
    if (!sourcePattern.test(file)) throw new Error(`非 TS/JS 输入：${file}`);
    return fs.realpathSync(file);
  });
  const sources = [...context.sources].sort();
  const { host, projects, candidates, configFiles } = prepareConfigurations(context, ts, sources);
  const associations = selected.map((file, index) => {
    if (!context.sources.has(file)) throw new Error(`选中源码不属于 workspace：${file}`);
    let names = candidates.get(file) ?? [];
    if (!names.length) {
      const name = path.join(context.root, `.narrant-inferred-${index}`);
      projects.push({ name, roots: [file], options: inferredOptions(context, file) });
      names = [name];
    }
    return { file, projects: names };
  });
  return { host, context, output: { projects, associations, sources, metadata: [...context.metadata].sort(), config_files: configFiles } };
}

/** 助手作为独立工具执行时只写自己的输出文件；服务加载模块时不重复执行入口。 */
function main() {
  const input = JSON.parse(fs.readFileSync(process.argv[2], 'utf8'));
  const require = createRequire(import.meta.url);
  const ts = require(path.join(input.runtime, 'typescript/lib/typescript.js'));
  if (ts.version !== '6.0.3') throw new Error('固定 TypeScript 版本不匹配');
  fs.writeFileSync(input.output, JSON.stringify(prepare(input, ts).output));
}
if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try { main(); } catch (error) { console.error(error.stack); process.exitCode = 1; }
}
