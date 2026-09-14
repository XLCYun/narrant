import fs from 'node:fs';
import path from 'node:path';
import { createRequire } from 'node:module';
import { copyRepository, mapped, within, sourcePattern } from './view.mjs';
import { prepareConfigurations, inferredOptions } from './config.mjs';

/** 只接收宿主写出的明确输入；结果写专用文件，stdout 不作为宽松协议。 */
function main() {
  const input = JSON.parse(fs.readFileSync(process.argv[2], 'utf8'));
  const { root, view, runtime, selected } = input;
  const notes = [];
  const context = { root, view, runtime, note(file, message) { if (notes.length < 200) notes.push({ file, message: message.slice(0, 4096) }); } };
  if (!path.isAbsolute(root) || !path.isAbsolute(view) || !path.isAbsolute(runtime) || within(root, view)) throw new Error('无效的项目准备目录');
  for (const file of selected) {
    mapped(context, file);
    if (!sourcePattern.test(file)) throw new Error(`非 TS/JS 输入：${file}`);
  }
  const { sources, metadata } = copyRepository(context);
  const require = createRequire(import.meta.url);
  const ts = require(path.join(runtime, 'typescript/lib/typescript.js'));
  if (ts.version !== '6.0.3') throw new Error('固定 TypeScript 版本不匹配');
  const { projects, candidates, configFiles } = prepareConfigurations(context, ts, sources);
  for (const name of ['@types/node', 'undici-types']) {
    const destination = path.join(view, 'node_modules', name);
    fs.mkdirSync(path.dirname(destination), { recursive: true });
    if (fs.existsSync(destination)) throw new Error(`运行时类型名称被第一方包占用：${name}`);
    // 复制固定声明，避免符号链接实路径解析绕过视图中的 undici 闭包。
    fs.cpSync(path.join(runtime, name), destination, { recursive: true, dereference: true });
  }
  const associations = [];
  for (const original of selected) {
    const file = mapped(context, original);
    if (!sources.some(source => source.view === file)) throw new Error(`选中源码未进入视图：${original}`);
    let names = candidates.get(file) ?? [];
    if (!names.length) {
      const name = path.join(view, `.narrant-inferred-${associations.length}`);
      projects.push({ name, roots: [file], options: inferredOptions(context, file) });
      names = [name];
    }
    associations.push({ original, view: file, projects: names });
  }
  fs.writeFileSync(input.output, JSON.stringify({ projects, associations, sources, metadata, notes, config_files: configFiles }));
}
try { main(); } catch (error) { console.error(error.stack); process.exitCode = 1; }
