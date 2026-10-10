"""对真实 LibTooling 二进制验证图、配置并集、来源边界和单次进程退出。"""
import hashlib
import json
import os
from pathlib import Path
import subprocess
import tempfile
import unittest

HERE = Path(__file__).resolve().parent.parent
HELPER = Path(os.environ.get('NARRANT_CLANG_HELPER', HERE / 'build/narrant-clang'))
LLVM = Path(os.environ.get('NARRANT_LLVM_ROOT', '/tmp/narrant-clang-native/LLVM-21.1.8-macOS-ARM64'))
PROTOCOL = 'narrant.native-analyzer/1'
CALLS = '''#if FAST
#define PICK fast
#define RESULT int
int mode(void) { return 1; }
#else
#define PICK safe
#define RESULT double
int mode(void) { return 2; }
#endif
int fast(void) { return 1; }
int safe(void) { return 2; }
int run(void) { return PICK(); }
RESULT value(void) { return 1; }
int branch(void) { return mode(); }
'''
CATEGORY = '''#if FAST
#define DECL typedef int
#else
#define DECL int
#endif
DECL item;
'''
PARENT = '''#if FAST
struct Left {
#else
struct Right {
#endif
int slot;
};
'''


def snapshot(root):
    """记录全部仓内文件和目录，验证分析没有写后回滚或新增产物。"""
    return {str(p.relative_to(root)): hashlib.sha256(p.read_bytes()).hexdigest() if p.is_file() else 'directory'
            for p in root.rglob('*')}


class NativeHelperTests(unittest.TestCase):
    """每个案例使用独立临时仓库与保持打开的 stdin 验证实际 helper。"""

    def setUp(self):
        """准备独立的测试源码与 scratch，绝不修改用户业务源码。"""
        self.temp = tempfile.TemporaryDirectory(prefix='narrant-clang-test-')
        self.base = Path(self.temp.name)
        self.root = self.base / 'repo'
        self.root.mkdir()
        self.scratch = self.base / 'scratch'
        self.scratch.mkdir()

    def tearDown(self):
        """清理本测试创建的临时材料。"""
        self.temp.cleanup()

    def prepare(self, sources, mode='cpp', order=(0, 1), selected=None, extra=(), standard=None):
        """为每个 TU 生成显式标准和宏配置的 CDB 数据。"""
        commands = []
        for name, source in sources.items():
            path = self.root / name
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text(source)
            if path.suffix in ('.c', '.cpp'):
                for fast in order:
                    commands.append({'directory': str(self.root), 'file': name,
                                     'arguments': ['clang', '-x', 'c' if mode == 'c' else 'c++',
                                                   '-std=' + (standard or ('c17' if mode == 'c' else 'c++20')),
                                                   '-DFAST=' + str(fast), *extra, '-c', name]})
        (self.root / 'compile_commands.json').write_text(json.dumps(commands))
        return {'protocol': PROTOCOL, 'type': 'analyze', 'id': 'test',
                'input': {'repository_root': str(self.root), 'files': selected or list(sources),
                          'include_tests': True, 'profile': 'clang-static-v1'},
                'environment': {'scratch_root': str(self.scratch), 'platforms': [
                    {'id': 'freestanding', 'kind': 'clang-target', 'version': '1',
                     'root': str(self.scratch),
                     'metadata': {'target_triple': 'arm64-apple-macosx14.0', 'system_include_directories': [],
                                  'resource_platform_id': 'resource'}},
                    {'id': 'resource', 'kind': 'clang-resource', 'version': '21.1.8',
                     'root': str(LLVM / 'lib/clang/21'), 'metadata': {}}]},
                'limits': {'analysis_timeout_ms': 15000, 'max_result_bytes': 8 * 1024 * 1024,
                           'max_diagnostic_bytes': 256 * 1024}}

    def launch(self):
        """按 NDJSON 的 UTF-8 契约打开三条管道，并校验真实二进制的唯一 ready。"""
        process = subprocess.Popen([str(HELPER)], stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                                   stderr=subprocess.PIPE, text=True, encoding='utf-8', errors='strict')
        ready = json.loads(process.stdout.readline())
        self.assertEqual(ready['helper_id'], 'narrant-clang')
        self.assertEqual(ready['projection_revision'], 'clang-source-v1')
        return process

    def run_request(self, request):
        """等待 completed 与退出，stdin 在退出前保持开启以捕捉 EOF 死锁。"""
        before = snapshot(self.root)
        process = self.launch()
        process.stdin.write(json.dumps(request) + '\n')
        process.stdin.flush()
        line = process.stdout.readline()
        self.assertTrue(line, process.stderr.read())
        result = json.loads(line)
        self.assertEqual(process.wait(timeout=10), 0, process.stderr.read())
        self.assertEqual(process.stdout.read(), '')
        self.assertEqual(snapshot(self.root), before)
        process.stdin.close()
        process.stdout.close()
        process.stderr.close()
        return result

    def ok(self, request):
        """成功必须包含完整图与逐文件证据。"""
        result = self.run_request(request)
        self.assertEqual(result['outcome'], 'ok', result)
        self.assertEqual(len(result['evidence']['files']), len(request['input']['files']))
        return result['graph']

    def test_five_union_cases_both_languages(self):
        """真实 AST 验证两种语言的关系并集、同名片段、原始类型和真实冲突。"""
        for mode in ('c', 'cpp'):
            suffix = '.c' if mode == 'c' else '.cpp'
            graph = self.ok(self.prepare({'main' + suffix: CALLS}, mode))
            by_name = {}
            for node in graph['nodes']:
                by_name.setdefault(node['name'], []).append(node)
            self.assertEqual(len(by_name['mode']), 2)
            self.assertEqual(len([r for r in by_name['run'][0]['references'] if r['semantic'] == 'code.calls']), 2)
            self.assertEqual(len(by_name['branch'][0]['references']), 2)
            self.assertIn('RESULT', by_name['value'][0]['signature'])
            reordered = self.ok(self.prepare({'main' + suffix: CALLS}, mode, (1, 0)))
            self.assertEqual(graph, reordered)
            for source in (CATEGORY, PARENT):
                error = self.run_request(self.prepare({'main' + suffix: source}, mode))
                self.assertEqual(error['error']['code'], 'union_conflict', error)
                self.assertNotIn('graph', error)

    def test_overload_cross_tu_header_and_indirect(self):
        """重载和跨 TU 定义以原生绑定关联，函数指针保持未知。"""
        sources = {'api.h': 'int pick(int x);\ndouble pick(double x);\n',
                   'api.cpp': '#include "api.h"\nint pick(int x){return x;}\ndouble pick(double x){return x;}\n',
                   'main.cpp': '#include "api.h"\nint run(int(*fp)(int)){return pick(1)+fp(2);}\n'}
        graph = self.ok(self.prepare(sources))
        run = next(n for n in graph['nodes'] if n['name'] == 'run')
        self.assertEqual({r['target']['kind'] for r in run['references']}, {'node', 'unresolved'})
        target = next(r['target']['id'] for r in run['references'] if r['target']['kind'] == 'node')
        definition = next(n for n in graph['nodes'] if n['id'] == target)
        self.assertIn('api.cpp', definition['id'])
        self.assertEqual(len([n for n in graph['nodes'] if n['name'] == 'pick']), 4)
        graph = self.ok(self.prepare(sources, selected=['main.cpp']))
        self.assertEqual(graph['external_symbols'][0]['scope'], 'repository')

    def test_cpp_and_c_are_separate(self):
        """C 的隐式 void 指针转换不能伪装为合法 C++。"""
        source = 'void convert(void *p){int *n=p;(void)n;}\n'
        self.ok(self.prepare({'main.c': source}, 'c'))
        result = self.run_request(self.prepare({'main.cpp': source}, 'cpp'))
        self.assertEqual(result['error']['code'], 'object_extraction_failed')

    def test_stable_language_standards_and_native_aliases(self):
        """逐 TU 的原生标准宏与 GNU 模式证明标准和 LLVM 别名没有被强制升级。"""
        dialects = [
            ('c', None, ('c89', 'c90', 'iso9899:1990', 'gnu89', 'gnu90')),
            ('c', 199409, ('iso9899:199409',)),
            ('c', 199901, ('c99', 'iso9899:1999', 'c9x', 'gnu99', 'gnu9x')),
            ('c', 201112, ('c11', 'iso9899:2011', 'c1x', 'gnu11', 'gnu1x')),
            ('c', 201710, ('c17', 'c18', 'iso9899:2017', 'iso9899:2018', 'gnu17', 'gnu18')),
            ('c', 202311, ('c23', 'iso9899:2024', 'c2x', 'gnu23', 'gnu2x')),
            ('cpp', 199711, ('c++98', 'c++03', 'gnu++98', 'gnu++03')),
            ('cpp', 201103, ('c++11', 'c++0x', 'gnu++11')),
            ('cpp', 201402, ('c++14', 'c++1y', 'gnu++14')),
            ('cpp', 201703, ('c++17', 'c++1z', 'gnu++17')),
            ('cpp', 202002, ('c++20', 'c++2a', 'gnu++20')),
            ('cpp', 202302, ('c++23', 'c++2b', 'gnu++23')),
        ]
        for mode, version, standards in dialects:
            macro = '__STDC_VERSION__' if mode == 'c' else '__cplusplus'
            version_check = '#ifdef ' + macro if version is None else f'#if {macro} != {version}L'
            for standard in standards:
                with self.subTest(standard=standard):
                    strict_check = '#ifdef' if standard.startswith('gnu') else '#ifndef'
                    source = (version_check + '\n#error incorrect language version\n#endif\n'
                              + strict_check + ' __STRICT_ANSI__\n#error incorrect GNU mode\n#endif\n'
                              + 'int dialect(void){return 1;}\n')
                    name = 'main.c' if mode == 'c' else 'main.cpp'
                    graph = self.ok(self.prepare({name: source}, mode, (0,), standard=standard))
                    self.assertEqual([(node['name'], node['language']) for node in graph['nodes']],
                                     [('dialect', mode)])

    def test_language_features_require_the_selected_standard(self):
        """固定 LLVM 已实现的稳定语法仅在项目选定的版本合法，旧标准必须报告错误。"""
        cases = [
            ('c', 'c99', 'c89', 'int feature(void){int sum=0;for(int i=0;i<3;++i)sum+=i;return sum;}'),
            ('c', 'c11', 'c99', 'int feature(void){return _Generic(1,int:11,default:0);}'),
            ('c', 'c23', 'c17', 'constexpr int answer=23; int feature(void){return answer;}'),
            ('cpp', 'c++11', 'c++98', 'constexpr int feature(){return 11;}'),
            ('cpp', 'c++14', 'c++11', 'constexpr int feature(){int value=14;return value;}'),
            ('cpp', 'c++17', 'c++14', 'int feature(){if constexpr(true)return 17;else return 0;}'),
            ('cpp', 'c++20', 'c++17', 'consteval int feature(){return 20;}'),
            ('cpp', 'c++23', 'c++20', 'constexpr int feature(){if consteval{return 23;}else{return 0;}}'),
        ]
        cases += [(mode, supported.replace('c', 'gnu', 1), older.replace('c', 'gnu', 1), source)
                  for mode, supported, older, source in cases]
        for mode, supported, older, source in cases:
            with self.subTest(supported=supported, rejected=older):
                name = 'main.c' if mode == 'c' else 'main.cpp'
                graph = self.ok(self.prepare({name: source}, mode, (0,), standard=supported))
                self.assertTrue(any(node['name'] == 'feature' for node in graph['nodes']))
                result = self.run_request(self.prepare({name: source}, mode, (0,), standard=older))
                self.assertEqual(result['error']['code'], 'object_extraction_failed', result)
                self.assertNotIn('graph', result)

    def test_gnu_dialect_preserves_gnu_syntax(self):
        """GNU 模式真实接受 typeof 和 statement expression，ISO 模式继续拒绝相同扩展。"""
        source = 'typeof(1) value; int feature(void){return ({typeof(value) local=1;local;});}\n'
        for mode, standard, strict in [('c', 'gnu99', 'c99'), ('c', 'gnu23', 'c23'),
                                       ('cpp', 'gnu++11', 'c++11'), ('cpp', 'gnu++23', 'c++23')]:
            with self.subTest(standard=standard):
                name = 'main.c' if mode == 'c' else 'main.cpp'
                graph = self.ok(self.prepare({name: source}, mode, (0,), standard=standard))
                self.assertEqual({node['name'] for node in graph['nodes']}, {'value', 'feature'})
                result = self.run_request(self.prepare({name: source}, mode, (0,), standard=strict))
                self.assertEqual(result['error']['code'], 'object_extraction_failed', result)

    def test_unknown_preview_and_mismatched_standards_fail(self):
        """未知标准、LLVM 中仍为草案的标准及语言错配都不会转为默认标准。"""
        cases = [('c', 'c1000'), ('cpp', 'c++999'), ('cpp', 'c++latest'),
                 ('c', 'c2y'), ('c', 'gnu2y'), ('cpp', 'c++2c'), ('cpp', 'c++26'),
                 ('cpp', 'gnu++2c'), ('cpp', 'gnu++26'), ('c', 'cl1.2'), ('cpp', 'hlsl2021'),
                 ('c', 'c++11'), ('cpp', 'c11')]
        for mode, standard in cases:
            with self.subTest(mode=mode, standard=standard):
                name = 'main.c' if mode == 'c' else 'main.cpp'
                result = self.run_request(self.prepare({name: 'int value;'}, mode, (0,), standard=standard))
                self.assertEqual(result['error']['code'], 'unsupported_configuration', result)
                self.assertIn(standard, result['error']['message'])
                self.assertNotIn('graph', result)

    def test_standard_option_order_and_aliases(self):
        """标准参数按 driver 的最后一次有效选择生效，但未知或空值不能被后项掩盖。"""
        source = '#if __cplusplus != 201703L\n#error incorrect final standard\n#endif\nint chosen;\n'
        for extra in (['-std=c++17'], ['--std=c++17'], ['--std', 'c++17']):
            with self.subTest(extra=extra):
                self.ok(self.prepare({'main.cpp': source}, order=(0,), standard='c++11', extra=extra))
        for extra in (['-std='], ['-std=invalid', '-std=c++17'], ['--std']):
            with self.subTest(rejected=extra):
                result = self.run_request(self.prepare({'main.cpp': 'int value;'}, order=(0,), extra=extra))
                self.assertEqual(result['error']['code'], 'unsupported_configuration', result)

    def test_cdb_language_inference_and_positional_override(self):
        """原生后缀、C++ driver 和输入之前的 -x 决定 TU 语言，输入之后的 -x 不追溯改变。"""
        cases = [
            ('main.c', 'c99', ['clang'], [], 'c'),
            ('main.c', 'c++11', ['clang++'], [], 'cpp'),
            ('main.c', 'c++11', ['g++'], [], 'cpp'),
            ('capital.C', 'c++17', ['clang'], [], 'cpp'),
            ('main.cc', 'c++14', ['clang'], [], 'cpp'),
            ('main.cxx', 'c++23', ['clang'], [], 'cpp'),
            ('main.cpp', 'c99', ['clang', '-x', 'c'], [], 'c'),
            ('main.c', 'c++17', ['clang', '-xc++'], [], 'cpp'),
            ('main.c', 'c11', ['clang', '-xc++', '-x', 'none'], [], 'c'),
            ('main.c', 'c99', ['clang'], ['-x', 'c++'], 'c'),
        ]
        for name, standard, prefix, suffix, language in cases:
            with self.subTest(name=name, standard=standard, prefix=prefix, suffix=suffix):
                request = self.prepare({name: 'int value;'}, order=(0,))
                command = {'directory': str(self.root), 'file': name,
                           'arguments': [*prefix, name, '-std=' + standard, *suffix]}
                (self.root / 'compile_commands.json').write_text(json.dumps([command]))
                graph = self.ok(request)
                self.assertEqual(graph['nodes'][0]['language'], language)
        for arguments in (['clang++', '-std=c17', 'main.c'],
                          ['clang', '-std=c++11', 'main.c', '-x', 'c++']):
            request = self.prepare({'main.c': 'int value;'}, 'c', (0,))
            (self.root / 'compile_commands.json').write_text(json.dumps([
                {'directory': str(self.root), 'file': 'main.c', 'arguments': arguments}]))
            self.assertEqual(self.run_request(request)['error']['code'], 'unsupported_configuration')

    def test_source_argument_suffix_determines_language(self):
        """语言推断使用命令中的源路径后缀，文件安全边界仍以链接解析后的真实路径检查。"""
        request = self.prepare({'implementation.cpp': 'void convert(void *p){int *n=p;(void)n;}\n'}, order=(0,))
        (self.root / 'entry.c').symlink_to(self.root / 'implementation.cpp')
        (self.root / 'compile_commands.json').write_text(json.dumps([
            {'directory': str(self.root), 'file': 'entry.c',
             'arguments': ['clang', '-std=c99', 'entry.c']}]))
        graph = self.ok(request)
        self.assertEqual(graph['nodes'][0]['language'], 'c')

    def test_shared_header_union_across_language_standards(self):
        """同一头文件的四种稳定 C/C++ 上下文分别解析，条件分支和共同声明保留现有并集规则。"""
        sources = {'api.h': '#if __cplusplus >= 202302L || __STDC_VERSION__ >= 202311L\n'
                           'int modern(void);\n#else\nint legacy(void);\n#endif\nint common(void);\n'}
        modes = [('a.c', 'c', 'c99'), ('b.c', 'c', 'c23'),
                 ('first.cpp', 'c++', 'c++11'), ('second.cpp', 'c++', 'c++23')]
        commands = []
        for name, mode, standard in modes:
            sources[name] = '#include "api.h"\n'
            commands.append({'directory': str(self.root), 'file': name,
                             'arguments': ['clang', '-x', mode, '-std=' + standard, name]})
        request = self.prepare(sources, order=(0,))
        path = self.root / 'compile_commands.json'
        path.write_text(json.dumps(commands))
        result = self.run_request(request)
        self.assertEqual(result['outcome'], 'ok', result)
        graph = result['graph']
        self.assertEqual({node['name'] for node in graph['nodes']}, {'legacy', 'modern', 'common'})
        self.assertEqual(len(graph['nodes']), 3)
        coverage = next(item for item in result['evidence']['files'] if item['path'] == 'api.h')
        self.assertEqual(len(coverage['configuration_ids']), 4)
        path.write_text(json.dumps(list(reversed(commands))))
        self.assertEqual(graph, self.ok(request))

    def test_cdb_and_explicit_standard_remain_required(self):
        """缺少 CDB 或 -std 必须明确失败，目标 SDK 不再提供会覆盖项目标准的重复字段。"""
        request = self.prepare({'main.cpp': 'int value;'}, order=(0,))
        path = self.root / 'compile_commands.json'
        path.unlink()
        self.assertEqual(self.run_request(request)['error']['code'], 'unsupported_configuration')
        request = self.prepare({'main.cpp': 'int value;'}, order=(0,))
        commands = json.loads(path.read_text())
        commands[0]['arguments'] = [argument for argument in commands[0]['arguments']
                                   if not argument.startswith('-std=')]
        path.write_text(json.dumps(commands))
        result = self.run_request(request)
        self.assertEqual(result['error']['code'], 'unsupported_configuration')
        self.assertIn('explicit -std', result['error']['message'])
        request = self.prepare({'main.cpp': 'int value;'}, order=(0,))
        request['environment']['platforms'][0]['metadata']['cpp_standard'] = 'c++20'
        self.assertEqual(self.run_request(request)['error']['code'], 'invalid_input')

    def test_missing_header_and_orphan_selected(self):
        """缺头文件与无 TU 上下文的选中头文件必须失败。"""
        for sources in ({'main.cpp': '#include "missing.h"\nint run(){return 1;}\n'},
                        {'main.cpp': 'int run(){return 1;}\n', 'orphan.h': 'int lost();\n'}):
            result = self.run_request(self.prepare(sources))
            self.assertEqual(result['error']['code'], 'object_extraction_failed')

    def test_argument_and_response_rejections(self):
        """插件、模块构建、工具查询和响应文件不会被执行。"""
        for arg in ('-fplugin=evil.so', '-Xclang', '@evil.rsp', '-fmodules', '--config=evil.cfg',
                    '--query-driver=evil', '-funknown-option'):
            result = self.run_request(self.prepare({'main.cpp': 'int f(){return 1;}'}, extra=[arg]))
            self.assertEqual(result['error']['code'], 'unsupported_configuration', result)

    def test_source_symlink_and_business_cache_denied(self):
        """仓内 include 链接与业务缓存不可绕过 VFS 来源检查。"""
        outside = self.base / 'outside.h'
        outside.write_text('int outside();')
        (self.root / 'escape.h').symlink_to(outside)
        result = self.run_request(self.prepare({'main.cpp': '#include "escape.h"\nint f(){return 1;}'}))
        self.assertEqual(result['error']['code'], 'object_extraction_failed')
        result = self.run_request(self.prepare({'main.cpp': '#include "vendor/dep.h"\nint f(){return 1;}',
                                               'vendor/dep.h': 'int dep();'}, selected=['main.cpp']))
        self.assertEqual(result['error']['code'], 'object_extraction_failed')

    def test_projection_and_stable_ids(self):
        """格式注释与子方法正文不污染父类型，移动行号不改变对象身份。"""
        first = self.ok(self.prepare({'main.cpp': 'struct Box { int value; int run(){return 1;} };\n'}))
        second = self.ok(self.prepare({'main.cpp': '// lead\n\nstruct Box { /* hi */ int value; int run() { return 2; } };\n'}))
        a = {n['name']: n for n in first['nodes']}
        b = {n['name']: n for n in second['nodes']}
        for name in a:
            self.assertEqual(a[name]['id'], b[name]['id'])
        self.assertEqual(a['Box']['code'], b['Box']['code'])
        self.assertEqual(a['Box']['signature'], b['Box']['signature'])
        self.assertNotEqual(a['run']['code'], b['run']['code'])

    def test_templates_anonymous_and_literals(self):
        """模板实例不复制显式声明，匿名类型和字面量保留源对象。"""
        source = 'template<class T> T id(T x){return x;}\nstruct { int n; } data;\nint f(){const char *s=R"(// literal)";return id(1);}\n'
        graph = self.ok(self.prepare({'main.cpp': source}))
        self.assertEqual(len([n for n in graph['nodes'] if n['name'] == 'id']), 1)
        self.assertTrue(any(n['name'] == '<anonymous>' for n in graph['nodes']))
        self.assertIn('// literal', next(n for n in graph['nodes'] if n['name'] == 'f')['code'])

    def test_cancel_eof_and_result_limit(self):
        """取消在原生分析外读取，终结自行退出，结果限额没有部分图。"""
        request = self.prepare({'main.cpp': '\n'.join(f'int f{i}(){{return {i};}}' for i in range(20000))})
        process = self.launch()
        process.stdin.write(json.dumps(request) + '\n' + json.dumps({'protocol': PROTOCOL, 'type': 'cancel', 'id': 'test'}) + '\n')
        process.stdin.flush()
        result = json.loads(process.stdout.readline())
        self.assertEqual(result['outcome'], 'cancelled', result)
        self.assertEqual(process.wait(timeout=5), 0)
        process.stdin.close(); process.stdout.close(); process.stderr.close()
        process = self.launch()
        process.stdin.write(json.dumps(request) + '\n'); process.stdin.flush(); process.stdin.close()
        self.assertNotEqual(process.wait(timeout=5), 0)
        process.stdout.close(); process.stderr.close()
        request = self.prepare({'main.cpp': 'int f(){return 1;}'})
        request['limits']['max_result_bytes'] = 20
        result = self.run_request(request)
        self.assertEqual(result['error']['code'], 'resource_limit')

    def test_mixed_c_cpp_shared_header(self):
        """每个 TU 保留语言模式，同一兼容头文件声明只生成一个源对象。"""
        request = self.prepare({'api.h': 'struct Shared { int field; };\nint common(void);\n',
                                'a.c': '#include "api.h"\nint from_c(void){return common();}\n',
                                'b.cpp': '#include "api.h"\nint from_cpp(){return common();}\n'})
        path = self.root / 'compile_commands.json'
        commands = json.loads(path.read_text())
        for command in commands:
            if command['file'] == 'a.c':
                command['arguments'][2] = 'c'
                command['arguments'][3] = '-std=c17'
        path.write_text(json.dumps(commands))
        graph = self.ok(request)
        self.assertEqual(len([n for n in graph['nodes'] if n['name'] == 'Shared']), 1)
        self.assertEqual(len([n for n in graph['nodes'] if n['name'] == 'common']), 1)
        self.assertEqual(next(n for n in graph['nodes'] if n['name'] == 'from_c')['language'], 'c')
        self.assertEqual(next(n for n in graph['nodes'] if n['name'] == 'from_cpp')['language'], 'cpp')
        path.write_text(json.dumps(list(reversed(commands))))
        self.assertEqual(graph, self.ok(request))

    def test_resolved_and_unknown_union(self):
        """某配置的确定目标不能掩盖另一配置的未知函数指针调用。"""
        source = '#if FAST\nint action(void){return 1;}\n#else\nint (*action)(void);\n#endif\nint run(void){return action();}\n'
        graph = self.ok(self.prepare({'main.cpp': source}))
        run = next(n for n in graph['nodes'] if n['name'] == 'run')
        self.assertEqual({r['target']['kind'] for r in run['references']}, {'node', 'unresolved'})

    def test_target_sdk_standard_headers(self):
        """明确 catalog 的本机 SDK 提供 C/C++ 标准头，不隐式使用宿主搜索路径。"""
        sdk = Path(os.environ.get('NARRANT_MACOS_SDK', '/Library/Developer/CommandLineTools/SDKs/MacOSX.sdk'))
        if not sdk.exists():
            if os.name == 'nt':
                self.skipTest('macOS SDK case is not applicable on Windows')
            self.fail('macOS SDK required for standard-header acceptance')
        settings = json.loads((sdk / 'SDKSettings.json').read_text())
        version = settings['Version']
        deployment = settings.get('DefaultProperties', {}).get('MACOSX_DEPLOYMENT_TARGET', version)
        for mode, source in [('c', '#include <stdint.h>\n#include <stdio.h>\nint f(void){uint32_t n=1;return puts("hello")+(int)n;}\n'),
                             ('cpp', '#include <vector>\nint f(){std::vector<int> v; v.push_back(1);return (int)v.size();}\n')]:
            request = self.prepare({'main.c' if mode == 'c' else 'main.cpp': source}, mode, (0,))
            target = request['environment']['platforms'][0]
            target['root'] = str(sdk)
            target['version'] = version
            target['metadata']['target_triple'] = 'arm64-apple-macosx' + deployment
            target['metadata']['system_include_directories'] = ['usr/include/c++/v1', 'usr/include']
            graph = self.ok(request)
            self.assertTrue(graph['nodes'])
            self.assertTrue(graph['external_symbols'])
            self.assertTrue(all(x['scope'] == 'dependency' for x in graph['external_symbols']))

    def test_macro_definition_body_is_compared(self):
        """宏定义正文独立参与比较，格式、注释和续行不替代原始 token。"""
        source = '#define PLUS(x) \\\n ((x) + 1)\nint f(){return PLUS(2); }\n'
        first = self.ok(self.prepare({'main.cpp': source}))
        second = self.ok(self.prepare({'main.cpp': source.replace('+ 1', '+ 2')}))
        before = next(n for n in first['nodes'] if n['name'] == 'PLUS')
        after = next(n for n in second['nodes'] if n['name'] == 'PLUS')
        self.assertEqual(before['id'], after['id'])
        self.assertNotEqual(before['code'], after['code'])
        function = next(n for n in first['nodes'] if n['name'] == 'f')
        self.assertTrue(any(r['semantic'] == 'code.references' and r['target'].get('id') == before['id']
                            for r in function['references']))

    def test_overloads_reorder_without_identity_changes(self):
        """唯一重载按源码参数槽定位，不依赖 AST 顺序或行号。"""
        a = 'int pick(int x){return x;}\n'
        b = 'double pick(double y){return y;}\n'
        first = self.ok(self.prepare({'main.cpp': a + b}))
        second = self.ok(self.prepare({'main.cpp': b + a}))
        self.assertEqual({n['signature']: n['id'] for n in first['nodes']},
                         {n['signature']: n['id'] for n in second['nodes']})

    def test_additional_system_header_catalog(self):
        """分离的标准头根通过 ID 授权，来源、宏及证据不依赖共同父目录。"""
        headers = self.base / 'approved-msvc-headers'
        headers.mkdir()
        (headers / 'extra.h').write_text('#define COUNT 2\nint platform_function(void);\n')
        request = self.prepare({'main.cpp': '#include <extra.h>\nint f(){return platform_function()+COUNT;}\n'})
        target = request['environment']['platforms'][0]
        target['metadata']['additional_system_platform_ids'] = ['msvc-headers']
        request['environment']['platforms'].append({'id': 'msvc-headers', 'kind': 'clang-system-headers',
            'root': str(headers), 'version': 'fixture-1', 'metadata': {}})
        result = self.run_request(request)
        self.assertEqual(result['outcome'], 'ok', result)
        self.assertEqual(result['graph']['external_symbols'][0]['scope'], 'dependency')
        self.assertTrue(all('msvc-headers' in x['platform_ids'] for x in result['evidence']['configurations']))
        target['metadata']['additional_system_platform_ids'] = ['msvc-headers', 'msvc-headers']
        self.assertEqual(self.run_request(request)['error']['code'], 'invalid_input')
        target['metadata']['additional_system_platform_ids'] = ['missing']
        self.assertEqual(self.run_request(request)['error']['code'], 'target_unavailable')

    def test_unicode_source_path(self):
        """协议路径按 UTF-8 往返，空格和中文不经过 shell 或系统 ANSI 编码。"""
        graph = self.ok(self.prepare({'源码 space.cpp': 'int value(){return 1;}\n'}))
        self.assertEqual(graph['nodes'][0]['name'], 'value')

    def test_windows_sdk_standard_headers(self):
        """Windows 原生 CI 使用精确 SDK Include 根与独立 MSVC include 根。"""
        sdk = os.environ.get('NARRANT_WINDOWS_SDK')
        msvc = os.environ.get('NARRANT_MSVC_ROOT')
        if not sdk or not msvc:
            self.skipTest('Windows SDK and MSVC roots not provided; not a Windows pass')
        for mode, source in [('c', '#include <stdint.h>\n#include <stdio.h>\nint f(void){uint32_t n=1;return puts("hello")+(int)n;}\n'),
                             ('cpp', '#include <vector>\nint f(){std::vector<int> v;v.push_back(1);return (int)v.size();}\n')]:
            request = self.prepare({'main.c' if mode == 'c' else 'main.cpp': source}, mode, (0,))
            target = request['environment']['platforms'][0]
            target['root'] = sdk
            target['version'] = Path(sdk).name
            target['metadata']['target_triple'] = 'x86_64-pc-windows-msvc'
            target['metadata']['system_include_directories'] = ['ucrt', 'shared', 'um', 'winrt']
            target['metadata']['additional_system_platform_ids'] = ['msvc']
            request['environment']['platforms'].append({'id': 'msvc', 'kind': 'clang-system-headers',
                'root': str(Path(msvc) / 'include'), 'version': Path(msvc).name, 'metadata': {}})
            graph = self.ok(request)
            self.assertTrue(graph['nodes'])
            self.assertTrue(graph['external_symbols'])

    def test_review_terminal_tokens_are_preserved(self):
        """审查回归：缓冲区末尾宏数字和 enum initializer 的真实 token 不得丢失。"""
        first = self.ok(self.prepare({'main.cpp': '#define NUMBER 1\nenum E { A=1, B=2 };\n'}))
        second = self.ok(self.prepare({'main.cpp': '#define NUMBER 9\nenum E { A=9, B=8 };\n'}))
        a = {n['name']: n for n in first['nodes']}
        b = {n['name']: n for n in second['nodes']}
        for name in ('NUMBER', 'A', 'B'):
            self.assertNotEqual(a[name]['code'], b[name]['code'], name)
        self.assertIn('1', a['NUMBER']['code'])
        self.assertIn('2', a['B']['code'])

    def test_review_optimization_predefined_macros(self):
        """审查回归：优化参数影响预定义宏，因此必须传入真实 Clang 上下文。"""
        source = '#ifdef __OPTIMIZE__\nint optimized;\n#else\nint plain;\n#endif\n'
        plain = self.ok(self.prepare({'main.cpp': source}, order=(0,), extra=['-O0']))
        optimized = self.ok(self.prepare({'main.cpp': source}, order=(0,), extra=['-O2']))
        self.assertEqual([n['name'] for n in plain['nodes']], ['plain'])
        self.assertEqual([n['name'] for n in optimized['nodes']], ['optimized'])

    def test_review_linkage_signature_owns_only_header(self):
        """审查回归：extern C 扩展的 signature/code 不包含独立子函数正文。"""
        for first_source, second_source in [('extern "C" { int f(){return 1;} }', 'extern "C" { int f(){return 2;} }'),
                                            ('extern "C" int f(){return 1;}', 'extern "C" int f(){return 2;}')]:
            first = self.ok(self.prepare({'main.cpp': first_source}))
            second = self.ok(self.prepare({'main.cpp': second_source}))
            a = next(n for n in first['nodes'] if n['native_kind'] == 'LinkageSpec')
            b = next(n for n in second['nodes'] if n['native_kind'] == 'LinkageSpec')
            self.assertEqual(a['signature'], b['signature'])
            self.assertEqual(a['code'], b['code'])
            self.assertNotIn('return', a['signature'])

    def test_review_multiple_declarators_are_independent(self):
        """审查回归：普通全局变量、字段、指针与数组 declarator 的自身内容相互独立。"""
        for mode, source in [('cpp', 'int a=1, b=2;'), ('cpp', 'struct Box { int a=1, b=2; };'),
                             ('cpp', 'const char *a="one", *b="two";'), ('cpp', 'int a[2]={1,2}, b[2]={3,4};'),
                             ('c', 'int a=1, b=2;'), ('c', 'struct Box { int a[2], b[2]; };')]:
            changed = source.replace('a=1', 'a=3').replace('a="one"', 'a="new"').replace('a[2]={1,2}', 'a[2]={5,6}')
            if mode == 'c':
                changed = changed.replace('a[2]', 'a[3]')
            file = 'main.c' if mode == 'c' else 'main.cpp'
            first = self.ok(self.prepare({file: source}, mode))
            second = self.ok(self.prepare({file: changed}, mode))
            a = {n['name']: n for n in first['nodes']}
            b = {n['name']: n for n in second['nodes']}
            self.assertNotEqual(a['a']['code'], b['a']['code'], source)
            self.assertEqual(a['b']['signature'], b['b']['signature'], source)
            self.assertEqual(a['b']['code'], b['b']['code'], source)
            self.assertEqual(a['b']['id'], b['b']['id'], source)
            if 'Box' in a:
                self.assertEqual(a['Box']['code'], b['Box']['code'])

    def test_review_member_pointer_declarator_prefix(self):
        """复审回归：成员指针的完整限定名属于 a，不能泄漏到共享声明中的 int b。"""
        for declarations, left, right in [('struct C {}; struct D {};', 'C', 'D'),
                                           ('namespace N { struct C {}; struct D {}; }', 'N::C', 'N::D')]:
            for declarator in ('{owner}::*a=nullptr', '{owner}::*a[2]={{}}'):
                first_source = declarations + ' int ' + declarator.format(owner=left) + ', b=2;'
                second_source = declarations + ' int ' + declarator.format(owner=right) + ', b=2;'
                first = self.ok(self.prepare({'main.cpp': first_source}))
                second = self.ok(self.prepare({'main.cpp': second_source}))
                a = {n['name']: n for n in first['nodes']}
                b = {n['name']: n for n in second['nodes']}
                self.assertEqual(a['b']['signature'], 'int b = 2 ;')
                self.assertEqual(a['b']['code'], 'int b = 2 ;')
                self.assertEqual(a['b']['signature'], b['b']['signature'])
                self.assertEqual(a['b']['code'], b['b']['code'])
                self.assertEqual(a['b']['id'], b['b']['id'])
                self.assertNotEqual(a['a']['code'], b['a']['code'])

    def test_duplicate_protocol_keys(self):
        """协议拒绝重复键而不是悄悄覆盖。"""
        process = self.launch()
        process.stdin.write('{"protocol":"x","protocol":"y"}\n'); process.stdin.flush()
        self.assertNotEqual(process.wait(timeout=5), 0)
        self.assertEqual(process.stdout.read(), '')
        process.stdin.close(); process.stdout.close(); process.stderr.close()


if __name__ == '__main__':
    unittest.main(verbosity=2)
