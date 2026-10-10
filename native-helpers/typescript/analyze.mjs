import fs from 'node:fs';
import path from 'node:path';
import { createWorkspace, within, checkedWorkspacePath } from './workspace.mjs';
import { prepareConfigurations, inferredOptions } from './config.mjs';
import { project, normalize, unwrap, isTest } from './projection.mjs';

/** 一侧一次固定请求：同一个 LanguageService 同时提供语法树、类型诊断和定义导航。 */
export function analyze(ts, request) {
  const input = request.input;
  if (input.profile !== 'typescript-static-v1') throw new Error('不支持的 TypeScript profile');
  const platforms = request.environment.platforms;
  const library = platforms.find(p => p.metadata.package === 'typescript');
  if (!library || ts.version !== '6.0.3') throw new Error('需要固定 TypeScript 6.0.3 与声明平台');
  const libraryRoot = fs.realpathSync(library.root);
  const runtime = path.dirname(path.dirname(libraryRoot));
  for (const name of ['typescript', '@types/node', 'undici-types']) {
    const entries = platforms.filter(p => p.kind === 'typescript-declarations' && p.metadata.package === name);
    const expected = path.join(runtime, name, name === 'typescript' ? 'lib' : '');
    if (entries.length !== 1 || fs.realpathSync(entries[0].root) !== fs.realpathSync(expected)) throw new Error('声明 catalog 与固定运行时目录不一致');
  }
  const root = fs.realpathSync(input.repository_root);
  const selected = input.files.map(file => fs.realpathSync(checkedWorkspacePath(root, path.join(root, file))));
  if (selected.some(file => !fs.statSync(file).isFile())) throw new Error('选集必须是普通源码文件');
  // 旧入口先校验路径，再短路空选集/全词法测试选集；不读取无关坏配置。
  if (!selected.length || !input.include_tests && input.files.every(isTest)) {
    const id = 'excluded:typescript-tests';
    return {
      graph: { root_contains: [], nodes: [], external_symbols: [] },
      evidence: {
        configurations: selected.length ? [{ id, source_paths: [], platform_ids: [] }] : [],
        files: input.files.map(file => ({ path: file, disposition: 'excluded_test', configuration_ids: [id] })),
        origins: [],
      },
    };
  }
  const context = createWorkspace({ root, runtime });
  const { host, projects, candidates, configFiles } = prepareConfigurations(context, ts, [...context.sources]);
  for (const file of selected) {
    if (!context.sources.has(fs.realpathSync(file))) throw new Error(`选集不是受控源码：${file}`);
    if (!candidates.get(file)?.length) {
      const converted = ts.convertCompilerOptionsFromJson(inferredOptions(context, file), context.root);
      if (converted.errors.length) throw new Error("默认项目配置转换失败");
      projects.push({ name: `inferred:${file}`, roots: [file], options: converted.options });
      candidates.set(file, [`inferred:${file}`]);
    }
  }
  const documents = new Map(), services = new Map();
  const registry = ts.createDocumentRegistry();
  /** 服务宿主关闭写入、监听、自动类型获取，并允许原生项目引用重定向到源码。 */
  function service(projectName) {
    if (services.has(projectName)) return services.get(projectName);
    const config = projects.find(p => p.name === projectName);
    const options = { ...config.options, noEmit: true, disableSourceOfProjectReferenceRedirect: false };
    const languageHost = {
      ...host, getCompilationSettings: () => options,
      getScriptFileNames: () => config.roots,
      getScriptVersion: () => '0', getProjectVersion: () => '0',
      getScriptSnapshot: file => { const text = host.readFile(file); return text === undefined ? undefined : ts.ScriptSnapshot.fromString(text); },
      getDefaultLibFileName: opts => path.join(libraryRoot, ts.getDefaultLibFileName(opts)),
      getProjectReferences: () => config.references,
      useSourceOfProjectReferenceRedirect: () => true,
      getParsedCommandLine: file => ts.getParsedCommandLineOfConfigFile(file, {}, { ...host, onUnRecoverableConfigFileDiagnostic: () => { throw new Error('引用项目配置无效'); } }),
    };
    const ls = ts.createLanguageService(languageHost, registry);
    const value = { ls, program: ls.getProgram(), diagnostics: new Map() };
    if (!value.program) throw new Error('TypeScript 无法加载项目');
    services.set(projectName, value); return value;
  }
  /** 同一服务 AST 保持原始 UTF-16 位置；只投影实际选集与导航命中的上下文。 */
  function document(svc, file) {
    file = host.realpath(file);
    if (documents.has(file)) return documents.get(file);
    const sf = svc.program.getSourceFile(file) ?? svc.program.getSourceFiles().find(f => host.realpath(f.fileName) === file);
    if (!sf) throw new Error(`导航目标不在已加载项目中：${file}`);
    if (sf.parseDiagnostics.length || svc.program.getSyntacticDiagnostics(sf).length) throw new Error(`源码无法解析：${path.relative(context.root, file)}`);
    // 固定 TS 6.0.3 的语义诊断入口先完成绑定，再读取本文件 bindDiagnostics。
    // 仅保留旧早期语法错误；跨文件 2451 和其他一般类型错误不能阻止图提取。
    const diagnostics = svc.program.getSemanticDiagnostics(sf);
    if (diagnostics.some(d => [1155, 1107].includes(d.code)) || sf.bindDiagnostics.some(d => d.code === 2451)) throw new Error(`源码无法解析：${path.relative(context.root, file)}`);
    const runtime = !within(context.root, file);
    if (!runtime) new TextDecoder('utf-8', { fatal: true }).decode(fs.readFileSync(file));
    const doc = project(ts, sf, runtime ? context.runtime : context.root, runtime);
    documents.set(file, doc); return doc;
  }
  const output = [], origins = [], files = [], originById = new Map();
  for (let i = 0; i < selected.length; i++) {
    const file = selected[i], relative = input.files[i];
    const configs = candidates.get(file);
    const excluded = !input.include_tests && (isTest(relative) || isTest(path.relative(context.root, file).split(path.sep).join('/')));
    files.push({ path: relative, disposition: excluded ? 'excluded_test' : 'analyzed', configuration_ids: configs });
    if (excluded) continue;
    const doc = document(service(configs[0]), file);
    for (const declaration of doc.declarations) {
      if (originById.has(declaration.node.id)) originById.get(declaration.node.id).selected_paths.push(relative);
      else {
        const origin = { node_id: declaration.node.id, selected_paths: [relative] };
        output.push(declaration.node); origins.push(origin); originById.set(declaration.node.id, origin);
      }
    }
  }
  const membership = new Set(output.map(n => n.id)), external = new Map();
  /** 导航命中声明名或直接初始化器才有目标证据，不能以邻近源码猜测。 */
  function declared(doc, start, length) {
    return doc.declarations.find(d => d.parts.some(p => p.selection && p.selection.getStart(doc.sf) <= start && start + length <= p.selection.end)
      || d.callable && d.callable.getStart(doc.sf) === start && d.callable.end === start + length
      || d.node.name === null && d.parts.some(p => p.ast.getStart(doc.sf) === start));
  }
  /** 运行时类型诊断保留旧边界：type-only 绑定不能因导航可达而成为调用。 */
  function typeOnlyError(svc, file, position) {
    if (!svc.diagnostics.has(file)) svc.diagnostics.set(file, svc.ls.getSemanticDiagnostics(file).filter(d => [1361, 1362, 2693, 2702].includes(d.code)));
    return svc.diagnostics.get(file).some(d => d.start <= position && position < d.start + d.length);
  }
  /** 找到导航范围所属 AST，供 const/import/export 别名续查。 */
  function astAt(sf, position) {
    let found = sf;
    const visit = node => { if (node.getStart(sf) <= position && position < node.end) { found = node; ts.forEachChild(node, visit); } };
    visit(sf); return found;
  }
  /** 导航别名只沿常量直接绑定和模块别名，不把 mutable/参数推测为确定函数。 */
  function follow(svc, file, start, length, runtimeUse, seen = new Set()) {
    file = host.realpath(file);
    const key = `${file}:${start}`;
    if (seen.has(key) || seen.size >= 32) return null;
    seen = new Set(seen).add(key);
    const doc = document(svc, file);
    const target = declared(doc, start, length);
    if (target) return [{ target, runtime: doc.runtime }];
    let node = astAt(doc.sf, start), query;
    for (let current = node; current && !ts.isSourceFile(current); current = current.parent) {
      if (runtimeUse && (current.isTypeOnly || ts.isImportClause(current) && current.isTypeOnly)) return null;
      if (ts.isVariableDeclaration(current)) {
        if (!(current.parent.flags & ts.NodeFlags.Const) || !current.initializer) return null;
        const init = unwrap(ts, current.initializer);
        if (!(ts.isIdentifier(init) || ts.isPropertyAccessExpression(init))) return null;
        query = ts.isPropertyAccessExpression(init) ? init.name : init; break;
      }
      if (ts.isImportSpecifier(current) || ts.isExportSpecifier(current)) { query = current.propertyName ?? current.name; break; }
      if (ts.isImportClause(current) || ts.isNamespaceImport(current) || ts.isImportEqualsDeclaration(current)) { query = current.name; break; }
    }
    if (!query || runtimeUse && typeOnlyError(svc, file, query.getStart(doc.sf))) return null;
    const defs = svc.ls.getDefinitionAtPosition(file, query.getStart(doc.sf)) ?? [];
    if (!defs.length) return null;
    const results = defs.map(d => follow(svc, d.fileName, d.textSpan.start, d.textSpan.length, runtimeUse, seen));
    return results.every(Boolean) ? results.flat() : null;
  }
  /** 调用只接受实现/ambient 函数；一般类型使用仅针对字段和属性。 */
  function accepts({ target: d, runtime }, kind) {
    const callable = Boolean(d.callable || d.node.category === 'callable' && !['getter', 'setter'].includes(d.node.native_kind) && (d.parts.some(p => p.body) || d.ambient || runtime));
    if (kind === 'construct') return d.node.native_kind === 'class' || d.node.native_kind === 'constructor' && callable;
    if (kind === 'jsx') return callable || d.node.native_kind === 'class';
    if (['extends', 'implements', 'type'].includes(kind)) return d.node.category === 'type';
    return callable;
  }
  /** 检查原生符号的每一层模块路由，nocheck 不会把 type-only 绑定变成值。 */
  function typeOnlyRoute(checker, expression, seen = new Set()) {
    if (!expression) return false;
    expression = unwrap(ts, expression);
    if (ts.isPropertyAccessExpression(expression) && typeOnlyRoute(checker, expression.expression, seen)) return true;
    const symbol = checker.getSymbolAtLocation(ts.isPropertyAccessExpression(expression) ? expression.name : expression);
    /** 保留 import/export 别名节点的 type-only 信息，再沿原生模块导出继续。 */
    function inspect(symbol) {
      if (!symbol || seen.has(symbol)) return false;
      seen.add(symbol);
      for (const d of symbol.declarations ?? []) {
        let container = d;
        while (container && !ts.isSourceFile(container)) {
          if (container.isTypeOnly) return true;
          if (ts.isImportDeclaration(container) || ts.isExportDeclaration(container)) break;
          container = container.parent;
        }
        if (container?.moduleSpecifier) {
          const moduleSymbol = checker.getSymbolAtLocation(container.moduleSpecifier);
          const original = d.propertyName?.text ?? (ts.isImportClause(d) ? 'default' : d.name?.text);
          if (original && moduleSymbol && inspect(checker.getExportsOfModule(moduleSymbol).find(s => s.name === original))) return true;
        } else if (ts.isExportSpecifier(d) && inspect(checker.getExportSpecifierLocalTargetSymbol(d))) return true;
        else if (ts.isVariableDeclaration(d) && d.initializer && (d.parent.flags & ts.NodeFlags.Const) && typeOnlyRoute(checker, d.initializer, seen)) return true;
      }
      return false;
    }
    return inspect(symbol);
  }
  /** 每个实际项目独立查询；冲突由上层合为 Unresolved。 */
  function resolve(svc, doc, occurrence) {
    const runtimeUse = !['extends', 'implements', 'type'].includes(occurrence.kind);
    const source = svc.program.getSourceFile(doc.sf.fileName);
    let expression = occurrence.expression;
    if (source && source !== doc.sf) {
      const start = expression.getStart(doc.sf), end = expression.end;
      /** 每个项目使用自己的绑定 AST，不能借用另一配置的符号。 */
      const locate = node => {
        if (node.getStart(source) === start && node.end === end) { expression = node; return; }
        if (node.getStart(source) <= start && end <= node.end) ts.forEachChild(node, locate);
      };
      locate(source);
    }
    if (runtimeUse && typeOnlyRoute(svc.program.getTypeChecker(), expression)) return null;
    if (occurrence.dynamic || runtimeUse && typeOnlyError(svc, doc.sf.fileName, occurrence.query.getStart(doc.sf))) return null;
    const defs = svc.ls.getDefinitionAtPosition(doc.sf.fileName, occurrence.query.getStart(doc.sf)) ?? [];
    if (!defs.length) return null;
    const results = defs.map(d => follow(svc, d.fileName, d.textSpan.start, d.textSpan.length, runtimeUse));
    const targets = [...new Map(results.filter(Boolean).flat().map(t => [t.target.node.id, t])).values()];
    const unknown = occurrence.kind === 'reference' && targets.some(t => accepts(t, occurrence.kind)) ? [{ kind: 'unresolved', name: occurrence.name }] : null;
    if (results.some(r => !r) || !targets.length || !targets.every(t => accepts(t, occurrence.kind))) return unknown;
    if (targets.length > 1 && (runtimeUse || !targets.every(t => ['type', 'container'].includes(t.target.node.category) && t.target.node.name === targets[0].target.node.name && t.target.node.native_kind === targets[0].target.node.native_kind))) return unknown;
    return targets.map(({ target: d, runtime }) => {
      if (membership.has(d.node.id)) return { kind: 'node', id: d.node.id };
      external.set(d.node.id, { symbol: d.node.id, name: d.node.name ?? d.node.native_kind, scope: runtime ? 'dependency' : 'repository' });
      return { kind: 'external', symbol: d.node.id };
    }).sort((a, b) => JSON.stringify(a).localeCompare(JSON.stringify(b)));
  }
  const visited = new Set();
  for (let i = 0; i < selected.length; i++) {
    if (files[i].disposition === 'excluded_test' || visited.has(selected[i])) continue;
    const file = selected[i];
    visited.add(file);
    const configs = candidates.get(file), doc = documents.get(host.realpath(file));
    for (const occurrence of occurrences(ts, doc)) {
      const alternatives = configs.map(c => resolve(service(c), doc, occurrence));
      if (occurrence.kind === 'reference' && alternatives.every(a => !a)) continue;
      const conflict = alternatives.some(a => JSON.stringify(a) !== JSON.stringify(alternatives[0]));
      const targets = conflict ? null : alternatives[0];
      const semantic = { call: 'code.calls', construct: 'code.calls', reference: 'code.references', extends: 'code.extends', implements: 'code.implements', type: 'code.type_use', jsx: 'jsx.uses_component' }[occurrence.kind];
      occurrence.owner.node.references.push(...(targets ?? [{ kind: 'unresolved', name: occurrence.name }]).map(target => ({ semantic, target })));
    }
  }
  const used = new Set();
  for (const node of output) {
    node.references = [...new Map(node.references.map(r => [JSON.stringify(r), r])).values()];
    node.references.forEach(r => { if (r.target.kind === 'external') used.add(r.target.symbol); });
  }
  services.forEach(s => s.ls.dispose());
  return {
    graph: { root_contains: output.filter(n => n.native_kind === 'file').map(n => n.id), nodes: output, external_symbols: [...external.values()].filter(s => used.has(s.symbol)) },
    evidence: { configurations: [...new Set(files.flatMap(f => f.configuration_ids))].map(id => ({ id, source_paths: id.startsWith('inferred:') ? [] : configFiles.map(f => path.relative(context.root, f).split(path.sep).join('/')), platform_ids: platforms.map(p => p.id) })), files, origins },
  };
}

/** 从原生 AST 收集候选意图；匿名回调内的调用属于最内层已建模声明。 */
export function occurrences(ts, doc) {
  const { sf, declarations } = doc, result = [], strong = new Set();
  /** 选择最内层精确声明片段，忽略文件容器。 */
  function ownerAt(node) {
    const start = node.getStart(sf);
    return declarations.flatMap(d => d.parts.filter(p => p.ast.getStart(sf) <= start && start < p.ast.end).map(p => [p.ast.end - p.ast.getStart(sf), d])).sort((a, b) => a[0] - b[0])[0]?.[1];
  }
  /** 静态成员链保留查询名，工厂与计算接收者保持动态。 */
  function dynamic(node) { node = unwrap(ts, node); return !(ts.isIdentifier(node) || node.kind === ts.SyntaxKind.ThisKeyword || node.kind === ts.SyntaxKind.SuperKeyword || ts.isPropertyAccessExpression(node) && !dynamic(node.expression)); }
  /** 语义名由 AST 打印，保留动态字符串值中的空白。 */
  function add(kind, expression, at = expression) {
    expression = unwrap(ts, expression);
    const query = ts.isPropertyAccessExpression(expression) ? expression.name : ts.isElementAccessExpression(expression) ? expression.argumentExpression : expression;
    const owner = ownerAt(at);
    if (!owner || kind === 'type' && owner.node.category !== 'value') return;
    const name = ts.isIdentifier(query) || ts.isPrivateIdentifier(query) ? query.text : normalize(ts, sf, query);
    const occurrence = { kind, query, expression, owner, name, dynamic: ['extends', 'implements', 'type'].includes(kind) ? false : dynamic(expression) };
    result.push(occurrence); if (kind !== 'reference' && !occurrence.dynamic) strong.add(query.getStart(sf));
  }
  /** 排除写入位置、声明名、类型与模块别名，普通函数值读取仍需导航证明。 */
  function isRead(node) {
    let current = node;
    for (let p = node.parent; p; current = p, p = p.parent) {
      if (ts.isTypeNode(p) || ts.isImportDeclaration(p) || ts.isImportEqualsDeclaration(p) || ts.isExportDeclaration(p)) return false;
      if (ts.isBinaryExpression(p) && ts.isAssignmentOperator(p.operatorToken.kind)) {
        if (p.right === current) break;
        if (p.left === current) return false;
      }
      if ((ts.isPostfixUnaryExpression(p) || ts.isPrefixUnaryExpression(p)) && [ts.SyntaxKind.PlusPlusToken, ts.SyntaxKind.MinusMinusToken].includes(p.operator)) return false;
      if (ts.isStatement(p)) break;
    }
    const p = node.parent;
    if (p.name === node && !ts.isPropertyAccessExpression(p) && !ts.isShorthandPropertyAssignment(p)) return false;
    if (ts.isPropertyAccessExpression(p) && p.expression === node) return false;
    if (ts.isJsxAttribute(p) || ts.isJsxOpeningElement(p) || ts.isJsxClosingElement(p) || ts.isJsxSelfClosingElement(p)) return false;
    return true;
  }
  /** 类型使用只来自字段/属性的类型注解，初始化器断言和匿名参数不能借用外层字段。 */
  function typeUse(node) {
    for (let parent = node.parent; parent; parent = parent.parent) {
      const member = ts.isPropertyDeclaration(parent) || ts.isPropertySignature(parent) || ts.isIndexSignatureDeclaration(parent)
        || ts.isParameter(parent) && doc.byAst.get(parent)?.node.category === 'value';
      if (member && parent.type && parent.type.pos <= node.pos && node.end <= parent.type.end) return true;
    }
    return false;
  }
  /** 原生访问器自动穿过回调，强语义位置覆盖普通读取。 */
  function visit(node) {
    if (ts.isCallExpression(node) || ts.isTaggedTemplateExpression(node)) add('call', node.expression ?? node.tag, node);
    else if (ts.isNewExpression(node)) add('construct', node.expression, node);
    else if (ts.isExpressionWithTypeArguments(node) && ts.isHeritageClause(node.parent)) add(node.parent.token === ts.SyntaxKind.ExtendsKeyword ? 'extends' : 'implements', node.expression, node);
    else if (ts.isTypeReferenceNode(node) && typeUse(node)) add('type', ts.isQualifiedName(node.typeName) ? node.typeName.right : node.typeName, node);
    else if (ts.isJsxOpeningElement(node) || ts.isJsxSelfClosingElement(node)) { if (node.tagName.kind !== ts.SyntaxKind.ThisKeyword && (!ts.isIdentifier(node.tagName) || !/^[a-z]/.test(node.tagName.text) && !node.tagName.text.includes('-') && node.tagName.text !== 'this')) add('jsx', node.tagName, node); }
    else if ((ts.isIdentifier(node) || ts.isPropertyAccessExpression(node)) && isRead(node)) add('reference', node, node);
    ts.forEachChild(node, visit);
  }
  visit(sf);
  return [...new Map(result.filter(o => o.kind !== 'reference' || !strong.has(o.query.getStart(sf))).map(o => [`${o.owner.node.id}:${o.kind}:${o.query.getStart(sf)}:${o.query.end}`, o])).values()];
}
