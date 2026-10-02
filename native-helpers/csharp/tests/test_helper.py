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
        inactive = self.run_helper(self.request(["A/Main.cs", "A/Extra.cs"]))
        self.assertEqual(inactive["outcome"], "ok", inactive.get("error"))
        self.assertEqual(next(f for f in inactive["evidence"]["files"] if f["path"] == "A/Extra.cs")["disposition"], "inactive")
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


if __name__ == "__main__":
    unittest.main()
