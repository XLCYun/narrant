import path from 'node:path';

/** 与 url::form_urlencoded::byte_serialize 一致，身份不能依赖安装目录或源码偏移。 */
export function component(value) {
  return [...Buffer.from(value)].map(byte => byte === 32 ? '+' : /[A-Za-z0-9*_.-]/.test(String.fromCharCode(byte)) ? String.fromCharCode(byte) : `%${byte.toString(16).toUpperCase().padStart(2, '0')}`).join('');
}
/** 测试判定只看仓库相对路径。 */
export function isTest(file) { return file.split('/').includes('__tests__') || /\.(test|spec)\.(?:ts|tsx|mts|cts|js|jsx|mjs|cjs)$/.test(file); }
/** 透明表达式只影响分类与导航，不从比较内容中删除。 */
export function unwrap(ts, node) {
  while (node && (ts.isParenthesizedExpression(node) || ts.isAsExpression(node) || ts.isTypeAssertionExpression(node) || ts.isNonNullExpression(node) || ts.isSatisfiesExpression(node))) node = node.expression;
  return node;
}
/** 原生 AST 投影为稳定声明；不建立第二套语法模型。 */
export function project(ts, sf, root, runtime = false) {
  const relative = path.relative(root, sf.fileName);
  const language = /\.[cm]?jsx?$/i.test(sf.fileName) ? 'javascript' : 'typescript';
  const scope = `${language}:${runtime ? 'runtime' : 'repo'}:${component(relative)}`;
  const declarations = [], byAst = new Map(), counts = new Map(), executions = new Map();
  const modifier = (node, kind) => node.modifiers?.some(m => m.kind === kind);
  const fn = node => node && (ts.isFunctionExpression(node) || ts.isArrowFunction(node));
  /** 计算名字仅在静态字面量时有稳定名字，其余使用 computed。 */
  function nameOf(node) {
    if (!node) return null;
    if (ts.isComputedPropertyName(node)) return ts.isStringLiteralLike(node.expression) || ts.isNumericLiteral(node.expression) ? node.expression.text : 'computed';
    return node.text ?? node.getText(sf);
  }
  /** 保留前导注释原文；比较内容始终来自独立的原生打印过程。 */
  function excerpt(node) { return sf.text.slice(node.getFullStart(), node.end).trimStart(); }
  /** 具名类型主体字段可建模，嵌套泛型参数内的结构字段仍属于完整类型表达式。 */
  function namedType(node) {
    let parent = node.parent;
    if (ts.isInterfaceDeclaration(parent)) return true;
    if (!ts.isTypeLiteralNode(parent)) return false;
    parent = parent.parent;
    while (parent && (ts.isParenthesizedTypeNode(parent) || ts.isUnionTypeNode(parent) || ts.isIntersectionTypeNode(parent))) parent = parent.parent;
    return parent && ts.isTypeAliasDeclaration(parent);
  }
  /** 只有相同词法父级和种类的签名/实现组成重载，不合并 namespace/interface 片段。 */
  function add(node, owner, category, kind, name, body, bindings, ambient, callable) {
    let existing = ['function', 'method', 'static_method', 'constructor'].includes(kind)
      ? declarations.findLast(d => d.parent === owner && d.node.native_kind === kind && d.node.name === name && (!body || d.parts.every(p => !p.body))) : null;
    const part = { ast: node, body, selection: node.name, source: ts.isVariableDeclaration(node) ? node.parent.parent : node };
    if (existing) { existing.parts.push(part); byAst.set(node, existing); return existing; }
    const base = `${owner?.node.id ?? scope}${bindings.map(b => `::binding:${component(b)}`).join('')}::${kind}:${component(name ?? 'anonymous')}`;
    const count = (counts.get(base) ?? 0) + 1; counts.set(base, count);
    const item = { parent: owner, parts: [part], ambient, callable, node: { category, id: count === 1 ? base : `${base}#${count}`, language, native_kind: kind, name, signature: null, code: null, source: null, contains: [], references: [], extensions: {} } };
    declarations.push(item); byAst.set(node, item); if (owner) owner.node.contains.push(item.node.id);
    return item;
  }
  /** 顶层执行只合并文件或 namespace 的语句，函数体中的回调保留外层所有权。 */
  function execution(owner, statement) {
    if (!executions.has(owner)) executions.set(owner, new Set());
    executions.get(owner).add(statement);
  }
  /** 一次递归同时确定词法归属、函数边界、透明包装与参数属性。 */
  function visit(node, owner = null, inFunction = false, bindings = [], ambient = sf.isDeclarationFile, top = null) {
    const inheritedOwner = owner;
    const staticMember = modifier(node, ts.SyntaxKind.StaticKeyword);
    const initializer = unwrap(ts, node.initializer);
    const callable = fn(initializer) ? initializer : null;
    const key = nameOf(node.name);
    ambient ||= modifier(node, ts.SyntaxKind.DeclareKeyword);
    let shape;
    if (ts.isFunctionDeclaration(node) && node.name && !inFunction) shape = ['callable', 'function', key, node.body];
    else if (ts.isFunctionExpression(node) && node.name && !inFunction && owner?.callable !== node) shape = ['callable', 'function', key, node.body];
    else if (ts.isClassDeclaration(node) || ts.isClassExpression(node) && node.name) shape = ['type', 'class', key, null];
    else if (ts.isInterfaceDeclaration(node)) shape = ['type', 'interface', key, null];
    else if (ts.isTypeAliasDeclaration(node)) shape = ['type', 'type_alias', key, null];
    else if (ts.isEnumDeclaration(node)) shape = ['type', 'enum', key, null];
    else if (ts.isModuleDeclaration(node)) shape = ['container', node.flags & ts.NodeFlags.GlobalAugmentation ? 'global' : ts.isStringLiteral(node.name) ? 'module' : 'namespace', key, null];
    else if (ts.isVariableDeclaration(node) && ts.isIdentifier(node.name) && callable && !inFunction) shape = ['callable', ts.isArrowFunction(callable) ? 'arrow' : 'function', key, callable.body];
    else if (ts.isConstructorDeclaration(node) && !inFunction) shape = ['callable', 'constructor', 'constructor', node.body];
    else if ((ts.isMethodDeclaration(node) || ts.isMethodSignature(node) || ts.isGetAccessor(node) || ts.isSetAccessor(node)) && !inFunction && key !== 'computed') shape = ['callable', ts.isGetAccessor(node) ? 'getter' : ts.isSetAccessor(node) ? 'setter' : ts.isObjectLiteralExpression(node.parent) ? 'object_method' : staticMember ? 'static_method' : 'method', key, node.body];
    else if (ts.isPropertyAssignment(node) && callable && !inFunction && key !== 'computed') shape = ['callable', 'object_method', key, callable.body];
    else if (ts.isPropertyDeclaration(node) && !modifier(node, ts.SyntaxKind.AccessorKeyword)) shape = callable ? (!inFunction && key !== 'computed' ? ['callable', staticMember ? 'static_method' : 'method', key, callable.body] : null) : ['value', staticMember ? 'static_field' : 'field', key, node.initializer];
    else if (ts.isParameter(node) && node.modifiers?.some(m => [ts.SyntaxKind.PublicKeyword, ts.SyntaxKind.PrivateKeyword, ts.SyntaxKind.ProtectedKeyword, ts.SyntaxKind.ReadonlyKeyword, ts.SyntaxKind.OverrideKeyword].includes(m.kind)) && ts.isIdentifier(node.name)) {
      const parentClass = byAst.get(node.parent.parent);
      if (parentClass) { owner = parentClass; shape = ['value', 'field', key, node.initializer]; }
    }
    else if (ts.isEnumMember(node)) shape = ['value', 'enum_member', key, node.initializer];
    else if (ts.isPropertySignature(node) && !inFunction && namedType(node)) shape = ['value', 'property', key, null];
    else if (ts.isIndexSignatureDeclaration(node) && !inFunction && namedType(node)) shape = ['value', 'index_signature', null, null];
    else if (ts.isClassStaticBlockDeclaration(node)) shape = ['execution_region', 'static_block', null, node.body];
    if (shape) { owner = add(node, owner, ...shape, bindings, ambient, ts.isParameter(node) ? null : callable); bindings = []; }
    if (top && owner === inheritedOwner && (ts.isCallExpression(node) || ts.isNewExpression(node) || ts.isTaggedTemplateExpression(node) || ts.isJsxOpeningElement(node) || ts.isJsxSelfClosingElement(node) || ts.isBinaryExpression(node) && ts.isAssignmentOperator(node.operatorToken.kind) || ts.isPostfixUnaryExpression(node) || ts.isPrefixUnaryExpression(node) && [ts.SyntaxKind.PlusPlusToken, ts.SyntaxKind.MinusMinusToken].includes(node.operator))) execution(top.owner, top.node);
    if (ts.isSourceFile(node) || ts.isModuleBlock(node)) {
      for (const statement of node.statements) {
        if (!(ts.isDeclarationStatement(statement) || ts.isVariableStatement(statement) || ts.isImportDeclaration(statement) || ts.isExportDeclaration(statement) || ts.isEmptyStatement(statement)) && !(ts.isExportAssignment(statement) && ts.isIdentifier(statement.expression))) execution(owner, statement);
        visit(statement, owner, inFunction, [], ambient, { owner, node: statement });
      }
      return;
    }
    if (!shape && ts.isVariableDeclaration(node) && ts.isIdentifier(node.name)) bindings = [...bindings, key];
    if (!shape && ts.isPropertyAssignment(node)) bindings = [...bindings, key];
    const entersFunction = ts.isFunctionLike(node) && !ts.isMethodSignature(node) && !ts.isCallSignatureDeclaration(node) && !ts.isConstructSignatureDeclaration(node);
    ts.forEachChild(node, child => visit(child, owner, inFunction || entersFunction, bindings, ambient, shape || entersFunction ? null : top));
  }
  visit(sf);
  for (const [owner, statements] of executions) {
    const first = [...statements][0];
    const item = add(first, owner, 'execution_region', 'top_level', '顶层执行', first, [], false, null);
    item.parts = [...statements].map(ast => ({ ast, body: ast, source: ast }));
  }
  const memberSet = new Set(declarations.flatMap(d => d.parts.map(p => p.ast)));
  /** 只裁剪声明成员列表；初始化器、参数、函数体中的表达式必须完整比较。 */
  function signatureNode(node) {
    const f = ts.factory;
    const keep = list => list.filter(n => !memberSet.has(n));
    if (ts.isClassDeclaration(node) || ts.isClassExpression(node)) return f[ts.isClassDeclaration(node) ? 'updateClassDeclaration' : 'updateClassExpression'](node, node.modifiers, node.name, node.typeParameters, node.heritageClauses, keep(node.members));
    if (ts.isInterfaceDeclaration(node)) return f.updateInterfaceDeclaration(node, node.modifiers, node.name, node.typeParameters, node.heritageClauses, keep(node.members));
    if (ts.isEnumDeclaration(node)) return f.updateEnumDeclaration(node, node.modifiers, node.name, keep(node.members));
    if (ts.isTypeAliasDeclaration(node)) {
      /** 只沿别名自身的联合/交叉/括号裁剪。 */
      const prune = type => ts.isTypeLiteralNode(type) ? f.updateTypeLiteralNode(type, keep(type.members)) : ts.isUnionTypeNode(type) ? f.updateUnionTypeNode(type, type.types.map(prune)) : ts.isIntersectionTypeNode(type) ? f.updateIntersectionTypeNode(type, type.types.map(prune)) : ts.isParenthesizedTypeNode(type) ? f.updateParenthesizedType(type, prune(type.type)) : type;
      return f.updateTypeAliasDeclaration(node, node.modifiers, node.name, node.typeParameters, prune(node.type));
    }
    if (ts.isModuleDeclaration(node) && node.body) {
      const statements = ts.isModuleBlock(node.body) ? node.body.statements.flatMap(statement => {
        if (memberSet.has(statement)) return [];
        if (!ts.isVariableStatement(statement)) return [statement];
        const declarations = keep(statement.declarationList.declarations);
        return declarations.length ? [f.updateVariableStatement(statement, statement.modifiers, f.updateVariableDeclarationList(statement.declarationList, declarations))] : [];
      }) : [];
      return f.updateModuleDeclaration(node, node.modifiers, node.name, f.createModuleBlock(statements));
    }
    return node;
  }
  for (const item of declarations) {
    item.node.source = item.parts.map(p => excerpt(p.source)).join('\n');
    item.node.signature = item.parts.map(p => {
      const projected = signatureNode(p.ast);
      let printed = normalize(ts, sf, projected, projected === p.ast ? p.body : null);
      if (ts.isVariableDeclaration(p.ast)) {
        const list = p.ast.parent;
        printed = `${modifier(list.parent, ts.SyntaxKind.ExportKeyword) ? 'export ' : ''}${list.flags & ts.NodeFlags.Const ? 'const' : list.flags & ts.NodeFlags.Let ? 'let' : 'var'} ${printed}`;
      }
      return printed;
    }).join('\n');
    item.node.code = item.parts.filter(p => p.body).map(p => normalize(ts, sf, p.body)).join('\n') || null;
  }
  if (declarations.length) declarations.push({ parts: [], parent: null, node: { category: 'container', id: scope, language, native_kind: 'file', name: relative.split(path.sep).join('/'), signature: null, code: null, source: sf.text, contains: declarations.filter(d => !d.parent).map(d => d.node.id), references: [], extensions: {} } });
  return { sf, declarations, byAst, runtime };
}

/** 原生打印统一普通格式与字符串引号，AST token 保留模板、正则、JSX 文本和数组空位。 */
export function normalize(ts, sf, node, omit = null) {
  const result = ts.transform(node, [context => {
    /** 只移除本对象实现以及无语义的可选尾逗号，不改写表达式。 */
    function visitor(current) {
      if (current === omit) return (ts.isArrowFunction(current.parent) || ts.isClassStaticBlockDeclaration(current.parent)) ? ts.factory.createBlock([]) : undefined;
      if (ts.isStringLiteral(current)) return ts.factory.createStringLiteral(current.text);
      return ts.visitEachChild(current, visitor, context, (list, visit, test, start, count) => {
        const updated = ts.visitNodes(list, visit, test, start, count);
        if (!updated) return updated;
        return ts.factory.createNodeArray(updated, updated.length > 0 && ts.isOmittedExpression(updated.at(-1)));
      });
    }
    return root => visitor(root);
  }]);
  let printed;
  try { printed = result.transformed[0] ? ts.createPrinter({ removeComments: true, newLine: ts.NewLineKind.LineFeed }).printNode(ts.EmitHint.Unspecified, result.transformed[0], sf) : ''; }
  finally { result.dispose(); }
  // 片段在所属语法位置重新解析，避免对象/方法片段被当成错误的顶层语句丢失 token。
  let prefix = '', suffix = '';
  if (ts.isExpression(node)) { prefix = 'const __narrant = ('; suffix = ');'; }
  else if (ts.isVariableDeclaration(node)) { prefix = 'const '; suffix = ';'; }
  else if (ts.isParameter(node)) { prefix = 'function __narrant('; suffix = '){}'; }
  else if (ts.isEnumMember(node)) { prefix = 'enum __narrant {'; suffix = '}'; }
  else if (ts.isClassElement(node)) { prefix = 'class __narrant {'; suffix = '}'; }
  else if (ts.isTypeElement(node)) { prefix = 'interface __narrant {'; suffix = '}'; }
  else if (ts.isTypeNode(node)) { prefix = 'type __narrant = '; suffix = ';'; }
  const text = prefix + printed + suffix;
  const parsed = ts.createSourceFile(sf.fileName, text, ts.ScriptTarget.Latest, true, sf.scriptKind);
  const tokens = [];
  /** 叶子文本保持语义空白；其他空白只是 token 分隔符。 */
  function collect(current) {
    const children = current.getChildren(parsed);
    if (children.length) children.forEach(collect);
    else {
      // JSX 文本的前导/纯空白属于渲染内容；getStart 会把它当 trivia 跳过，必须保留完整原生 span。
      const start = ts.isJsxText(current) ? current.pos : current.getStart(parsed);
      if (current.kind !== ts.SyntaxKind.EndOfFileToken && current.end > start && start >= prefix.length && current.end <= prefix.length + printed.length) tokens.push(text.slice(start, current.end));
    }
  }
  collect(parsed);
  return tokens.join(' ');
}
