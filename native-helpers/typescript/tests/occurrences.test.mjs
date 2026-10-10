import assert from 'node:assert/strict';
import path from 'node:path';
import { test } from 'node:test';
import { ts } from './support.mjs';
import { project } from '../projection.mjs';
import { occurrences } from '../analyze.mjs';

/** 原生 AST 候选测试保留旧 syntax 断言，端到端导航另由 graph_service 全集验证。 */
function parse(source) {
  const root = path.resolve('repo');
  const doc = project(ts, ts.createSourceFile(path.join(root, 'a.tsx'), source, ts.ScriptTarget.Latest, true), root);
  return { ...doc, occurrences: occurrences(ts, doc) };
}
/** 精确对象名，避免把文件容器当作语言模块。 */
function named(doc, name) { const d = doc.declarations.find(d => d.node.name === name); assert.ok(d, name); return d; }
/** 候选保留实际所有者与查询位置。 */
function has(doc, kind, name, owner) { return doc.occurrences.some(o => o.kind === kind && o.name === name && (!owner || o.owner.node.name === owner)); }

test('top_level_statements_and_initializers_share_scope_execution', () => {
  const doc = parse("'use strict'; function target() {} function outer() { if (enabled) target(); } const a = target(); const b = target(); target(); if (enabled) target(); [1].forEach(() => { target(); }); namespace N { const c = target(); target(); export function inner() { target(); } } class C { static { target(); } }");
  const regions = doc.declarations.filter(d => d.node.native_kind === 'top_level');
  assert.equal(regions.length, 2);
  for (const [parent, count] of [[null, 5], ['N', 2]]) {
    const region = regions.find(d => (d.parent?.node.name ?? null) === parent);
    assert.equal(doc.occurrences.filter(o => o.owner === region && o.kind === 'call' && o.name === 'target').length, count);
  }
  for (const owner of ['outer', 'inner']) assert.ok(has(doc, 'call', 'target', owner));
  assert.equal(doc.declarations.filter(d => d.node.native_kind === 'static_block').length, 1);
});
test('compound_callees_preserve_independent_function_value_references', () => {
  const doc = parse('function f() { (() => handler)(); (() => accept(callback))(); factory(argument)(); object[keyHandler](); direct(); }');
  for (const name of ['handler', 'callback', 'argument', 'keyHandler']) assert.ok(has(doc, 'reference', name, 'f'), name);
  assert.ok(doc.occurrences.filter(o => o.kind === 'call' && o.dynamic).length >= 4);
  assert.ok(!has(doc, 'reference', 'direct'));
  assert.ok(doc.occurrences.every(o => o.owner.node.name === 'f'));
});
test('tagged_templates_capture_calls_and_substitution_evidence', () => {
  const doc = parse('function f() { tag`hello ${format(valueHandler)}`; object.tag`member ${memberHandler}`; object[keyTag]`computed ${compute()}`; factory(factoryHandler)`dynamic ${dynamicHandler}`; (() => returnedTag)`iife ${substitutionHandler}`; }');
  for (const name of ['tag', 'format', 'compute', 'factory']) assert.ok(doc.occurrences.some(o => o.kind === 'call' && o.name === name && !o.dynamic), name);
  assert.equal(doc.occurrences.filter(o => o.kind === 'call' && o.name === 'tag').length, 2);
  assert.equal(doc.occurrences.filter(o => o.kind === 'call' && o.dynamic).length, 3);
  for (const name of ['valueHandler', 'memberHandler', 'keyTag', 'factoryHandler', 'dynamicHandler', 'returnedTag', 'substitutionHandler']) assert.ok(has(doc, 'reference', name, 'f'), name);
  assert.ok(doc.occurrences.every(o => o.owner.node.name === 'f'));
});
test('shorthand_private_computed_and_qualified_type_names_keep_evidence', () => {
  const doc = parse('function handler() {} function owner() { const obj = { handler }; } class C implements NS.Contract { #secret = 1; field: NS.Value; [makeKey()]() { return this.#secret; } } interface I extends NS.Base {}');
  assert.ok(has(doc, 'reference', 'handler', 'owner'));
  assert.equal(named(doc, '#secret').node.native_kind, 'field');
  assert.ok(!doc.declarations.some(d => d.parts.some(p => p.selection?.getText(doc.sf) === 'makeKey')));
  assert.ok(has(doc, 'call', 'makeKey'));
  for (const [kind, name] of [['extends', 'Base'], ['implements', 'Contract'], ['type', 'Value']]) assert.ok(has(doc, kind, name), name);
});
test('occurrence_selections_preserve_native_utf16_for_all_newlines', () => {
  const source = 'function 调用() {}\r\nfunction f() {\u2028  "😀中"; 调用();\u2029  调用();\r  调用();\r\n}';
  const doc = parse(source), calls = doc.occurrences.filter(o => o.kind === 'call');
  assert.equal(calls.length, 3);
  for (const [call, line, character] of [[calls[0], 2, 9], [calls[1], 3, 2], [calls[2], 4, 2]]) {
    assert.equal(call.query.getText(doc.sf), '调用');
    assert.deepEqual(doc.sf.getLineAndCharacterOfPosition(call.query.getStart(doc.sf)), { line, character });
    assert.equal(call.owner.node.name, 'f');
  }
});
test('assignment_targets_are_omitted_but_defaults_calls_and_reads_remain', () => {
  const doc = parse('function f(){ target = rhs; target += rhs; ++target; target--; ({x: target = fallback()} = obj); [target = defaultValue] = items; target = callback; call(target); }');
  assert.equal(doc.occurrences.filter(o => o.kind === 'reference' && o.name === 'target').length, 1);
  for (const name of ['rhs', 'obj', 'defaultValue', 'items', 'callback']) assert.ok(has(doc, 'reference', name), name);
  for (const name of ['fallback', 'call']) assert.ok(has(doc, 'call', name), name);
});
test('parameter_properties_preserve_source_expression_and_type_ownership', () => {
  const doc = parse("'use strict'; class C { constructor(/** 服务依赖 */ @inject() private readonly service: Model = make(), public optional?: Model, ordinary: Model = makeOrdinary(), readonly callback: () => Model = () => makeCallback(), ...rest: Model[]) {} }");
  assert.equal(named(doc, 'service').node.source, '/** 服务依赖 */ @inject() private readonly service: Model = make()');
  assert.match(named(doc, 'optional').node.signature, /\?/);
  for (const omitted of ['ordinary', 'rest']) assert.ok(!doc.declarations.some(d => d.node.name === omitted));
  for (const [owner, call] of [['service', 'make'], ['constructor', 'makeOrdinary'], ['callback', 'makeCallback']]) assert.ok(has(doc, 'call', call, owner));
  assert.equal(named(doc, 'callback').node.category, 'value');
  assert.equal(named(doc, 'callback').callable, null);
  assert.ok(has(doc, 'type', 'Model', 'service'));
  assert.match(named(doc, '顶层执行').node.code, /use strict/);
});
test('parameter_property_owners_survive_overloads_and_anonymous_classes', () => {
  const doc = parse('class Outer { constructor(value: number); constructor(public value: number) {} create() { class Inner { constructor(readonly nested: number) {} } return class { constructor(public hidden: number) {} }; } } class Container { constructor(readonly factory = class { constructor(public hidden: HiddenModel) {} }) {} }');
  assert.equal(named(doc, 'constructor').parts.length, 2);
  assert.equal(named(doc, 'value').parent.node.name, 'Outer');
  assert.equal(named(doc, 'nested').parent.node.name, 'Inner');
  assert.ok(!doc.declarations.some(d => d.node.name === 'hidden'));
  assert.ok(!has(doc, 'type', 'HiddenModel'));
  const cast = parse('class C { value: Declared = other as Asserted; }');
  assert.ok(has(cast, 'type', 'Declared', 'value'));
  assert.ok(!has(cast, 'type', 'Asserted'));
});
