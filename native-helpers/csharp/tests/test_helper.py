"""C# helper 的独立进程集成测试；只分析临时源码。"""

import hashlib
import json
import os
from pathlib import Path
import subprocess
import tempfile
import unittest


DOTNET = os.environ.get("NARRANT_TEST_DOTNET", "/tmp/narrant-dotnet-10.0.100/dotnet")
REFS = os.environ.get(
    "NARRANT_TEST_REFS",
    "/tmp/narrant-dotnet-10.0.100/packs/Microsoft.NETCore.App.Ref/10.0.0/ref/net10.0",
)
REFS8 = os.environ.get("NARRANT_TEST_REFS8", "/private/tmp/narrant-dotnet-ref8-8.0.0/ref/net8.0")
REFS9 = os.environ.get("NARRANT_TEST_REFS9")
REFS9_VERSION = os.environ.get("NARRANT_TEST_REFS9_VERSION", "9.0.20")
HELPER = Path(os.environ.get("NARRANT_TEST_HELPER", str(
    Path(__file__).resolve().parents[1] / "bin/Release/net10.0/Narrant.CSharp.Helper.dll")))


class HelperTests(unittest.TestCase):
    """通过 NDJSON 真实会话验证源码、项目模型、目标与取消边界。"""

    def setUp(self):
        """为每个案例建立独立的只读候选仓库与 scratch。"""
        self.temp = tempfile.TemporaryDirectory(prefix="narrant-csharp-test-")
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name) / "repo"
        self.root.mkdir()
        self.scratch = Path(self.temp.name) / "scratch"
        self.scratch.mkdir()

    def write(self, relative, text):
        """在临时仓库写入案例输入。"""
        path = self.root / relative
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(text, encoding="utf-8")

    def source_set(self, configs):
        """写入确定性 source-set 声明。"""
        self.write("narrant.csharp.json", json.dumps({"version": "csharp-static-v1", "configurations": configs}))

    def config(self, name, files, defines=(), tfm="net10.0", language_version="14.0"):
        """生成明确 TFM、语言版本的独立静态上下文。"""
        return {"id": name, "files": files, "tfm": tfm, "language_version": language_version,
                "define_constants": list(defines), "implicit_usings": False}

    def platform(self, tfm="net10.0", version="10.0.0", root=REFS):
        """生成宿主已选定的引用目录，补丁版本只属于测试夹具。"""
        return {"id": tfm, "kind": "dotnet-reference-assemblies", "version": version, "root": root,
                "metadata": {"tfm": tfm, "reference_pack": "Microsoft.NETCore.App.Ref",
                             "reference_pack_version": version}}

    def request(self, files, platforms=None, timeout=120000, include_tests=False):
        """生成宿主侧 v1 请求，平台由案例显式提供。"""
        if platforms is None:
            platforms = [self.platform()]
        return {"protocol": "narrant.native-analyzer/1", "type": "analyze", "id": "fixture",
                "input": {"repository_root": str(self.root), "files": files,
                          "include_tests": include_tests, "profile": "csharp-static-v1"},
                "environment": {"scratch_root": str(self.scratch), "platforms": platforms},
                "limits": {"analysis_timeout_ms": timeout, "max_result_bytes": 134217728,
                           "max_diagnostic_bytes": 262144}}

    def run_helper(self, request, extra_line=None):
        """保持 stdin 打开直到终结消息，以模拟真实 Host 生命周期。"""
        env = {**os.environ, "DOTNET_ROOT": str(Path(DOTNET).parent), "DOTNET_MULTILEVEL_LOOKUP": "0"}
        proc = subprocess.Popen([DOTNET, str(HELPER)], stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                                stderr=subprocess.PIPE, text=True, env=env)
        try:
            ready = json.loads(proc.stdout.readline())
            self.assertEqual(ready["type"], "ready")
            proc.stdin.write(json.dumps(request) + "\n")
            if extra_line is not None:
                proc.stdin.write(extra_line + "\n")
            proc.stdin.flush()
            completed = json.loads(proc.stdout.readline())
            self.assertEqual(completed["type"], "completed")
            proc.stdin.close()
            self.assertEqual(proc.wait(timeout=10), 0, proc.stderr.read())
            self.assertEqual(proc.stdout.read(), "")
            return completed
        finally:
            if proc.poll() is None:
                proc.kill()
                proc.wait()
            proc.stdin.close() if not proc.stdin.closed else None
            proc.stdout.close()
            proc.stderr.close()

    def test_partial_overload_and_read_only(self):
        """跨文件 partial 的两个重载应保留不同身份和精确调用。"""
        self.write("A.cs", "namespace N; public partial class C { public static int F(int x)=>x; public void G(){F(1);} }")
        self.write("B.cs", "namespace N; public partial class C { public static int F(string x)=>x.Length; }")
        self.source_set([self.config("main", ["A.cs", "B.cs"])])
        before = {p.relative_to(self.root).as_posix(): hashlib.sha256(p.read_bytes()).hexdigest()
                  for p in self.root.rglob("*") if p.is_file()}
        result = self.run_helper(self.request(["A.cs", "B.cs"]))
        self.assertEqual(result["outcome"], "ok")
        nodes = result["graph"]["nodes"]
        overloads = [n for n in nodes if n["name"] == "F"]
        self.assertEqual(len(overloads), 2)
        g = next(n for n in nodes if n["name"] == "G")
        calls = [r["target"]["id"] for r in g["references"] if r["semantic"] == "code.calls"]
        self.assertEqual(calls, [next(n["id"] for n in overloads if "int" in n["signature"])])
        after = {p.relative_to(self.root).as_posix(): hashlib.sha256(p.read_bytes()).hexdigest()
                 for p in self.root.rglob("*") if p.is_file()}
        self.assertEqual(before, after)

    def test_conditional_union_and_unresolved(self):
        """两个明确条件配置的调用事实取并集且未知调用留存。"""
        self.write("A.cs", """class A { public static void F(){} } class B { public static void F(){} }
class C { void G(){
#if LEFT
A.F();
#else
B.F();
#endif
Missing.Call();
} }""")
        self.source_set([self.config("left", ["A.cs"], ["LEFT"]), self.config("right", ["A.cs"])])
        result = self.run_helper(self.request(["A.cs"]))
        self.assertEqual(result["outcome"], "ok", result.get("error"))
        g = next(n for n in result["graph"]["nodes"] if n["name"] == "G")
        calls = [r["target"] for r in g["references"] if r["semantic"] == "code.calls"]
        self.assertEqual(len([c for c in calls if c["kind"] == "node"]), 2)
        self.assertIn("Missing.Call", [c["name"] for c in calls if c["kind"] == "unresolved"])

    def test_conditional_declarations_keep_both_branches(self):
        """条件分支各自声明对象，文件及类型自身投影仍应相同。"""
        self.write("A.cs", """class C {
#if LEFT
void A(){}
#else
void B(){}
#endif
}""")
        self.source_set([self.config("left", ["A.cs"], ["LEFT"]), self.config("right", ["A.cs"])])
        result = self.run_helper(self.request(["A.cs"]))
        self.assertEqual(result["outcome"], "ok", result.get("error"))
        names = [n["name"] for n in result["graph"]["nodes"]]
        self.assertIn("A", names)
        self.assertIn("B", names)

    def test_project_reference_and_static_rejection(self):
        """静态 ProjectReference 能绑定，项目 Target 必须拒绝且不可执行。"""
        self.write("A/A.csproj", '<Project Sdk="Microsoft.NET.Sdk"><PropertyGroup><TargetFramework>net10.0</TargetFramework></PropertyGroup></Project>')
        self.write("B/B.csproj", '<Project Sdk="Microsoft.NET.Sdk"><PropertyGroup><TargetFramework>net10.0</TargetFramework><ImplicitUsings>enable</ImplicitUsings></PropertyGroup><ItemGroup><ProjectReference Include="../A/A.csproj" /></ItemGroup></Project>')
        self.write("A/X.cs", "namespace N; public class X { public static void F(){} }")
        self.write("B/Y.cs", "using N; public class Y { public void G(){ X.F(); } }")
        result = self.run_helper(self.request(["A/X.cs", "B/Y.cs"]))
        self.assertEqual(result["outcome"], "ok", result.get("error"))
        g = next(n for n in result["graph"]["nodes"] if n["name"] == "G")
        self.assertTrue(any(r["target"]["kind"] == "node" for r in g["references"] if r["semantic"] == "code.calls"))
        marker = self.root / "marker"
        self.write("B/B.csproj", '<Project Sdk="Microsoft.NET.Sdk"><PropertyGroup><TargetFramework>net10.0</TargetFramework></PropertyGroup><Target Name="Evil" BeforeTargets="Compile"><WriteLinesToFile File="marker" Lines="bad" /></Target></Project>')
        rejected = self.run_helper(self.request(["B/Y.cs"]))
        self.assertEqual(rejected["error"]["code"], "unsupported_configuration")
        self.assertFalse(marker.exists())

    def test_extension_block_and_interface_relation(self):
        """C# 14 extension block 与接口实现保持独立对象和确认关系。"""
        self.write("A.cs", """interface I { int F(); }
class C : I { public int F() => 1; }
public static class E { extension(string s) { public int Size() => s.Length; } }""")
        self.source_set([self.config("main", ["A.cs"])])
        result = self.run_helper(self.request(["A.cs"]))
        self.assertEqual(result["outcome"], "ok", result.get("error"))
        nodes = result["graph"]["nodes"]
        self.assertEqual(len([n for n in nodes if n["category"] == "extension"]), 1)
        impl = next(n for n in nodes if n["name"] == "F" and "public" in n["signature"])
        self.assertTrue(any(r["semantic"] == "code.implements" for r in impl["references"]))

    def test_conditional_compile_and_unsupported_import(self):
        """等值条件只选确定成员；未知导入完整失败。"""
        self.write("A/A.csproj", '''<Project Sdk="Microsoft.NET.Sdk"><PropertyGroup><TargetFramework>net10.0</TargetFramework><EnableDefaultCompileItems>false</EnableDefaultCompileItems></PropertyGroup>
<ItemGroup><Compile Include="Main.cs" /></ItemGroup><ItemGroup Condition="'$(Configuration)' == 'Debug'"><Compile Include="Extra.cs" /></ItemGroup></Project>''')
        self.write("A/Main.cs", "class MainClass {}")
        self.write("A/Extra.cs", "class ExtraClass {}")
        result = self.run_helper(self.request(["A/Main.cs", "A/Extra.cs"]))
        self.assertEqual(result["outcome"], "ok", result.get("error"))
        self.write("A/A.csproj", '''<Project Sdk="Microsoft.NET.Sdk"><PropertyGroup><TargetFramework>net10.0</TargetFramework><EnableDefaultCompileItems>false</EnableDefaultCompileItems></PropertyGroup>
<ItemGroup><Compile Include="Main.cs" /></ItemGroup><ItemGroup Condition="'$(Configuration)' == 'Release'"><Compile Include="Extra.cs" /></ItemGroup></Project>''')
        release = self.run_helper(self.request(["A/Main.cs", "A/Extra.cs"]))
        self.assertEqual(release["outcome"], "ok", release.get("error"))
        self.assertEqual(next(f for f in release["evidence"]["files"] if f["path"] == "A/Extra.cs")["disposition"], "analyzed")
        self.write("A/A.csproj", '<Project Sdk="Microsoft.NET.Sdk"><PropertyGroup><TargetFramework>net10.0</TargetFramework></PropertyGroup><Import Project="bad.targets" /></Project>')
        rejected = self.run_helper(self.request(["A/Main.cs"]))
        self.assertEqual(rejected["error"]["code"], "unsupported_configuration")

    def test_missing_target(self):
        """无明确目标引用程序集时必须返回 target_unavailable。"""
        self.write("A.cs", "class A{}")
        self.source_set([self.config("main", ["A.cs"])])
        result = self.run_helper(self.request(["A.cs"], platforms=[]))
        self.assertEqual(result["error"]["code"], "target_unavailable")

    def test_host_roslyn_api_does_not_leak_into_target_binding(self):
        """宿主运行时可见的 Roslyn API 不在目标引用内时必须保持未解析。"""
        host_api = HELPER.parent / "Microsoft.CodeAnalysis.CSharp.dll"
        target_api = Path(REFS) / host_api.name
        self.assertTrue(host_api.is_file(), "helper runtime must contain its Roslyn dependency")
        self.assertFalse(target_api.exists(), "the official net10 target references must not contain Roslyn")
        self.write("A.cs", '''class C { void F() {
            Microsoft.CodeAnalysis.CSharp.CSharpCompilation.Create("host-only");
            System.Console.WriteLine("target-api");
        } }''')
        self.source_set([self.config("main", ["A.cs"])])
        result = self.run_helper(self.request(["A.cs"]))
        self.assertEqual(result["outcome"], "ok", result.get("error"))
        method = next(node for node in result["graph"]["nodes"] if node["name"] == "F")
        calls = [ref["target"] for ref in method["references"] if ref["semantic"] == "code.calls"]
        self.assertEqual(len(calls), 2)
        self.assertTrue(any(target["kind"] == "unresolved" and target["name"] ==
                            "Microsoft.CodeAnalysis.CSharp.CSharpCompilation.Create" for target in calls))
        self.assertTrue(any(target["kind"] == "external" for target in calls))

    def test_real_net8_and_net10_target_isolation_and_union(self):
        """同一 .NET 10 宿主以真实 8/10 引用分别绑定并合并目标事实。"""
        self.assertTrue(Path(REFS8).is_dir(), "official net8.0/8.0.0 reference pack is required")
        self.assertGreater(len(list(Path(REFS8).glob("*.dll"))), 10)
        self.assertFalse((Path(REFS8) / "System.Threading.Lock.dll").exists())
        source = '''class C { void F() {
            var gate = new System.Threading.Lock();
            System.Console.WriteLine("shared");
        } }
#if NET8_0
class OnlyNet8 {}
#endif
#if NET10_0
class OnlyNet10 {}
#endif'''
        self.write("A.cs", source)
        net8 = self.config("net8", ["A.cs"], defines=["NET8_0"], tfm="net8.0", language_version="12.0")
        net10 = self.config("net10", ["A.cs"], defines=["NET10_0"])
        platform8 = self.platform("net8.0", "8.0.0", REFS8)
        platform10 = self.request(["A.cs"])["environment"]["platforms"][0]

        self.source_set([self.config("net10-no-defines", ["A.cs"])])
        explicit_only = self.run_helper(self.request(["A.cs"], platforms=[platform10]))
        self.assertEqual(explicit_only["outcome"], "ok", explicit_only.get("error"))
        self.assertNotIn("OnlyNet8", [node["name"] for node in explicit_only["graph"]["nodes"]])
        self.assertNotIn("OnlyNet10", [node["name"] for node in explicit_only["graph"]["nodes"]])

        self.source_set([net8])
        older = self.run_helper(self.request(["A.cs"], platforms=[platform8]))
        self.assertEqual(older["outcome"], "ok", older.get("error"))
        older_nodes = older["graph"]["nodes"]
        self.assertIn("OnlyNet8", [node["name"] for node in older_nodes])
        self.assertNotIn("OnlyNet10", [node["name"] for node in older_nodes])
        older_f = next(node for node in older_nodes if node["name"] == "F")
        older_calls = [ref["target"] for ref in older_f["references"] if ref["semantic"] == "code.calls"]
        self.assertTrue(any(target["kind"] == "unresolved" and target["name"] == "System.Threading.Lock"
                            for target in older_calls))
        self.assertTrue(any(target["kind"] == "external" for target in older_calls))

        self.source_set([net10])
        newer = self.run_helper(self.request(["A.cs"], platforms=[platform10]))
        self.assertEqual(newer["outcome"], "ok", newer.get("error"))
        newer_nodes = newer["graph"]["nodes"]
        self.assertIn("OnlyNet10", [node["name"] for node in newer_nodes])
        self.assertNotIn("OnlyNet8", [node["name"] for node in newer_nodes])
        newer_f = next(node for node in newer_nodes if node["name"] == "F")
        newer_calls = [ref["target"] for ref in newer_f["references"] if ref["semantic"] == "code.calls"]
        self.assertFalse(any(target["kind"] == "unresolved" and target["name"] == "System.Threading.Lock"
                             for target in newer_calls))
        self.assertEqual(len([target for target in newer_calls if target["kind"] == "external"]), 2)
        self.assertEqual(older_f["id"], newer_f["id"])

        self.source_set([net8, net10])
        union = self.run_helper(self.request(["A.cs"], platforms=[platform8, platform10]))
        self.assertEqual(union["outcome"], "ok", union.get("error"))
        names = [node["name"] for node in union["graph"]["nodes"]]
        self.assertIn("OnlyNet8", names)
        self.assertIn("OnlyNet10", names)
        combined_f = next(node for node in union["graph"]["nodes"] if node["name"] == "F")
        self.assertEqual(combined_f["id"], older_f["id"])
        combined_calls = [ref["target"] for ref in combined_f["references"] if ref["semantic"] == "code.calls"]
        self.assertTrue(any(target["kind"] == "unresolved" and target["name"] == "System.Threading.Lock"
                            for target in combined_calls))
        self.assertEqual(len([target for target in combined_calls if target["kind"] == "external"]), 2)

        self.source_set([net10, net8])
        reverse = self.run_helper(self.request(["A.cs"], platforms=[platform8, platform10]))
        self.assertEqual(reverse["outcome"], "ok", reverse.get("error"))
        self.assertEqual(union["graph"], reverse["graph"])

        wrong = dict(platform8, version="10.0.0")
        rejected = self.run_helper(self.request(["A.cs"], platforms=[wrong, platform10]))
        self.assertEqual(rejected["error"]["code"], "target_unavailable")

    def test_sdk_net8_defaults_to_csharp12(self):
        """普通 net8 SDK 项目默认 C# 12，只有明确语言覆盖才接受 C# 14 语法。"""
        self.assertTrue(Path(REFS8).is_dir(), "official net8.0/8.0.0 reference pack is required")
        self.write("App/App.csproj", '<Project Sdk="Microsoft.NET.Sdk"><PropertyGroup><TargetFramework>net8.0</TargetFramework></PropertyGroup></Project>')
        self.write("App/E.cs", "public static class E { extension(string s) { public int Size() => s.Length; } }")
        platform8 = self.platform("net8.0", "8.0.0", REFS8)
        default = self.run_helper(self.request(["App/E.cs"], platforms=[platform8]))
        self.assertEqual(default["outcome"], "error")
        self.assertEqual(default["error"]["code"], "object_extraction_failed")
        self.assertNotIn("graph", default)
        self.write("App/App.csproj", '<Project Sdk="Microsoft.NET.Sdk"><PropertyGroup><TargetFramework>net8.0</TargetFramework><LangVersion>14.0</LangVersion></PropertyGroup></Project>')
        explicit = self.run_helper(self.request(["App/E.cs"], platforms=[platform8]))
        self.assertEqual(explicit["outcome"], "ok", explicit.get("error"))
        self.assertIn("Size", [node["name"] for node in explicit["graph"]["nodes"]])

    def test_platform_protocol_rejects_legacy_and_unknown_fields(self):
        """平台协议不再接受 fingerprint，且仍严格拒绝缺失或未知字段。"""
        for change in ("fingerprint", "unknown", "missing_root"):
            with self.subTest(change=change):
                request = self.request([])
                platform = request["environment"]["platforms"][0]
                if change == "missing_root":
                    del platform["root"]
                else:
                    platform[change] = "unexpected"
                env = {**os.environ, "DOTNET_ROOT": str(Path(DOTNET).parent), "DOTNET_MULTILEVEL_LOOKUP": "0"}
                result = subprocess.run([DOTNET, str(HELPER)], input=json.dumps(request) + "\n",
                                        capture_output=True, text=True, env=env, timeout=10)
                self.assertEqual(result.returncode, 1)
                self.assertEqual([json.loads(line)["type"] for line in result.stdout.splitlines()], ["ready"])
                self.assertIn("Invalid analyze message", result.stderr)

    def test_modern_targets_are_not_a_fixed_allowlist(self):
        """未预置的现代 TFM 和其默认语言可解析，缺本地引用时统一报告 target_unavailable。"""
        self.write("App/A.cs", "class C {}")
        for major in (5, 6, 7, 9):
            with self.subTest(tfm=f"net{major}.0"):
                self.write("App/App.csproj", f'<Project Sdk="Microsoft.NET.Sdk"><PropertyGroup><TargetFramework>net{major}.0</TargetFramework></PropertyGroup></Project>')
                result = self.run_helper(self.request(["App/A.cs"], platforms=[]))
                self.assertEqual(result["error"]["code"], "target_unavailable", result)

    def test_unsupported_target_shapes_fail_explicitly(self):
        """基础现代 .NET 以外的框架、平台后缀和非标准拼写不能静默套用基础目标。"""
        self.write("A.cs", "class C {}")
        for tfm in ("net48", "netstandard2.1", "netcoreapp3.1", "net10.0-windows", "net9.1", "net09.0", "net4.0"):
            with self.subTest(tfm=tfm):
                self.source_set([self.config("main", ["A.cs"], tfm=tfm)])
                result = self.run_helper(self.request(["A.cs"], platforms=[]))
                self.assertEqual(result["error"]["code"], "unsupported_configuration", result)

    def test_future_default_language_does_not_silently_downgrade(self):
        """固定 Roslyn 无法解释未来默认语言时必须失败；显式已有语言仍须匹配本地目标。"""
        self.write("App/A.cs", "class C {}")
        self.write("App/App.csproj", '<Project Sdk="Microsoft.NET.Sdk"><PropertyGroup><TargetFramework>net11.0</TargetFramework></PropertyGroup></Project>')
        default = self.run_helper(self.request(["App/A.cs"], platforms=[]))
        self.assertEqual(default["error"]["code"], "unsupported_configuration", default)
        self.assertIn("default C# 15.0 for net11.0", default["error"]["message"])
        self.write("App/App.csproj", '<Project Sdk="Microsoft.NET.Sdk"><PropertyGroup><TargetFramework>net11.0</TargetFramework><LangVersion>14.0</LangVersion></PropertyGroup></Project>')
        explicit = self.run_helper(self.request(["App/A.cs"], platforms=[]))
        self.assertEqual(explicit["error"]["code"], "target_unavailable", explicit)

    def test_implicit_usings_respect_explicit_language_version(self):
        """SDK 项目和显式 source-set 都不能在 C# 9 中偷偷启用 global using。"""
        self.write("A.cs", "public class A { public int M() { return Math.Abs(-1); } }")
        for source_set in (False, True):
            if source_set:
                (self.root / "App.csproj").unlink()
            for version, outcome in (("9.0", "error"), ("10.0", "ok")):
                with self.subTest(source_set=source_set, language_version=version):
                    if source_set:
                        config = self.config("main", ["A.cs"], language_version=version)
                        config["implicit_usings"] = True
                        self.source_set([config])
                    else:
                        self.write("App.csproj", f'<Project Sdk="Microsoft.NET.Sdk"><PropertyGroup><TargetFramework>net10.0</TargetFramework><LangVersion>{version}</LangVersion><ImplicitUsings>enable</ImplicitUsings></PropertyGroup></Project>')
                    result = self.run_helper(self.request(["A.cs"]))
                    self.assertEqual(result["outcome"], outcome, result)
                    if outcome == "error":
                        self.assertEqual(result["error"]["code"], "unsupported_configuration", result)
                        self.assertIn("ImplicitUsings", result["error"]["message"])
                        self.assertNotIn("graph", result)

    def test_explicit_language_versions_follow_roslyn_capabilities(self):
        """接受当前 Roslyn 支持的数字版本，拒绝未来语言和随工具版本变化的别名。"""
        self.write("A.cs", "class C {}")
        self.source_set([self.config("main", ["A.cs"], language_version="13.0")])
        accepted = self.run_helper(self.request(["A.cs"]))
        self.assertEqual(accepted["outcome"], "ok", accepted)
        for version in ("15.0", "latest", "latestMajor", "preview", "default", "014.0", "7.4"):
            with self.subTest(language_version=version):
                self.source_set([self.config("main", ["A.cs"], language_version=version)])
                result = self.run_helper(self.request(["A.cs"]))
                self.assertEqual(result["error"]["code"], "unsupported_configuration", result)

    def test_pathological_targets_fail_before_implicit_symbol_expansion(self):
        """超大 TFM 必须先经过数值、语言或目标引用校验，不能提前展开数十亿个常量。"""
        self.write("App/A.cs", "class C {}")
        for tfm, language, code in (
            ("net2147483647.0", "14.0", "unsupported_configuration"),
            ("net999999999999999999999999999999.0", "14.0", "unsupported_configuration"),
            ("net1000000000.0", None, "unsupported_configuration"),
            ("net1000000000.0", "14.0", "target_unavailable"),
        ):
            with self.subTest(tfm=tfm, language=language):
                override = f"<LangVersion>{language}</LangVersion>" if language else ""
                self.write("App/App.csproj", f'<Project Sdk="Microsoft.NET.Sdk"><PropertyGroup><TargetFramework>{tfm}</TargetFramework>{override}</PropertyGroup></Project>')
                result = self.run_helper(self.request(["App/A.cs"], platforms=[], timeout=1000))
                self.assertEqual(result["error"]["code"], code, result)

    def test_reference_platform_requires_exact_target_and_stable_patch(self):
        """错误 TFM、引用包、主次版本和不一致或预发行版本都不能借用有效的目标引用。"""
        self.write("A.cs", "class C {}")
        self.source_set([self.config("main", ["A.cs"])])
        mismatched = self.platform()
        mismatched["metadata"]["reference_pack_version"] = "10.0.1"
        wrong_tfm = self.platform()
        wrong_tfm["metadata"]["tfm"] = "net8.0"
        wrong_pack = self.platform()
        wrong_pack["metadata"]["reference_pack"] = "Microsoft.AspNetCore.App.Ref"
        invalid = [mismatched, wrong_tfm, wrong_pack, self.platform(root=str(self.scratch / "missing"))]
        invalid.extend(self.platform(version=version) for version in
                       ("8.0.31", "10.1.0", "10.0.1-preview.1", "10.0", "10.0.0.0", "010.0.0"))
        for platform in invalid:
            with self.subTest(platform=platform):
                result = self.run_helper(self.request(["A.cs"], platforms=[platform]))
                self.assertEqual(result["error"]["code"], "target_unavailable", result)
        duplicate = dict(self.platform(), id="second-copy")
        ambiguous = self.run_helper(self.request(["A.cs"], platforms=[self.platform(), duplicate]))
        self.assertEqual(ambiguous["error"]["code"], "target_unavailable", ambiguous)

    @unittest.skipUnless(REFS9, "set NARRANT_TEST_REFS9 to exercise a real nonzero .NET 9 patch fixture")
    def test_real_net9_patch_defaults_and_implicit_symbols(self):
        """真实 net9 补丁引用证明目标无白名单，并验证 C# 13 默认值与 SDK 常量。"""
        self.assertTrue(Path(REFS9).is_dir(), "configured .NET 9 reference fixture is required")
        self.assertTrue(REFS9_VERSION.startswith("9.0."))
        self.assertNotEqual(REFS9_VERSION, "9.0.0", "the fixture must prove nonzero patch support")
        self.write("App/App.csproj", '<Project Sdk="Microsoft.NET.Sdk"><PropertyGroup><TargetFramework>net9.0</TargetFramework></PropertyGroup></Project>')
        self.write("App/A.cs", r'''class C {
    public string Escape() => "\e";
    public void F() { var gate = new System.Threading.Lock(); }
}
#if NET && NETCOREAPP && NET9_0 && NET9_0_OR_GREATER && NET8_0_OR_GREATER && NET7_0_OR_GREATER && NET6_0_OR_GREATER && NET5_0_OR_GREATER && NETCOREAPP3_1_OR_GREATER && NETCOREAPP1_0_OR_GREATER
class CorrectTargetSymbols {}
#endif
#if NET10_0_OR_GREATER || NET8_0 || NETCOREAPP3_1 || NETSTANDARD2_1_OR_GREATER
class WrongTargetSymbols {}
#endif''')
        platform = self.platform("net9.0", REFS9_VERSION, REFS9)
        result = self.run_helper(self.request(["App/A.cs"], platforms=[platform]))
        self.assertEqual(result["outcome"], "ok", result)
        nodes = result["graph"]["nodes"]
        self.assertIn("CorrectTargetSymbols", [node["name"] for node in nodes])
        self.assertNotIn("WrongTargetSymbols", [node["name"] for node in nodes])
        method = next(node for node in nodes if node["name"] == "F")
        self.assertEqual([ref["target"]["kind"] for ref in method["references"] if ref["semantic"] == "code.calls"], ["external"])
        self.assertEqual(result["evidence"]["configurations"][0]["platform_ids"], ["net9.0"])
        self.write("App/App.csproj", '<Project Sdk="Microsoft.NET.Sdk"><PropertyGroup><TargetFramework>net9.0</TargetFramework><LangVersion>12.0</LangVersion></PropertyGroup></Project>')
        older_language = self.run_helper(self.request(["App/A.cs"], platforms=[platform]))
        self.assertEqual(older_language["error"]["code"], "object_extraction_failed", older_language)
        self.write("App/A.cs", "public static class E { extension(string s) { public int Size() => s.Length; } }")
        self.write("App/App.csproj", '<Project Sdk="Microsoft.NET.Sdk"><PropertyGroup><TargetFramework>net9.0</TargetFramework></PropertyGroup></Project>')
        default = self.run_helper(self.request(["App/A.cs"], platforms=[platform]))
        self.assertEqual(default["error"]["code"], "object_extraction_failed", default)
        self.write("App/App.csproj", '<Project Sdk="Microsoft.NET.Sdk"><PropertyGroup><TargetFramework>net9.0</TargetFramework><LangVersion>14.0</LangVersion></PropertyGroup></Project>')
        explicit = self.run_helper(self.request(["App/A.cs"], platforms=[platform]))
        self.assertEqual(explicit["outcome"], "ok", explicit)

    def test_malformed_syntax_fails_without_graph(self):
        """Roslyn 恢复语法树不能让畸形声明变成成功的部分图。"""
        self.write("A.cs", "class C { void F( { }")
        self.source_set([self.config("main", ["A.cs"])])
        result = self.run_helper(self.request(["A.cs"]))
        self.assertEqual(result["outcome"], "error")
        self.assertEqual(result["error"]["code"], "object_extraction_failed")
        self.assertNotIn("graph", result)
        self.assertNotIn("evidence", result)

    def test_source_generator_sentinel_is_rejected_without_execution(self):
        """真实能执行的源码生成器在静态项目边界被拒绝，不能写哨兵。"""
        sdk_roslyn = Path(DOTNET).parent / "sdk" / "10.0.100" / "Roslyn" / "bincore"
        compiler = sdk_roslyn / "csc.dll"
        self.assertTrue(compiler.is_file())
        marker = self.root / "App" / "generator-ran.marker"
        analyzer_marker = self.root / "App" / "analyzer-ran.marker"
        sentinel = self.root / "App" / "Sentinel.dll"
        sentinel.parent.mkdir(parents=True, exist_ok=True)
        generator_source = Path(self.temp.name) / "Sentinel.cs"
        generator_source.write_text('''using System.IO;
using System.Collections.Immutable;
using Microsoft.CodeAnalysis;
using Microsoft.CodeAnalysis.Diagnostics;
[Generator]
public sealed class SentinelGenerator : ISourceGenerator {
    public SentinelGenerator() { File.WriteAllText(''' + json.dumps(str(marker)) + ''', "constructed"); }
    public void Initialize(GeneratorInitializationContext context) {
        File.WriteAllText(''' + json.dumps(str(marker)) + ''', "initialized");
    }
    public void Execute(GeneratorExecutionContext context) {
        File.WriteAllText(''' + json.dumps(str(marker)) + ''', "executed");
    }
}
[DiagnosticAnalyzer(LanguageNames.CSharp)]
public sealed class SentinelAnalyzer : DiagnosticAnalyzer {
    private static readonly DiagnosticDescriptor Rule = new DiagnosticDescriptor(
        "SENT001", "Sentinel", "Sentinel", "Testing", DiagnosticSeverity.Warning, true);
    public override ImmutableArray<DiagnosticDescriptor> SupportedDiagnostics => ImmutableArray.Create(Rule);
    public override void Initialize(AnalysisContext context) {
        File.WriteAllText(''' + json.dumps(str(analyzer_marker)) + ''', "analyzer-initialized");
    }
}''', encoding="utf-8")
        references = [f"/reference:{path}" for path in sorted(Path(REFS).glob("*.dll"))]
        env = {**os.environ, "DOTNET_ROOT": str(Path(DOTNET).parent), "DOTNET_MULTILEVEL_LOOKUP": "0"}
        compile_generator = subprocess.run(
            [DOTNET, str(compiler), "/nologo", "/target:library", f"/out:{sentinel}",
             f"/reference:{sdk_roslyn / 'Microsoft.CodeAnalysis.dll'}", *references, str(generator_source)],
            capture_output=True, text=True, env=env, check=False, timeout=30)
        self.assertEqual(compile_generator.returncode, 0, compile_generator.stdout + compile_generator.stderr)
        self.assertTrue(sentinel.is_file())

        # 在仓库外单独运行同一生成器，确认它确实会执行并写入 marker。
        validation_source = Path(self.temp.name) / "Validation.cs"
        validation_source.write_text("class Validation {}", encoding="utf-8")
        validate_generator = subprocess.run(
            [DOTNET, str(compiler), "/nologo", "/target:library",
             f"/out:{Path(self.temp.name) / 'Validation.dll'}", f"/analyzer:{sentinel}",
             *references, str(validation_source)],
            capture_output=True, text=True, env=env, check=False, timeout=30)
        self.assertEqual(validate_generator.returncode, 0, validate_generator.stdout + validate_generator.stderr)
        self.assertTrue(marker.is_file(), "the real generator must have executed during control compilation")
        self.assertEqual(marker.read_text(encoding="utf-8"), "executed")
        self.assertTrue(analyzer_marker.is_file(), "the real diagnostic analyzer must have initialized")
        self.assertEqual(analyzer_marker.read_text(encoding="utf-8"), "analyzer-initialized")
        marker.unlink()
        analyzer_marker.unlink()

        self.write("App/App.csproj", '''<Project Sdk="Microsoft.NET.Sdk"><PropertyGroup>
<TargetFramework>net10.0</TargetFramework></PropertyGroup><ItemGroup>
<Analyzer Include="Sentinel.dll" /></ItemGroup></Project>''')
        self.write("App/Main.cs", "class MainClass {}")
        before = {path.relative_to(self.root).as_posix(): hashlib.sha256(path.read_bytes()).hexdigest()
                  for path in self.root.rglob("*") if path.is_file()}
        result = self.run_helper(self.request(["App/Main.cs"]))
        self.assertEqual(result["outcome"], "error")
        self.assertEqual(result["error"]["code"], "unsupported_configuration")
        self.assertNotIn("graph", result)
        self.assertFalse(marker.exists())
        self.assertFalse(analyzer_marker.exists())
        after = {path.relative_to(self.root).as_posix(): hashlib.sha256(path.read_bytes()).hexdigest()
                 for path in self.root.rglob("*") if path.is_file()}
        self.assertEqual(before, after)

    def test_test_only_source_set(self):
        """有可靠 test_only 声明时保留覆盖证据并按开关提取。"""
        self.write("Tests.cs", "class TestCase { void Run() {} }")
        test_config = self.config("tests", ["Tests.cs"])
        test_config["test_only"] = True
        self.source_set([test_config])
        excluded = self.run_helper(self.request(["Tests.cs"]))
        self.assertEqual(excluded["outcome"], "ok")
        self.assertEqual(excluded["evidence"]["files"][0]["disposition"], "excluded_test")
        self.assertEqual(excluded["graph"]["nodes"], [])
        included = self.run_helper(self.request(["Tests.cs"], include_tests=True))
        self.assertEqual(included["outcome"], "ok")
        self.assertIn("TestCase", [n["name"] for n in included["graph"]["nodes"]])

    def test_missing_binding_and_cancel(self):
        """缺第三方绑定只产生 Unresolved；显式取消返回 cancelled。"""
        self.write("A.cs", "class A { void F(){ MissingLibrary.Call(); } }")
        self.source_set([self.config("main", ["A.cs"])])
        result = self.run_helper(self.request(["A.cs"]))
        self.assertEqual(result["outcome"], "ok")
        f = next(n for n in result["graph"]["nodes"] if n["name"] == "F")
        self.assertTrue(any(r["target"]["kind"] == "unresolved" for r in f["references"]))
        cancel = json.dumps({"protocol": "narrant.native-analyzer/1", "type": "cancel", "id": "fixture"})
        cancelled = self.run_helper(self.request(["A.cs"]), extra_line=cancel)
        self.assertEqual(cancelled["outcome"], "cancelled")
        wrong_cancel = json.dumps({"protocol": "narrant.native-analyzer/1", "type": "cancel", "id": "other"})
        rejected = self.run_helper(self.request(["A.cs"]), extra_line=wrong_cancel)
        self.assertEqual(rejected["error"]["code"], "invalid_input")

    def test_body_and_format_projection(self):
        """方法体变化归自身；纯注释排版变化不更改规范内容。"""
        self.write("A.cs", "class A { int F() { return 1; } }")
        self.source_set([self.config("main", ["A.cs"])])
        first = self.run_helper(self.request(["A.cs"]))["graph"]["nodes"]
        self.write("A.cs", "class A { /* note */ int F( ) { return 1 ; } }")
        second = self.run_helper(self.request(["A.cs"]))["graph"]["nodes"]
        self.assertEqual([(n["id"], n["signature"], n["code"]) for n in first],
                         [(n["id"], n["signature"], n["code"]) for n in second])
        self.write("A.cs", "class A { int F() { return 2; } }")
        third = self.run_helper(self.request(["A.cs"]))["graph"]["nodes"]
        own = next(n for n in third if n["name"] == "F")
        self.assertNotEqual(own["code"], next(n for n in first if n["name"] == "F")["code"])
        self.assertEqual(next(n for n in third if n["name"] == "A")["code"],
                         next(n for n in first if n["name"] == "A")["code"])

    def test_sdk_defaults_hidden_files_and_true_usings(self):
        """固定 SDK 常量、true implicit usings 与隐藏目录过滤应和目标 SDK 一致。"""
        self.write("App/App.csproj", '<Project Sdk="Microsoft.NET.Sdk"><PropertyGroup><TargetFramework>net10.0</TargetFramework><ImplicitUsings>true</ImplicitUsings></PropertyGroup></Project>')
        self.write("App/Main.cs", """class MainClass { void Run() {
#if NETCOREAPP1_1_OR_GREATER
Console.WriteLine(1);
#else
Missing.Call();
#endif
} }""")
        self.write("App/.hidden/H.cs", "this is invalid C# !!!")
        result = self.run_helper(self.request(["App/Main.cs"]))
        self.assertEqual(result["outcome"], "ok", result.get("error"))
        run = next(n for n in result["graph"]["nodes"] if n["name"] == "Run")
        self.assertTrue(any(r["semantic"] == "code.calls" and r["target"]["kind"] == "external"
                            for r in run["references"]))
        self.assertFalse(any(r["target"].get("name") == "Missing.Call" for r in run["references"]))

    def test_literal_release_branch_is_analyzed(self):
        """普通项目的明确 Release 分支必须与 Debug 分别分析后并集。"""
        self.write("App/App.csproj", '''<Project Sdk="Microsoft.NET.Sdk"><PropertyGroup><TargetFramework>net10.0</TargetFramework><EnableDefaultCompileItems>false</EnableDefaultCompileItems></PropertyGroup>
<ItemGroup><Compile Include="Common.cs" /></ItemGroup>
<ItemGroup Condition="'$(Configuration)' == 'Debug'"><Compile Include="Debug.cs" /></ItemGroup>
<ItemGroup Condition="'$(Configuration)' == 'Release'"><Compile Include="Release.cs" /></ItemGroup></Project>''')
        for name in ("Common", "Debug", "Release"):
            self.write(f"App/{name}.cs", f"class {name} {{}}")
        result = self.run_helper(self.request(["App/Common.cs", "App/Debug.cs", "App/Release.cs"]))
        self.assertEqual(result["outcome"], "ok", result.get("error"))
        self.assertIn("Release", [n["name"] for n in result["graph"]["nodes"]])
        self.assertEqual(next(f for f in result["evidence"]["files"] if f["path"] == "App/Release.cs")["disposition"], "analyzed")

    def test_not_equal_debug_selects_release_branch(self):
        """配置不等于 Debug 的条件必须枚举有限域中的 Release。"""
        self.write("App/App.csproj", '''<Project Sdk="Microsoft.NET.Sdk"><PropertyGroup><TargetFramework>net10.0</TargetFramework><EnableDefaultCompileItems>false</EnableDefaultCompileItems></PropertyGroup>
<ItemGroup><Compile Include="Common.cs" /></ItemGroup>
<ItemGroup Condition="'$(Configuration)' != 'Debug'"><Compile Include="Release.cs" /></ItemGroup></Project>''')
        self.write("App/Common.cs", "class Common {}")
        self.write("App/Release.cs", "class ReleaseOnly {}")
        result = self.run_helper(self.request(["App/Common.cs", "App/Release.cs"]))
        self.assertEqual(result["outcome"], "ok", result.get("error"))
        self.assertIn("ReleaseOnly", [node["name"] for node in result["graph"]["nodes"]])
        self.assertEqual(next(file for file in result["evidence"]["files"] if file["path"] == "App/Release.cs")["disposition"], "analyzed")

    def test_release_project_reference_uses_release_dependency(self):
        """引用项目必须随调用方 Release 上下文重新静态编译，而非借用 Debug。"""
        self.write("Lib/Lib.csproj", '<Project Sdk="Microsoft.NET.Sdk"><PropertyGroup><TargetFramework>net10.0</TargetFramework></PropertyGroup></Project>')
        self.write("App/App.csproj", '''<Project Sdk="Microsoft.NET.Sdk"><PropertyGroup><TargetFramework>net10.0</TargetFramework><EnableDefaultCompileItems>false</EnableDefaultCompileItems></PropertyGroup>
<ItemGroup><Compile Include="Common.cs" /><ProjectReference Include="../Lib/Lib.csproj" /></ItemGroup>
<ItemGroup Condition="'$(Configuration)' == 'Release'"><Compile Include="Release.cs" /></ItemGroup></Project>''')
        self.write("Lib/Api.cs", "public class Api {\n#if DEBUG\npublic static void DebugOnly(){}\n#else\npublic static void ReleaseOnly(){}\n#endif\n}")
        self.write("App/Common.cs", "class Common {}")
        self.write("App/Release.cs", "class ReleaseCaller { void Run(){Api.ReleaseOnly();} }")
        result = self.run_helper(self.request(["Lib/Api.cs", "App/Common.cs", "App/Release.cs"]))
        self.assertEqual(result["outcome"], "ok", result.get("error"))
        run = next(node for node in result["graph"]["nodes"] if node["name"] == "Run")
        self.assertTrue(any(ref["semantic"] == "code.calls" and ref["target"]["kind"] == "node" and
                            "ReleaseOnly" in ref["target"]["id"] for ref in run["references"]))

    def test_nested_generic_parameter_overload_identity(self):
        """外层和内层同序号类型参数在重载槽位中仍须区分。"""
        self.write("A.cs", "class Outer<T> { class Inner<U> { void F(T x){} void F(U x){} } }")
        self.source_set([self.config("main", ["A.cs"])])
        result = self.run_helper(self.request(["A.cs"]))
        self.assertEqual(result["outcome"], "ok", result.get("error"))
        overloads = [node for node in result["graph"]["nodes"] if node["name"] == "F"]
        self.assertEqual(len(overloads), 2)
        self.assertEqual(len({node["id"] for node in overloads}), 2)

    def test_field_and_event_shared_headers(self):
        """共享 field/event 类型、访问性与属性须进入每个变量声明内容。"""
        self.write("A.cs", "class C { public int X; public event System.Action E; }")
        self.source_set([self.config("main", ["A.cs"])])
        first = self.run_helper(self.request(["A.cs"]))
        self.assertEqual(first["outcome"], "ok")
        x1 = next(n for n in first["graph"]["nodes"] if n["name"] == "X")
        e = next(n for n in first["graph"]["nodes"] if n["name"] == "E")
        self.assertIn("public", x1["signature"])
        self.assertIn("int", x1["signature"])
        self.assertIn("System.Action", e["signature"].replace(" ", ""))
        self.write("A.cs", "class C { private int X; public event System.Action E; }")
        second = self.run_helper(self.request(["A.cs"]))
        x2 = next(n for n in second["graph"]["nodes"] if n["name"] == "X")
        self.assertNotEqual(x1["signature"], x2["signature"])

    def test_token_projection_preserves_operator_and_raw_literal(self):
        """运算符分组及任意长度 raw 字符串中的空格必须保持语义差异。"""
        self.write("A.cs", 'class C { int F(int x,int y){return x++ + y;} string S()=> """"a""" b""""; }')
        self.source_set([self.config("main", ["A.cs"])])
        first = self.run_helper(self.request(["A.cs"]))
        self.assertEqual(first["outcome"], "ok", first.get("error"))
        self.write("A.cs", 'class C { int F(int x,int y){return x + ++y;} string S()=> """"a"""  b""""; }')
        second = self.run_helper(self.request(["A.cs"]))
        self.assertEqual(second["outcome"], "ok", second.get("error"))
        for name in ("F", "S"):
            self.assertNotEqual(next(n for n in first["graph"]["nodes"] if n["name"] == name)["code"],
                                next(n for n in second["graph"]["nodes"] if n["name"] == name)["code"])

    def test_branch_id_ignores_comment_directives(self):
        """注释和字符串里的伪指令不能改变后续声明身份。"""
        self.write("A.cs", "class C { void F(){} }")
        self.source_set([self.config("main", ["A.cs"])])
        first = self.run_helper(self.request(["A.cs"]))
        self.write("A.cs", 'class C { /*\n#if FAKE\n*/ string s = "#if STR"; void F(){} }')
        second = self.run_helper(self.request(["A.cs"]))
        f1 = next(n for n in first["graph"]["nodes"] if n["name"] == "F")
        f2 = next(n for n in second["graph"]["nodes"] if n["name"] == "F")
        self.assertEqual(f1["id"], f2["id"])

    def test_distinct_lexical_parts_and_generic_rename(self):
        """接口限定、同文件 partial/namespace 片段不碰撞；泛型改名不换 ID。"""
        self.write("A.cs", """interface I { void F(); } interface J { void F(); }
class C : I,J { void I.F(){} void J.F(){} }
partial class P { void A(){} } partial class P { void B(){} }
namespace N { class X{} } namespace N { class Y{} }
class G { void M<T>(T value){} }""")
        self.source_set([self.config("main", ["A.cs"])])
        first = self.run_helper(self.request(["A.cs"]))
        self.assertEqual(first["outcome"], "ok", first.get("error"))
        ids = [n["id"] for n in first["graph"]["nodes"]]
        self.assertEqual(len(ids), len(set(ids)))
        m1 = next(n for n in first["graph"]["nodes"] if n["name"] == "M")
        self.write("A.cs", self.root.joinpath("A.cs").read_text().replace("M<T>(T value)", "M<U>(U value)"))
        second = self.run_helper(self.request(["A.cs"]))
        m2 = next(n for n in second["graph"]["nodes"] if n["name"] == "M")
        self.assertEqual(m1["id"], m2["id"])

    def test_partial_member_selected_implementation(self):
        """未选定义与已选实现配对，调用必须指向最终图中的实现。"""
        self.write("A.cs", "partial class C { partial void F(); }")
        self.write("B.cs", "partial class C { partial void F(){} void G(){F();} }")
        self.source_set([self.config("main", ["A.cs", "B.cs"])])
        result = self.run_helper(self.request(["B.cs"]))
        self.assertEqual(result["outcome"], "ok", result.get("error"))
        g = next(n for n in result["graph"]["nodes"] if n["name"] == "G")
        self.assertTrue(any(r["semantic"] == "code.calls" and r["target"]["kind"] == "node" and
                            "implementation" in r["target"]["id"] for r in g["references"]))

    def test_external_method_identities_do_not_collapse(self):
        """同文件/同程序集的不同 owner 或重载须保留各自 External。"""
        self.write("A.cs", 'class C { void G(){X.F(1);Y.F("x");System.Math.Abs(1);System.Math.Abs(1.0);} }')
        self.write("B.cs", "class X { public static void F(int x){} } class Y { public static void F(string x){} }")
        self.source_set([self.config("main", ["A.cs", "B.cs"])])
        result = self.run_helper(self.request(["A.cs"]))
        self.assertEqual(result["outcome"], "ok", result.get("error"))
        g = next(n for n in result["graph"]["nodes"] if n["name"] == "G")
        symbols = [r["target"]["symbol"] for r in g["references"] if r["semantic"] == "code.calls" and r["target"]["kind"] == "external"]
        self.assertEqual(len(symbols), 4)
        self.assertEqual(len(set(symbols)), 4)

    def test_constructor_initializer_and_unknown_delegate(self):
        """构造初始化器指向确定构造函数，委托 Invoke 保持未知。"""
        self.write("A.cs", "class B { public B(int x){} } class C:B { public C():base(1){} void L(){} void M(){System.Action a=L;a();} int N(System.Func<int> fn){return fn();} }")
        self.source_set([self.config("main", ["A.cs"])])
        result = self.run_helper(self.request(["A.cs"]))
        self.assertEqual(result["outcome"], "ok", result.get("error"))
        ctor = next(n for n in result["graph"]["nodes"] if n["name"] == "C" and n["category"] == "callable")
        self.assertTrue(any(r["semantic"] == "code.calls" and r["target"]["kind"] == "node" for r in ctor["references"]))
        m = next(n for n in result["graph"]["nodes"] if n["name"] == "M")
        self.assertTrue(any(r["semantic"] == "code.calls" and r["target"]["kind"] == "unresolved" for r in m["references"]))
        n = next(n for n in result["graph"]["nodes"] if n["name"] == "N")
        self.assertTrue(any(r["semantic"] == "code.calls" and r["target"]["kind"] == "unresolved" for r in n["references"]))

    def test_property_event_and_expression_getter_relations(self):
        """属性、event、表达式 getter 的接口关系和 event 类型用途不可丢失。"""
        self.write("A.cs", """interface I { int P {get;} event System.Action E; }
class C:I { public int P => 1; public event System.Action E { add {} remove {} } }""")
        self.source_set([self.config("main", ["A.cs"])])
        result = self.run_helper(self.request(["A.cs"]))
        self.assertEqual(result["outcome"], "ok", result.get("error"))
        nodes = result["graph"]["nodes"]
        prop = next(n for n in nodes if n["name"] == "P" and "public" in n["signature"])
        evt = next(n for n in nodes if n["name"] == "E" and "public" in n["signature"])
        getter = next(n for n in nodes if n["name"] == "get" and n["id"].startswith(prop["id"] + "/"))
        self.assertTrue(any(r["semantic"] == "code.implements" for r in prop["references"]))
        self.assertTrue(any(r["semantic"] == "code.implements" for r in evt["references"]))
        self.assertTrue(any(r["semantic"] == "code.type_use" for r in evt["references"]))
        self.assertTrue(any(r["semantic"] == "code.implements" for r in getter["references"]))


    def test_interface_inheritance_and_class_implementation(self):
        """接口继承和类或结构体实现接口应具有不同语义及准确目标。"""
        self.write("A.cs", "interface IBase {} interface IDerived : IBase {} class C : IDerived {} struct S : IBase {}")
        self.source_set([self.config("main", ["A.cs"])])
        result = self.run_helper(self.request(["A.cs"]))
        self.assertEqual(result["outcome"], "ok", result)
        nodes = {n["name"]: n for n in result["graph"]["nodes"]}
        for source, semantic, target in [("IDerived", "code.extends", "IBase"),
                                         ("C", "code.implements", "IDerived"),
                                         ("S", "code.implements", "IBase")]:
            bases = [r for r in nodes[source]["references"] if r["semantic"] in ("code.extends", "code.implements")]
            self.assertEqual(bases, [{"semantic": semantic, "target": {"kind": "node", "id": nodes[target]["id"]}}])

if __name__ == "__main__":
    unittest.main()
