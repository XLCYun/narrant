import fs from 'node:fs';
import path from 'node:path';
import { createRequire } from 'node:module';
import { prepare } from './prepare.mjs';

/** 官方 tsserver 使用同一 workspace 的读取接口；只在进程内覆盖配置与模块查找。 */
const input = JSON.parse(fs.readFileSync(new URL('./input.json', import.meta.url), 'utf8'));
const require = createRequire(import.meta.url);
const ts = require(path.join(input.runtime, 'typescript/lib/typescript.js'));
if (ts.version !== '6.0.3') throw new Error('固定 TypeScript 版本不匹配');
const { host } = prepare(input, ts);
Object.assign(ts.sys, host);
require(path.join(input.runtime, 'typescript/lib/tsserver.js'));
