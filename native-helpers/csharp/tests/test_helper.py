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
HELPER = Path(__file__).resolve().parents[1] / "bin/Release/net10.0/Narrant.CSharp.Helper.dll"


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

    def config(self, name, files, defines=()):
        """生成 C# 14 / net10.0 的独立静态上下文。"""
        return {"id": name, "files": files, "tfm": "net10.0", "language_version": "14.0",
                "define_constants": list(defines), "implicit_usings": False}

    def request(self, files, platforms=None, timeout=120000, include_tests=False):
        """生成宿主侧 v1 请求，平台由案例显式提供。"""
        if platforms is None:
            platforms = [{"id": "net10", "kind": "dotnet-reference-assemblies", "version": "10.0.0",
                          "root": REFS, "fingerprint": "fixture-ref-10.0.0",
                          "metadata": {"tfm": "net10.0", "reference_pack": "Microsoft.NETCore.App.Ref",
                                       "reference_pack_version": "10.0.0"}}]
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


if __name__ == "__main__":
    unittest.main()
