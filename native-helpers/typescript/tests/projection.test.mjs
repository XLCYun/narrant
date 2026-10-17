import assert from 'node:assert/strict';
import path from 'node:path';
import { test } from 'node:test';
import { ts } from './support.mjs';
import { project, component, isTest } from '../projection.mjs';

/** 内存原生 AST 测试比较语义内容，原文另行逐字检查。 */
function parse(text, file = 'a.tsx', root = path.resolve('repo')) { return project(ts, ts.createSourceFile(path.join(root, file), text, ts.ScriptTarget.Latest, true), root).declarations; }
/** 从声明中取出公共节点。 */
function named(ds, name) { const d = ds.find(d => d.node.name === name); assert.ok(d, `missing ${name}`); return d.node; }
/** 比较自身内容，忽略展示原文与无序包含集合。 */
function contents(ds) { return Object.fromEntries(ds.map(d => [d.node.id, [d.node.signature, d.node.code]])); }
/** 对相同身份检查发生内容变化的对象名。 */
function changes(before, after) { const a = contents(before), b = contents(after); return before.filter(d => JSON.stringify(a[d.node.id]) !== JSON.stringify(b[d.node.id])).map(d => d.node.name); }

test('own_content_excludes_children_but_keeps_callbacks', () => {
  const source = 'class C { p: T = make(); constructor() {} get value() { return 1; } set value(v: number) {} m() { function nested() { return 1; } run(() => 1); } static { run(); } }';
  const before = parse(source);
  assert.deepEqual(changes(before, parse(source.replaceAll('return 1', 'return 2'))), ['value', 'm']);
  assert.deepEqual(changes(before, parse(source.replace('run(() => 1)', 'run(() => 2)'))), ['m']);
});
test('formatting_and_comments_do_not_change_diff', () => {
  const a = parse('/** original docs */\nexport const f = (x: number) => { // ordinary\n return /a b/g.test(`hello ${x}`) ? <Widget text="x"/> : null; }; ');
  const b = parse('/** changed docs */\nexport const f=(x:number)=>{return /a b/g.test(`hello ${x}`)?<Widget text="x" />:null;};');
  assert.deepEqual(contents(a), contents(b)); assert.match(named(a, 'f').source, /original docs/);
});
test('meaningful_javascript_text_survives_normalization', () => {
  for (const [a, b] of [
    ['function f() { return\n{ x: 1 }; }', 'function f() { return { x: 1 }; }'],
    ['class C { x = `a b`; }', 'class C { x = `a  b`; }'],
    ['class C { x = /a b/; }', 'class C { x = /a  b/; }'],
    ['const x = <div>a b</div>;', 'const x = <div>a  b</div>;'],
    ['function f(){return <div>a</div>}', 'function f(){return <div> a</div>}'],
    ['function f(){return <div></div>}', 'function f(){return <div> </div>}'],
    ['const x = tag`a\\nb`;', 'const x = tag`a\nb`;'],
  ]) assert.notDeepEqual(contents(parse(a)), contents(parse(b)), a);
});
test('sparse_array_hole_positions_and_counts_remain_distinct', () => {
  const arrays = ['[keep]', '[, keep]', '[keep, ,]', '[,,]', '[]'];
  assert.equal(new Set(arrays.map(a => named(parse(`class C { value = ${a}; }`), 'value').code)).size, arrays.length);
});
test('embedded_declaration_edits_remain_in_complete_expressions', () => {
  for (const [owner, source] of [
    ['value', 'class C { value = [class Inner { field = 1; }, fallback]; }'],
    ['C', 'class C extends factory(class Inner { field = 1; }) {}'],
    ['f', 'function f(value = class Inner { field = 1; }) {}'],
    ['f', 'function f() { class Inner { field = 1; } return Inner; }'],
    ['Item', 'enum E { Item = factory(class Inner { field = 1; }) }'],
  ]) {
    const a = parse(source), b = parse(source.replace('field = 1', 'field = 2')), changed = changes(a, b);
    assert.ok(changed.includes(owner), source); assert.ok(changed.includes('field'), source); assert.ok(!changed.includes('Inner'), source);
  }
});
test('full_expressions_ignore_formatting_and_optional_trailing_commas', () => {
  for (const [a, b] of [['[keep]', '[keep,]'], ['{ key: 1 }', '{ key: 1, }'], ['factory(a)', 'factory(a,)'], ['new Model(a)', 'new Model(a,)'], ['{ method(a) {} }', '{ method(a,) {} }']]) {
    assert.deepEqual(contents(parse(`class C { value = ${a}; }`)), contents(parse(`class C{/* comment */ value=${b};}`)), a);
  }
});
test('declaration_lists_preserve_unmodeled_members', () => {
  for (const [owner, a, b] of [
    ['C', 'class C { accessor kept = 1; }', 'class C { first() {} accessor kept = 1; last = 2; }'],
    ['I', 'interface I { (): string; new(): I; }', 'interface I { a: string; (): string; method(): void; new(): I; [name: string]: unknown; }'],
    ['T', 'type T = ({ (): string } | { new(): T }) & Base', 'type T = ({ field: string; (): string } | { new(): T; method(): void }) & Base'],
    ['N', 'namespace N { const kept = 1; }', 'namespace N { export const first = () => 1; const kept = 1; export function last() {} }'],
    ['external', "declare module 'external' {}", "declare module 'external' { export interface I { a: number } export function f(): void; }"],
    ['global', 'export {}; declare global {}', 'export {}; declare global { interface I { a: number } }'],
    ['N', 'namespace N {}', 'namespace N.Inner { export function f() {} }'],
  ]) { const before = named(parse(a), owner), after = named(parse(b), owner); assert.equal(before.signature, after.signature, owner); assert.notDeepEqual(before.contains, after.contains); }
});
test('mixed_variable_statements_keep_noncallable_bindings', () => {
  for (const prefix of ['', 'export ']) {
    const a = named(parse(`namespace N { ${prefix}const kept = 1; }`), 'N');
    const source = `namespace N { ${prefix}const first = () => 1, kept = 1, last = function() {}; }`;
    assert.equal(a.signature, named(parse(source), 'N').signature);
    assert.notEqual(a.signature, named(parse(source.replace('kept = 1', 'kept = 2')), 'N').signature);
  }
});
test('retained_declaration_content_still_changes', () => {
  for (const [owner, source, a, b] of [
    ['C', 'class C extends Before { field = 1; }', 'Before', 'After'],
    ['C', 'class C { accessor kept = 1; }', '= 1', '= 2'],
    ['I', 'interface I { (): string; field: number }', 'string', 'number'],
    ['T', 'type T = { field: number } & Before', 'Before', 'After'],
    ['T', 'type T = Array<{ field: string }>', 'string', 'number'],
    ['N', 'namespace N { const object = { run() { return 1; } }; }', 'return 1', 'return 2'],
  ]) assert.notEqual(named(parse(source), owner).signature, named(parse(source.replace(a, b)), owner).signature);
});
test('projected_containers_keep_original_source_and_positions', () => {
  const source = '/** docs */\nexport class C {\n  value = [class Inner { field = 1; }, fallback];\n  method() { return this.value; }\n}';
  const ds = parse(source), c = named(ds, 'C');
  assert.equal(c.source, source); assert.equal(c.contains.length, 2); assert.doesNotMatch(c.signature, /value/);
  assert.match(named(ds, 'value').code, /field = 1/);
  for (const name of ['value', 'Inner', 'field', 'method']) assert.equal(ds.find(d => d.node.name === name).parts[0].selection.getStart(), source.indexOf(name));
});
test('identity escaping and omitted wrappers remain stable', () => {
  assert.equal(component("a b/中文!~'()"), 'a+b%2F%E4%B8%AD%E6%96%87%21%7E%27%28%29');
  const source = 'const handlers = { nested: { save() {} } }; namespace N { export class C { method() {} } }';
  const a = parse(source, '目录/a.ts', path.resolve('repo')), b = parse(source, '目录/a.ts', path.resolve('other'));
  assert.deepEqual(contents(a), contents(b)); assert.match(named(a, 'save').id, /::binding:handlers::binding:nested::object_method:save$/);
});
test('file family, ambient declarations, overloads and direct callable values', () => {
  for (const ext of ['ts','tsx','js','jsx','mts','cts','mjs','cjs']) assert.equal(parse('function f() { return 1; }', `a.${ext}`).length, 2);
  for (const ext of ['d.ts','d.mts','d.cts']) assert.equal(parse('declare namespace N { interface I { p: string; } function f(): void; }', `a.${ext}`).length, 5);
  const ds = parse('export function f(x: string): string; export function f(x: number): number; export function f(x: any) { return x; } interface I {} interface I {} const a = () => {}; const b = function internal() {}; function outer() { function nested() {} const local = () => 1; }');
  assert.equal(ds.find(d => d.node.name === 'f').parts.length, 3);
  assert.equal(ds.filter(d => d.node.name === 'I').length, 2);
  assert.ok(ds.every(d => !['internal', 'nested', 'local'].includes(d.node.name)));
});
test('data members exclude ordinary values, callbacks and nested anonymous types', () => {
  const ds = parse('const plain = 1; const object = { leaf: 1 }; class C { field: string; static total = 1; constructor(public parameter: string) {} accessor automatic = ""; } interface I { property: C; [key: string]: C; child: { nested: string }; } type T = C & ({ active: boolean }); function f(p: { input: string }) { interface Local { local: string } }');
  for (const name of ['plain','object','leaf','automatic','nested','input','local']) assert.ok(!ds.some(d => d.node.name === name), name);
  for (const name of ['field','total','parameter','property','child','active']) assert.equal(named(ds,name).category,'value');
  assert.equal(named(ds, 'Local').category, 'type'); assert.match(named(ds,'child').signature,/nested/);
});
test('pure routes produce no objects, exports and complete expressions change content', () => {
  assert.equal(parse("import {f} from './x'; export {f}; export default f;").length, 0);
  for (const [a,b,name] of [['function f() {}','export function f() {}','f'],['class C {}','export default class C {}','C'],['const f = () => 1;','export const f = () => 1;','f']]) assert.notEqual(named(parse(a),name).signature,named(parse(b),name).signature);
  for (const [a,b] of [['[]','[function Child() {}]'],['[keep, ,]','[keep, class Child {}, ,]'],['{}','{method(){}}'],['factory(before, after)','factory(function Child(){}, before, after)'],['flag ? null : other','flag ? class Child {} : other']]) {
    const before = parse(`class C { value = ${a}; }`), after = parse(`class C { value = ${b}; }`);
    assert.notEqual(named(before,'value').code,named(after,'value').code);
    assert.notDeepEqual(named(before,'value').contains,named(after,'value').contains);
    assert.equal(named(before,'C').signature,named(after,'C').signature);
  }
});
test('array and object reordering changes only initializer content', () => {
  for (const [a,b] of [['[function First() {}, , keep, class Last {}, ...rest, ,]','[, class Last {}, keep, ...rest, , function First() {}]'], ['{ first() {}, ...left, middle() {}, ...right, last() {} }','{ last() {}, first() {}, ...left, ...right, middle() {} }']]) assert.deepEqual(changes(parse(`class C { value = ${a}; }`),parse(`class C { value = ${b}; }`)),['value']);
});


test('test convention stays case sensitive and repository relative', () => {
  for (const file of ['a.test.ts', 'a.spec.mjs', '__tests__/a.tsx']) assert.equal(isTest(file), true, file);
  for (const file of ['a.TEST.ts', 'a.Spec.js', '__TESTS__/a.ts', 'test/a.ts', 'a.test.txt']) assert.equal(isTest(file), false, file);
});

test('parameter_properties_are_direct_class_fields_including_override', () => {
  for (const modifier of ['public', 'private', 'protected', 'readonly', 'public readonly', 'private readonly', 'protected readonly', 'override', 'public override', 'override readonly', 'protected override readonly']) {
    const ds = parse(`class Base { protected value = 0; } class Derived extends Base { constructor(${modifier} value: number) { super(); } }`);
    const fields = ds.filter(d => d.node.name === 'value'), derived = named(ds, 'Derived'), constructor = named(ds, 'constructor');
    assert.equal(fields.length, 2, modifier); assert.notEqual(fields[0].node.id, fields[1].node.id);
    const field = fields[1];
    assert.equal(field.node.category, 'value'); assert.equal(field.node.native_kind, 'field');
    assert.equal(field.parent.node.id, derived.id); assert.ok(derived.contains.includes(field.node.id)); assert.ok(!constructor.contains.includes(field.node.id));
    assert.equal(field.node.source, `${modifier} value: number`);
  }
});
test('parameter_property_identity_and_content_match_explicit_fields', () => {
  const before = parse('class C { constructor(public first: number, private second: string) {} }');
  const reordered = parse('class C { constructor(private second: string, /* moved */ public first: number) {} }');
  const explicit = parse('class C { public first: number; private second: string; constructor(first: number, second: string) { this.first = first; this.second = second; } }');
  for (const name of ['first', 'second']) {
    assert.equal(named(before, name).id, named(reordered, name).id);
    assert.equal(named(before, name).id, named(explicit, name).id);
    assert.equal(named(before, name).signature, named(reordered, name).signature);
  }
  const source = 'class C { constructor(readonly value: number = 1) { run(); } }', a = parse(source);
  for (const changed of [source.replace('= 1', '= 2'), source.replace('number', 'number | string')]) assert.deepEqual(changes(a, parse(changed)).sort(), ['constructor', 'value']);
  assert.deepEqual(changes(a, parse(source.replace('run()', 'other()'))), ['constructor']);
});
test('comparison_view_state_changes_do_not_add_data_members', () => {
  const source = 'function ComparisonView({ comparison }: { comparison: StructureComparison }) { return comparison; }';
  const before = parse(source);
  for (const after of [source.replace('return comparison;', 'const [layoutResult, setLayoutResult] = useState<{ comparison: StructureComparison; layout: StructureLayout }>(); return comparison;'), source.replace('StructureComparison', 'OtherComparison')]) {
    const ds = parse(after); assert.ok(ds.every(d => d.node.category !== 'value')); assert.deepEqual(named(ds, 'ComparisonView').contains, []);
    assert.deepEqual(changes(before, ds), ['ComparisonView']);
  }
});
test('named_function_values_exclude_function_interiors', () => {
  const ds = parse('const a = () => {}; const b = function internalName() {}; let mutable = function() {}; var legacy = () => {}; namespace N { export const member = () => {}; } class C { handler = () => {}; static staticHandler = function() {}; #privateHandler = () => {}; method() {} } const object = { arrow: () => {}, expression: function internalProperty() {}, method() {}, get getter() { return 1; } }; function outer() { function nested() { target(); } const local = () => target(); const localObject = { hidden() { target(); }, field: function namedLocal() { target(); } }; run(() => { function callbackInner() { target(); } const callbackValue = () => target(); }); }');
  for (const name of ['a', 'b', 'mutable', 'legacy', 'member', 'handler', 'staticHandler', '#privateHandler', 'arrow', 'expression', 'getter', 'outer']) assert.equal(named(ds, name).category, 'callable');
  for (const name of ['internalName', 'internalProperty', 'nested', 'local', 'hidden', 'namedLocal', 'callbackInner', 'callbackValue']) assert.ok(!ds.some(d => d.node.name === name), name);
  assert.deepEqual(named(ds, 'outer').contains, []);
});
