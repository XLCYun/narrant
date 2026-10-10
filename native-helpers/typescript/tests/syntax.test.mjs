import assert from 'node:assert/strict';
import { test } from 'node:test';
import { ts } from './support.mjs';
import { hasSyntaxErrors } from '../syntax.mjs';

/** 原生内存 LS 只替代文件读取；语法树、binder、checker、导航和诊断缓存均为固定真实引擎。 */
function service(source, options = {}, filename = '/repo/main.ts') {
  const host = {
    getCompilationSettings: () => options, getScriptFileNames: () => [filename],
    getScriptVersion: () => '0', getProjectVersion: () => '0',
    getScriptSnapshot: name => name === filename ? ts.ScriptSnapshot.fromString(source) : undefined,
    getCurrentDirectory: () => '/repo', getDefaultLibFileName: () => '',
    fileExists: name => name === filename, readFile: name => name === filename ? source : undefined,
    readDirectory: () => [],
  };
  const ls = ts.createLanguageService(host), program = ls.getProgram(), sf = program.getSourceFile(filename);
  return { ls, program, sf, filename };
}

test('diagnostic transaction restores descriptors and native snapshots with cold and warm caches', () => {
  for (const warm of [false, true]) for (const [directive, settings, filename] of [
    ['', {}, '/repo/main.ts'], ['// @ts-nocheck\n', {}, '/repo/main.ts'],
    ['// @ts-ignore\n', {}, '/repo/main.ts'], ['// @ts-expect-error\n', {}, '/repo/main.ts'],
    ['', { noCheck: true }, '/repo/main.ts'], ['', { allowJs: true, checkJs: false }, '/repo/main.js'],
    ['', { noCheck: undefined, checkJs: undefined, skipLibCheck: undefined }, '/repo/main.ts'],
  ]) {
    const source = `${directive}return 1;\nfunction target(){} function caller(){target();missing()}`;
    const actual = service(source, { ...settings }, filename), baseline = service(source, { ...settings }, filename);
    try {
      const { ls, program, sf } = actual, options = program.getCompilerOptions();
      const beforeOptions = Object.getOwnPropertyDescriptors(options);
      const beforeDirective = Object.getOwnPropertyDescriptor(sf, 'checkJsDirective');
      const beforeRedirect = Object.getOwnPropertyDescriptor(program, 'isSourceOfProjectReferenceRedirect');
      const expected = baseline.ls.getSemanticDiagnostics(filename).map(d => d.code);
      if (warm) ls.getSemanticDiagnostics(filename);
      const position = source.indexOf('target();'), definitions = ls.getDefinitionAtPosition(filename, position);
      assert.equal(hasSyntaxErrors(ts, program, sf), true);
      assert.deepEqual(Object.getOwnPropertyDescriptors(options), beforeOptions);
      assert.deepEqual(Object.getOwnPropertyDescriptor(sf, 'checkJsDirective'), beforeDirective);
      assert.deepEqual(Object.getOwnPropertyDescriptor(program, 'isSourceOfProjectReferenceRedirect'), beforeRedirect);
      assert.equal(ls.getProgram(), program); assert.equal(program.getSourceFile(filename), sf); assert.equal(sf.text, source);
      assert.deepEqual(ls.getSemanticDiagnostics(filename).map(d => d.code), expected);
      assert.deepEqual(ls.getDefinitionAtPosition(filename, position), definitions);
    } finally { actual.ls.dispose(); baseline.ls.dispose(); }
  }
});

test('diagnostic transaction restores original absent and undefined properties even on native failure', () => {
  const { ls, program, sf } = service('// @ts-nocheck\nreturn 1;', { noCheck: undefined });
  const checker = program.getTypeChecker(), original = checker.getDiagnostics;
  const options = Object.getOwnPropertyDescriptors(program.getCompilerOptions()), directive = Object.getOwnPropertyDescriptor(sf, 'checkJsDirective');
  const redirect = Object.getOwnPropertyDescriptor(program, 'isSourceOfProjectReferenceRedirect');
  try {
    checker.getDiagnostics = () => { throw new Error('native diagnostic failure'); };
    assert.throws(() => hasSyntaxErrors(ts, program, sf), /native diagnostic failure/);
    assert.deepEqual(Object.getOwnPropertyDescriptors(program.getCompilerOptions()), options);
    assert.deepEqual(Object.getOwnPropertyDescriptor(sf, 'checkJsDirective'), directive);
    assert.deepEqual(Object.getOwnPropertyDescriptor(program, 'isSourceOfProjectReferenceRedirect'), redirect);
  } finally { checker.getDiagnostics = original; ls.dispose(); }
});

test('native early error families reject grammar while ordinary types targets and duplicate properties remain analyzable', () => {
  const rejected = [
    'return 1;', 'function f(){"use strict";with(x){}}', 'function f(a,a){"use strict";return a}',
    'let x;let x;', 'type T=number;type T=string;', 'function f(eval){"use strict"}',
    'function f(){continue}', 'outer: {function f(){break outer}}', 'function f(){await x}',
    'function* f(a=yield 1){}', 'const [x];', 'function f(...a,b){}',
    'const o={get x(a){return a}}', 'const o={set x(){}}', 'function f(){a?.x=1}',
    'class C{constructor(){}constructor(){}}', 'class C{static{return 1}}',
    'class C{#x=1;m(){delete this.#x}}',
  ];
  const allowed = [
    'const x:string=1; function f(){missing()}', 'function f(){return {x:1,x:2}}',
    'async function f(){await Promise.resolve(1)} class C{#x=1}', 'function f(){}function f(){}',
    'export default 1;export default 2;', 'export function f(){}export function f(){}',
    'const x=1;export {x};export {x};', 'export const x=1;const y=2;export {y as x};',
    'class C{#m(){}f(){this.#m=1}}', 'function f(){if({}==={})return 1}',
  ];
  for (const [expected, sources] of [[true, rejected], [false, allowed]]) for (const source of sources) {
    const { ls, program, sf } = service(source, { target: ts.ScriptTarget.ES5 });
    try { assert.equal(hasSyntaxErrors(ts, program, sf), expected, source); } finally { ls.dispose(); }
  }
});

test('private identifier diagnostic classification respects lexical instance static outer and shadowing declarations', () => {
  const cases = [
    ['class C{m(){return this.#x}}', true],
    ['class C{#x=1;m(x:{}){return x.#x}}', false],
    ['class C{#x=1}function f(x:C){return x.#x}', true],
    ['class C{#x=1;m(){return class D{m(x:{}){return x.#x}}}}', false],
    ['class C{m(){return class D{m(x:{}){return x.#x}}}}', true],
    ['class C{static #x=1;m(x:{}){return x.#x}}', false],
    ['class C{#x=1;m(){return class D{static #x=2;m(x:C){return x.#x}}}}', false],
    ['class C{#x=1;m(){return class D{m(x:{}){return x.#missing}}}}', true],
  ];
  for (const [source, expected] of cases) {
    const { ls, program, sf } = service(source);
    try { assert.equal(hasSyntaxErrors(ts, program, sf), expected, source); } finally { ls.dispose(); }
  }
});
