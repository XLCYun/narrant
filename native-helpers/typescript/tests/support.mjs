import fs from 'node:fs';
import path from 'node:path';
import { createRequire } from 'node:module';
import { createWorkspace } from '../workspace.mjs';
import { prepareConfigurations } from '../config.mjs';

export const runtime = fs.realpathSync.native(path.join(process.env.NARRANT_TYPESCRIPT_PACK ?? (() => { throw new Error('NARRANT_TYPESCRIPT_PACK required'); })(), 'runtime'));
export const ts = createRequire(import.meta.url)(path.join(runtime, 'typescript/lib/typescript.js'));
/** 旧配置/文件系统断言直接验证迁移后的原生 host，不创建旧 prepare 进程。 */
export function prepare(input) {
  const context = createWorkspace(input);
  const sources = [...context.sources].sort();
  const { host, configFiles } = prepareConfigurations(context, ts, sources);
  return { host, context, output: { sources, config_files: configFiles } };
}
