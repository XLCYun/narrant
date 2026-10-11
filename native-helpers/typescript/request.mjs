import path from 'node:path';

/** 在整体 JSON.parse 之前逐层检查键；转义拼写不同的同名键也不能被覆盖。 */
function rejectDuplicateKeys(text) {
  const token = /\s*("(?:[^"\\\u0000-\u001f]|\\(?:["\\/bfnrt]|u[0-9a-fA-F]{4}))*"|-?(?:0|[1-9]\d*)(?:\.\d+)?(?:[eE][+-]?\d+)?|true|false|null|[{}\[\],:])/y;
  let offset = 0;
  /** 读取一个严格 JSON 词法单元，拒绝未识别内容。 */
  function next() {
    token.lastIndex = offset;
    const match = token.exec(text);
    if (!match) throw new SyntaxError('无效 JSON');
    offset = token.lastIndex;
    return match[1];
  }
  /** 有限递归只保留当前对象的键集合，不构造另一份协议对象。 */
  function value(first, depth = 0) {
    if (depth > 64) throw new SyntaxError('JSON 嵌套过深');
    if (first === '{') {
      const keys = new Set();
      let key = next();
      if (key === '}') return;
      while (true) {
        if (!key.startsWith('"')) throw new SyntaxError('无效 JSON 对象键');
        const decoded = JSON.parse(key);
        if (keys.has(decoded)) throw new SyntaxError(`重复 JSON 键：${decoded}`);
        keys.add(decoded);
        if (next() !== ':') throw new SyntaxError('无效 JSON 对象');
        value(next(), depth + 1);
        const end = next();
        if (end === '}') return;
        if (end !== ',') throw new SyntaxError('无效 JSON 对象');
        key = next();
      }
    }
    if (first === '[') {
      let item = next();
      if (item === ']') return;
      while (true) {
        value(item, depth + 1);
        const end = next();
        if (end === ']') return;
        if (end !== ',') throw new SyntaxError('无效 JSON 数组');
        item = next();
      }
    }
    if (/^[}\],:]$/.test(first)) throw new SyntaxError('无效 JSON 值');
  }
  value(next());
  if (text.slice(offset).trim()) throw new SyntaxError('JSON 尾部内容');
}

/** 所有字段都必须出现且不得扩展；数组和 null 不能冒充对象。 */
function fields(object, names) {
  if (!object || typeof object !== 'object' || Array.isArray(object)
    || Object.keys(object).length !== names.length || !names.every(name => Object.hasOwn(object, name))) throw new Error(`无效协议字段：${names.join(',')}`);
}
/** 协议字符串必须能无损表达路径和身份，拒绝 NUL。 */
function string(value) {
  if (typeof value !== 'string' || !value || value.includes('\0')) throw new Error('无效协议字符串');
}
/** 目录使用宿主绝对路径；真实来源和链接仍交由已有读取政策校验。 */
function directory(value) {
  string(value);
  if (!path.isAbsolute(value)) throw new Error('协议目录必须为绝对路径');
}

/** 单次 analyze/cancel 的严格解码，仅验证输入契约，不重复 Host 的图验证。 */
export function decodeRequest(line, activeId) {
  rejectDuplicateKeys(line);
  const message = JSON.parse(line);
  if (!message || message.protocol !== 'narrant.native-analyzer/1') throw new Error('无效分析协议');
  if (activeId !== undefined) {
    if (message.type !== 'cancel' || message.id !== activeId) throw new Error('无效或重复分析控制');
    fields(message, ['protocol', 'type', 'id']);
    string(message.id);
    return message;
  }
  if (message.type !== 'analyze') throw new Error('无效分析请求');
  fields(message, ['protocol', 'type', 'id', 'input', 'environment', 'limits']);
  string(message.id);
  fields(message.input, ['repository_root', 'files', 'include_tests', 'profile']);
  directory(message.input.repository_root);
  if (typeof message.input.include_tests !== 'boolean' || message.input.profile !== 'typescript-static-v1') throw new Error('无效 TypeScript 输入政策');
  if (!Array.isArray(message.input.files)) throw new Error('无效协议文件数组');
  for (const file of message.input.files) {
    string(file);
    if (file.includes('\\') || path.posix.isAbsolute(file) || path.win32.isAbsolute(file) || file.split('/').some(part => !part || part === '..' || part === '.')) throw new Error('无效协议相对文件路径');
  }
  if (new Set(message.input.files).size !== message.input.files.length) throw new Error('重复协议文件路径');
  fields(message.environment, ['scratch_root', 'platforms']);
  directory(message.environment.scratch_root);
  if (!Array.isArray(message.environment.platforms)) throw new Error('无效协议平台数组');
  const ids = new Set();
  for (const platform of message.environment.platforms) {
    fields(platform, ['id', 'kind', 'version', 'root', 'metadata']);
    string(platform.id); string(platform.version); directory(platform.root);
    if (ids.has(platform.id)) throw new Error('重复平台 ID');
    ids.add(platform.id);
    fields(platform.metadata, ['package']);
    if (platform.kind !== 'typescript-declarations' || !['typescript', '@types/node', 'undici-types'].includes(platform.metadata.package)) throw new Error('无效 TypeScript 声明平台');
  }
  fields(message.limits, ['analysis_timeout_ms', 'max_result_bytes', 'max_diagnostic_bytes']);
  if (!Object.values(message.limits).every(value => Number.isSafeInteger(value) && value > 0)) throw new Error('协议限制必须为正整数');
  return message;
}
