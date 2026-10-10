"""Exercise the real JDT helper against disposable read-only project fixtures."""

import hashlib
import json
import os
import pathlib
import re
import subprocess
import sys
import tempfile
import threading
import time
import unittest
from unittest import mock

HELPER = pathlib.Path(__file__).resolve().parents[1]
REPO = HELPER.parents[1]
RUNTIME = pathlib.Path(os.environ.get("NARRANT_JAVA_RUNTIME", REPO / ".tools/java-native/jdk-25.0.4.1+1/Contents/Home"))
TARGET = pathlib.Path(os.environ.get("NARRANT_JAVA_TARGET", REPO / ".tools/java-native/jdk-17.0.20.1+1/Contents/Home"))
LIB = pathlib.Path(os.environ.get("NARRANT_JAVA_LIB", REPO / ".tools/java-native/lib"))
CP = subprocess.check_output([sys.executable, str(HELPER / "tools/classpath.py"), str(LIB)], text=True).strip()
COMMAND = [str(RUNTIME / "bin" / ("java.exe" if sys.platform == "win32" else "java")),
           "-cp", str(HELPER / "build/classes") + os.pathsep + CP,
           "dev.narrant.java.Main"]
RUN_TIMEOUT = 40


def platform(target, release=None):
    """以 JDK 的 release 文件构造真实版本的平台声明。"""
    version = re.search(r'^JAVA_VERSION="([^"\r\n]+)"$', (target / "release").read_text(), re.MULTILINE).group(1)
    feature = version.split(".")[1] if version.startswith("1.") else re.split(r"[.+-]", version)[0]
    return {"id": "jdk" + (release or feature), "kind": "java-jdk", "version": version,
            "root": str(target), "metadata": {"release": release or feature}}


def request(root, files, include_tests=False, target=TARGET, timeout=30000, platforms=None):
    """创建版本封闭的协议请求，默认只提供当前测试的真实目标 JDK。"""
    return {"protocol": "narrant.native-analyzer/1", "type": "analyze", "id": "fixture",
            "input": {"repository_root": str(root), "files": files, "include_tests": include_tests,
                      "profile": "java-static-v1"},
            "environment": {"scratch_root": str(root.parent / "scratch"),
                            "platforms": platforms if platforms is not None else [platform(target)]},
            "limits": {"analysis_timeout_ms": timeout, "max_result_bytes": 134217728,
                       "max_diagnostic_bytes": 262144}}


def run(message, cancel=False):
    """在总会话期限内读取终结帧；超时杀死并回收进程，保留 stderr。"""
    payload = json.dumps(message, ensure_ascii=False) + "\n"
    if cancel:
        payload += json.dumps({"protocol": "narrant.native-analyzer/1", "type": "cancel", "id": "fixture"}) + "\n"
    environment = dict(os.environ)
    for key in ("CLASSPATH", "JAVA_TOOL_OPTIONS", "JDK_JAVA_OPTIONS", "_JAVA_OPTIONS"):
        environment.pop(key, None)
    # 真实 Host 在终结帧前保持输入管道打开；EOF 另设断连回归。
    with subprocess.Popen(COMMAND, stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                          stderr=subprocess.PIPE, text=True, env=environment) as proc:
        expired = threading.Event()
        output = []
        problem = None

        def expire():
            """统一计时覆盖写入、两次帧读取和退出等待，终止阻塞管道的生产者。"""
            expired.set()
            try:
                proc.kill()
            except ProcessLookupError:
                pass

        deadline = threading.Timer(RUN_TIMEOUT, expire)
        deadline.start()
        try:
            proc.stdin.write(payload)
            proc.stdin.flush()
            for _ in range(2):
                output.append(proc.stdout.readline())
            rows = [json.loads(line) for line in output]
            proc.stdin.close()
            proc.wait()
            extra = proc.stdout.read()
            output.append(extra)
            assert extra == ""
        except BaseException as error:
            problem = error
        finally:
            if proc.poll() is None:
                proc.kill()
            proc.wait()
            diagnostics = proc.stderr.read()
            deadline.cancel()
            deadline.join()
        if expired.is_set():
            raise subprocess.TimeoutExpired(COMMAND, RUN_TIMEOUT, output="".join(output),
                                            stderr=diagnostics)
        if problem is not None:
            raise problem
        if proc.returncode:
            raise AssertionError((proc.returncode, diagnostics, output))
    assert len(rows) == 2 and rows[0]["type"] == "ready" and rows[1]["type"] == "completed", rows
    return rows[1]


def write(root, path, source):
    """Write one disposable source/config fixture."""
    target = root / path
    target.parent.mkdir(parents=True, exist_ok=True)
    target.write_text(source, encoding="utf-8")


def metadata(root, tests=False, release="17"):
    """声明指定整数 release 与显式生产、测试源码根。"""
    roots = [{"path": "src/main/java", "role": "main"}]
    if tests:
        roots.append({"path": "src/test/java", "role": "test"})
    write(root, "narrant-java.json", json.dumps({"schema": "java-static-v1", "release": release,
                                                   "source_roots": roots}))


def node(graph, suffix):
    """Find one stable source identity by its ending."""
    matches = [entry for entry in graph["nodes"] if entry["id"].endswith(suffix)]
    assert len(matches) == 1, (suffix, matches)
    return matches[0]


def snapshot_files(root):
    """记录夹具仓库所有文件的相对路径与哈希，捕捉新增和改写。"""
    return {path.relative_to(root).as_posix(): hashlib.sha256(path.read_bytes()).hexdigest()
            for path in root.rglob("*") if path.is_file()}


def build_lombok_sentinel(scratch, repository, marker):
    """在仓外编译可由 ServiceLoader 实际发现的合成注解处理器。"""
    source = scratch / "processor-source"
    classes = scratch / "processor-classes"
    source.mkdir(parents=True)
    classes.mkdir(parents=True)
    write(source, "lombok/Getter.java", """package lombok;
import java.lang.annotation.*;
/** 合成的 Lombok 入口注解，仅供隔离测试使用。 */
@Retention(RetentionPolicy.SOURCE)
@Target({ElementType.TYPE, ElementType.FIELD})
public @interface Getter {}
""")
    write(source, "lombok/SentinelProcessor.java", """package lombok;
import java.nio.file.*;
import java.util.Set;
import javax.annotation.processing.*;
import javax.lang.model.SourceVersion;
import javax.lang.model.element.TypeElement;
/** 合成处理器；一旦被加载，立即写入仓外哨兵。 */
@SupportedAnnotationTypes("lombok.Getter")
@SupportedSourceVersion(SourceVersion.RELEASE_17)
public final class SentinelProcessor extends AbstractProcessor {
  static {
    try { Files.writeString(Path.of(""" + json.dumps(str(marker)) + """), "loaded"); }
    catch (Exception error) { throw new RuntimeException(error); }
  }
  /** 处理流程无需生成代码，加载哨兵足以证明执行。 */
  @Override public boolean process(Set<? extends TypeElement> types, RoundEnvironment round) { return false; }
}
""")
    java_suffix = ".exe" if sys.platform == "win32" else ""
    javac = RUNTIME / "bin" / ("javac" + java_suffix)
    jar_tool = RUNTIME / "bin" / ("jar" + java_suffix)
    subprocess.run([str(javac), "--release", "17", "-proc:none", "-d", str(classes),
                    str(source / "lombok/Getter.java"), str(source / "lombok/SentinelProcessor.java")],
                   check=True, capture_output=True, text=True, timeout=30)
    write(classes, "META-INF/services/javax.annotation.processing.Processor", "lombok.SentinelProcessor\n")
    archive = repository / "local-libs/lombok-sentinel.jar"
    archive.parent.mkdir(parents=True, exist_ok=True)
    subprocess.run([str(jar_tool), "--create", "--file", str(archive), "-C", str(classes), "."],
                   check=True, capture_output=True, text=True, timeout=30)
    # 正对照只处理仓外临时源码；验证 JAR/服务入口确实能触发哨兵。
    control = scratch / "control"
    write(control, "Control.java", "import lombok.Getter; @Getter class Control {}")
    subprocess.run([str(javac), "--release", "17", "-proc:only", "-classpath", str(archive),
                    "-processorpath", str(archive), "-d", str(control), str(control / "Control.java")],
                   check=True, capture_output=True, text=True, timeout=30)
    assert marker.read_text() == "loaded"
    marker.unlink()
    return archive


class JavaHelperTests(unittest.TestCase):
    """Verify structural completeness, semantic evidence and strict boundaries."""

    def setUp(self):
        """Create a disposable repository for every test."""
        self.temp = tempfile.TemporaryDirectory(prefix="narrant-java-test-")
        self.addCleanup(self.temp.cleanup)
        self.root = pathlib.Path(self.temp.name) / "repository"
        self.root.mkdir()

    def test_graph_overloads_external_and_unresolved(self):
        """Resolved overloads become nodes; target JDK and missing types remain distinct."""
        metadata(self.root)
        write(self.root, "src/main/java/p/Helper.java", "package p; class Helper { static int pick(int x){return x;} static int pick(String x){return x.length();} }")
        write(self.root, "src/main/java/p/Main.java", "package p; import java.util.List; class Main { int run(){ Helper.pick(1); Helper.pick(\"x\"); missing.Type.nope(); return List.of(1).size(); } }")
        files = ["src/main/java/p/Main.java", "src/main/java/p/Helper.java"]
        before = {path: hashlib.sha256((self.root / path).read_bytes()).hexdigest() for path in files}
        completed = run(request(self.root, files))
        self.assertEqual(completed["outcome"], "ok", completed)
        graph = completed["graph"]
        method = node(graph, "/method:run()")
        node_targets = {ref["target"]["id"] for ref in method["references"] if ref["target"]["kind"] == "node"}
        self.assertTrue(any(target.endswith("/method:pick(int)") for target in node_targets))
        self.assertTrue(any(target.endswith("/method:pick(String)") for target in node_targets))
        self.assertTrue(any(ref["target"] == {"kind": "unresolved", "name": "nope"} for ref in method["references"]))
        self.assertTrue(any(item["scope"] == "dependency" and "java.util.List" in item["symbol"]
                            for item in graph["external_symbols"]))
        self.assertTrue(all(hashlib.sha256((self.root / path).read_bytes()).hexdigest() == before[path] for path in files))
        self.assertEqual(len(completed["evidence"]["origins"]), len(graph["nodes"]))

    def test_malformed_syntax_fails_without_graph_or_evidence(self):
        """缺依赖可恢复，真正畸形语法必须完整失败，不能交付部分图。"""
        metadata(self.root)
        path = "src/main/java/p/Main.java"
        write(self.root, path, "package p; class Main { void run(){ missing.Type.nope(); } }")
        missing_dependency = run(request(self.root, [path]))
        self.assertEqual(missing_dependency["outcome"], "ok", missing_dependency)
        references = node(missing_dependency["graph"], "/method:run()")["references"]
        self.assertTrue(any(ref["target"] == {"kind": "unresolved", "name": "nope"}
                            for ref in references))
        write(self.root, path, "package p; class Main { void broken( { } }")
        before = snapshot_files(self.root)
        malformed = run(request(self.root, [path]))
        self.assertEqual(malformed["outcome"], "error", malformed)
        self.assertEqual(malformed["error"]["code"], "object_extraction_failed")
        self.assertNotIn("graph", malformed)
        self.assertNotIn("evidence", malformed)
        self.assertEqual(snapshot_files(self.root), before)

    def test_lombok_processor_candidate_is_not_loaded(self):
        """有效的 Lombok 候选处理器在静态分析中不得加载或改写仓库。"""
        metadata(self.root)
        scratch = pathlib.Path(self.temp.name) / "processor-work"
        marker = scratch / "processor-loaded.txt"
        archive = build_lombok_sentinel(scratch, self.root, marker)
        self.assertTrue(archive.is_file())
        self.assertFalse(marker.exists())
        write(self.root, "build.gradle", "plugins { id 'java' }\n"
              "dependencies { compileOnly 'org.projectlombok:lombok:1.18.42'; "
              "annotationProcessor files('local-libs/lombok-sentinel.jar') }\n")
        path = "src/main/java/p/Model.java"
        write(self.root, path, "package p; @lombok.Getter class Model { int value; }")
        before = snapshot_files(self.root)
        result = run(request(self.root, [path]))
        self.assertEqual(result["outcome"], "ok", result)
        self.assertTrue(any(entry["id"].endswith("/class:Model") for entry in result["graph"]["nodes"]))
        self.assertFalse(any("getValue" in entry["id"] for entry in result["graph"]["nodes"]))
        self.assertFalse(marker.exists(), "annotation processor class was loaded")
        self.assertEqual(snapshot_files(self.root), before)

    def test_maven_lombok_processor_configuration_rejected_without_execution(self):
        """声明 Maven Lombok 处理器的 POM 静态拒绝，不能调用构建或加载处理器。"""
        scratch = pathlib.Path(self.temp.name) / "processor-work"
        marker = scratch / "processor-loaded.txt"
        build_lombok_sentinel(scratch, self.root, marker)
        write(self.root, "pom.xml", """<project><modelVersion>4.0.0</modelVersion>
<groupId>p</groupId><artifactId>sample</artifactId><version>1</version>
<properties><maven.compiler.release>17</maven.compiler.release></properties>
<build><plugins><plugin><groupId>org.apache.maven.plugins</groupId>
<artifactId>maven-compiler-plugin</artifactId><configuration>
<annotationProcessorPaths><path><groupId>org.projectlombok</groupId>
<artifactId>lombok</artifactId><version>1.18.42</version></path>
</annotationProcessorPaths></configuration></plugin></plugins></build></project>""")
        path = "src/main/java/p/Model.java"
        write(self.root, path, "package p; @lombok.Getter class Model { int value; }")
        before = snapshot_files(self.root)
        result = run(request(self.root, [path]))
        self.assertEqual(result["outcome"], "error", result)
        self.assertEqual(result["error"]["code"], "unsupported_configuration")
        self.assertNotIn("graph", result)
        self.assertNotIn("evidence", result)
        self.assertFalse(marker.exists(), "annotation processor class was loaded")
        self.assertEqual(snapshot_files(self.root), before)

    def test_target_api_does_not_leak_from_runtime(self):
        """Java 21 List API cannot bind in a Java 17 target graph."""
        metadata(self.root)
        path = "src/main/java/p/Main.java"
        write(self.root, path, "package p; import java.util.List; class Main { Object run(){ return List.of(1).getFirst(); } }")
        completed = run(request(self.root, [path]))
        self.assertEqual(completed["outcome"], "ok", completed)
        method = node(completed["graph"], "/method:run()")
        self.assertTrue(any(ref["target"] == {"kind": "unresolved", "name": "getFirst"}
                            for ref in method["references"]))
        self.assertFalse(any("getFirst" in item["symbol"] for item in completed["graph"]["external_symbols"]))
        wrong = run(request(self.root, [path], target=RUNTIME))
        self.assertEqual(wrong["outcome"], "error")
        self.assertEqual(wrong["error"]["code"], "target_unavailable")

    def test_content_projection_and_identity(self):
        """Whitespace and child-body changes do not alter parent own content."""
        metadata(self.root)
        path = "src/main/java/p/Main.java"
        first = "package p; class Main { int a(){return 1;} int b(){return 2;} }"
        write(self.root, path, first)
        a = run(request(self.root, [path]))["graph"]
        write(self.root, path, "package p; class Main { int a() { return 9; } int b(){return 2;} }")
        b = run(request(self.root, [path]))["graph"]
        self.assertEqual(node(a, "/class:Main")["code"], node(b, "/class:Main")["code"])
        self.assertNotEqual(node(a, "/method:a()")["code"], node(b, "/method:a()")["code"])
        self.assertEqual(node(a, "/method:a()")["id"], node(b, "/method:a()")["id"])
        write(self.root, path, "package p; class Main { int a(){return 1;} int b(){return 2;} }")
        c = run(request(self.root, [path]))["graph"]
        self.assertEqual(node(a, "/method:a()")["code"], node(c, "/method:a()")["code"])

    def test_test_root_and_production_test_suffix(self):
        """Only an explicit test source root is excluded."""
        metadata(self.root, tests=True)
        main = "src/main/java/p/RealTest.java"
        test = "src/test/java/p/Checks.java"
        write(self.root, main, "package p; class RealTest { void run(){} }")
        write(self.root, test, "package p; class Checks { void check(){} }")
        excluded = run(request(self.root, [main, test]))
        self.assertEqual(excluded["outcome"], "ok", excluded)
        self.assertEqual({x["path"]: x["disposition"] for x in excluded["evidence"]["files"]},
                         {main: "analyzed", test: "excluded_test"})
        self.assertTrue(any("RealTest" in x["id"] for x in excluded["graph"]["nodes"]))
        self.assertFalse(any("Checks" in x["id"] for x in excluded["graph"]["nodes"]))
        included = run(request(self.root, [main, test], include_tests=True))
        self.assertTrue(any("Checks" in x["id"] for x in included["graph"]["nodes"]))

    def test_gradle_and_jpms_rejected_without_execution(self):
        """Unknown project logic and JPMS descriptors fail before a partial graph."""
        write(self.root, "build.gradle", "throw new RuntimeException('MUST_NOT_EXECUTE')")
        path = "src/main/java/p/Main.java"
        write(self.root, path, "package p; class Main {}")
        completed = run(request(self.root, [path]))
        self.assertEqual(completed["error"]["code"], "unsupported_configuration")
        metadata(self.root)
        module = "src/main/java/module-info.java"
        write(self.root, module, "module p { exports p; }")
        completed = run(request(self.root, [path]))
        self.assertEqual(completed["error"]["code"], "unsupported_configuration")
        self.assertFalse((self.root / "MUST_NOT_EXECUTE").exists())

    def test_maven_literal_model_and_cancel(self):
        """Literal POM requires no Maven process; cancel ends with one cancelled frame."""
        write(self.root, "pom.xml", "<project><modelVersion>4.0.0</modelVersion><groupId>p</groupId><artifactId>a</artifactId><version>1</version><properties><maven.compiler.release>17</maven.compiler.release></properties></project>")
        path = "src/main/java/p/Main.java"
        write(self.root, path, "package p; class Main { void run() {} }")
        self.assertEqual(run(request(self.root, [path]))["outcome"], "ok")
        self.assertEqual(run(request(self.root, [path]), cancel=True)["outcome"], "cancelled")

    def test_invalid_input_and_timeout(self):
        """Path escape and expired analysis return bounded errors."""
        metadata(self.root)
        path = "src/main/java/p/Main.java"
        write(self.root, path, "package p; class Main {}")
        invalid = run(request(self.root, ["../outside.java"]))
        self.assertEqual(invalid["error"]["code"], "invalid_input")
        timeout = run(request(self.root, [path], timeout=1))
        self.assertEqual(timeout["error"]["code"], "resource_limit")

    def test_source_object_kinds(self):
        """Explicit records, enum values, fields, blocks and local classes are real nodes."""
        metadata(self.root)
        path = "src/main/java/p/Shapes.java"
        write(self.root, path, """package p;
@interface Marker { int value() default 2; }
record Pair(int left, int right) { Pair { if (left < 0) throw new IllegalArgumentException(); } }
enum Mode { ON, OFF; }
class Shapes {
  int first = 1, second = 2;
  static { System.getProperty("x"); }
  void run() { class Local { void call() {} } Runnable r = new Runnable() { public void run() {} }; }
}
""")
        completed = run(request(self.root, [path]))
        self.assertEqual(completed["outcome"], "ok", completed)
        kinds = {entry["native_kind"] for entry in completed["graph"]["nodes"]}
        self.assertTrue({"annotation", "annotation-member", "record", "record-component", "enum",
                         "enum-constant", "field", "static-initializer", "anonymous"}.issubset(kinds), kinds)
        self.assertEqual(len([entry for entry in completed["graph"]["nodes"]
                              if entry["native_kind"] == "field"]), 2)

    def test_maven_modules_inherit_literal_release(self):
        """Repository-local Maven modules inherit a literal release without executing Maven."""
        write(self.root, "pom.xml", "<project><modelVersion>4.0.0</modelVersion><groupId>p</groupId><artifactId>root</artifactId><version>1</version><packaging>pom</packaging><properties><maven.compiler.release>17</maven.compiler.release></properties><modules><module>a</module><module>b</module></modules></project>")
        for name in ("a", "b"):
            write(self.root, f"{name}/pom.xml", f"<project><modelVersion>4.0.0</modelVersion><parent><groupId>p</groupId><artifactId>root</artifactId><version>1</version></parent><artifactId>{name}</artifactId></project>")
            write(self.root, f"{name}/src/main/java/p/{name.upper()}.java", f"package p; class {name.upper()} {{}}")
        paths = ["a/src/main/java/p/A.java", "b/src/main/java/p/B.java"]
        completed = run(request(self.root, paths))
        self.assertEqual(completed["outcome"], "ok", completed)
        self.assertEqual(len(completed["evidence"]["configurations"]), 2)

    def test_binding_keys_are_scoped_to_each_maven_module(self):
        """同名类在独立模块中不能借另一模块的 binding key 指向选中节点。"""
        write(self.root, "pom.xml", "<project><modelVersion>4.0.0</modelVersion><groupId>p</groupId><artifactId>root</artifactId><version>1</version><packaging>pom</packaging><properties><maven.compiler.release>17</maven.compiler.release></properties><modules><module>a</module><module>b</module></modules></project>")
        for module in ("a", "b"):
            write(self.root, f"{module}/pom.xml", f"<project><modelVersion>4.0.0</modelVersion><parent><groupId>p</groupId><artifactId>root</artifactId><version>1</version></parent><artifactId>{module}</artifactId></project>")
            write(self.root, f"{module}/src/main/java/p/Foo.java", "package p; class Foo { static void ping(){} }")
        caller = "b/src/main/java/p/Caller.java"
        write(self.root, caller, "package p; class Caller { void run(){ Foo.ping(); } }")
        result = run(request(self.root, ["a/src/main/java/p/Foo.java", caller]))
        self.assertEqual(result["outcome"], "ok", result)
        call = [ref for ref in node(result["graph"], "/method:run()")["references"]
                if ref["semantic"] == "code.calls"]
        self.assertEqual(len(call), 1)
        self.assertEqual(call[0]["target"]["kind"], "external")
        self.assertIn("b/src/main/java/p/Foo.java", call[0]["target"]["symbol"])
        self.assertEqual(result["graph"]["external_symbols"][0]["scope"], "repository")

    def test_duplicate_declarations_fail_extraction(self):
        """完全相同的方法和跨源码根重复类型都不能被合并。"""
        metadata(self.root)
        path = "src/main/java/p/Foo.java"
        write(self.root, path, "package p; class Foo { void f(){} void f(){} }")
        duplicate_method = run(request(self.root, [path]))
        self.assertEqual(duplicate_method["error"]["code"], "object_extraction_failed")
        (self.root / "narrant-java.json").write_text(json.dumps({"schema": "java-static-v1", "release": "17",
            "source_roots": [{"path": "src/main/java", "role": "main"}, {"path": "second", "role": "main"}]}))
        write(self.root, path, "package p; class Foo {}")
        write(self.root, "second/p/Foo.java", "package p; class Foo {}")
        duplicate_type = run(request(self.root, [path]))
        self.assertEqual(duplicate_type["error"]["code"], "object_extraction_failed")

    def test_actual_maven_parent_overrides_aggregator(self):
        """子模块真正的仓内 parent 必须求值，不能继承无关聚合 POM 的 release。"""
        write(self.root, "pom.xml", "<project><modelVersion>4.0.0</modelVersion><groupId>p</groupId><artifactId>root</artifactId><version>1</version><packaging>pom</packaging><properties><maven.compiler.release>17</maven.compiler.release></properties><modules><module>child</module></modules></project>")
        write(self.root, "parents/parent/pom.xml", "<project><modelVersion>4.0.0</modelVersion><groupId>p</groupId><artifactId>other</artifactId><version>1</version><properties><maven.compiler.release>21</maven.compiler.release></properties></project>")
        write(self.root, "child/pom.xml", "<project><modelVersion>4.0.0</modelVersion><parent><groupId>p</groupId><artifactId>other</artifactId><version>1</version><relativePath>../parents/parent/pom.xml</relativePath></parent><artifactId>child</artifactId></project>")
        path = "child/src/main/java/p/Child.java"
        write(self.root, path, "package p; class Child {}")
        result = run(request(self.root, [path]))
        self.assertEqual(result["error"]["code"], "target_unavailable")

    def test_same_source_configuration_union(self):
        """同一声明汇合多个隔离配置的确定与未知关系。"""
        configs = [
            {"id": "a", "release": "17", "source_roots": [{"path": "common", "role": "main"}, {"path": "a", "role": "main"}]},
            {"id": "b", "release": "17", "source_roots": [{"path": "common", "role": "main"}, {"path": "b", "role": "main"}]},
            {"id": "unknown", "release": "17", "source_roots": [{"path": "common", "role": "main"}]},
        ]
        write(self.root, "narrant-java.json", json.dumps({"schema": "java-static-v1", "configurations": configs}))
        main = "common/p/Main.java"
        write(self.root, main, "package p; class Main { void run(){ Helper.ping(); } }")
        for folder in ("a", "b"):
            write(self.root, f"{folder}/p/Helper.java", "package p; class Helper { static void ping(){} }")
        result = run(request(self.root, [main]))
        self.assertEqual(result["outcome"], "ok", result)
        calls = [ref["target"] for ref in node(result["graph"], "/method:run()")["references"]
                 if ref["semantic"] == "code.calls"]
        self.assertEqual({target["kind"] for target in calls}, {"external", "unresolved"})
        self.assertEqual(len([target for target in calls if target["kind"] == "external"]), 2)
        self.assertEqual(len(result["evidence"]["files"][0]["configuration_ids"]), 3)

    def test_external_metadata_symlink_rejected(self):
        """仓外 JSON 不得借仓内链接成为配置来源。"""
        outside = pathlib.Path(self.temp.name) / "outside.json"
        outside.write_text(json.dumps({"schema": "java-static-v1", "release": "17",
                                       "source_roots": [{"path": "src/main/java", "role": "main"}]}))
        (self.root / "narrant-java.json").symlink_to(outside)
        path = "src/main/java/p/Foo.java"
        write(self.root, path, "package p; class Foo {}")
        result = run(request(self.root, [path]))
        self.assertEqual(result["error"]["code"], "unsupported_configuration")

    def test_selected_implicit_constructor_is_repository_external(self):
        """隐式构造器不造源码节点，但其选中类型仍证明仓内来源。"""
        metadata(self.root)
        path = "src/main/java/p/Foo.java"
        write(self.root, path, "package p; class Foo {} class Caller { void run(){ new Foo(); } }")
        result = run(request(self.root, [path]))
        self.assertEqual(result["outcome"], "ok", result)
        constructor = [item for item in result["graph"]["external_symbols"] if "<init>" in item["symbol"]]
        self.assertEqual(len(constructor), 1)
        self.assertEqual(constructor[0]["scope"], "repository")
        self.assertFalse(any(entry["native_kind"] == "constructor" for entry in result["graph"]["nodes"]))

    def test_real_error_wins_racing_cancel(self):
        """配置错误即使和有效 cancel 同时到达，也保持 error。"""
        path = "src/main/java/p/Foo.java"
        write(self.root, path, "package p; class Foo {}")
        write(self.root, "build.gradle", "throw new Error('never run')")
        for _ in range(5):
            result = run(request(self.root, [path]), cancel=True)
            self.assertEqual(result["outcome"], "error", result)
            self.assertEqual(result["error"]["code"], "unsupported_configuration")

    def test_annotation_brace_does_not_truncate_signature(self):
        """字符串中的左花括号不能截断类型或方法声明头。"""
        metadata(self.root)
        path = "src/main/java/p/Foo.java"
        write(self.root, path, 'package p; @interface Note { String value(); } @Note("{") class Foo { @Note("{") void run(){} }')
        result = run(request(self.root, [path]))
        self.assertEqual(result["outcome"], "ok", result)
        self.assertIn("class Foo", node(result["graph"], "/class:Foo")["signature"])
        self.assertIn('"{"', node(result["graph"], "/method:run()")["signature"])


    def test_host_eof_cancels_real_entrypoint(self):
        """请求后提前 EOF 必须停止真实 Java 分析，不能发送普通成功。"""
        metadata(self.root)
        path = "src/main/java/A.java"
        write(self.root, path, "class A { void run(){} }")
        result = subprocess.run(COMMAND, input=json.dumps(request(self.root, [path])) + "\n",
                                text=True, capture_output=True, timeout=10)
        self.assertEqual(result.returncode, 0, result.stderr)
        terminal = json.loads(result.stdout.splitlines()[-1])
        self.assertEqual(terminal["outcome"], "cancelled", terminal)
        self.assertNotIn("graph", terminal)

    def test_inherited_maven_source_roots(self):
        """父 POM 的主测试目录在子模块继承，默认目录不再属于该模块。"""
        write(self.root, "pom.xml", "<project><groupId>p</groupId><artifactId>parent</artifactId><version>1</version><modules><module>child</module></modules><properties><maven.compiler.release>17</maven.compiler.release></properties><build><sourceDirectory>sources</sourceDirectory><testSourceDirectory>checks</testSourceDirectory></build></project>")
        write(self.root, "child/pom.xml", "<project><parent><groupId>p</groupId><artifactId>parent</artifactId><version>1</version></parent><artifactId>child</artifactId></project>")
        main, test, default = "child/sources/B.java", "child/checks/Check.java", "child/src/main/java/A.java"
        for path, name in [(main, "B"), (test, "Check"), (default, "A")]:
            write(self.root, path, "class " + name + " {}")
        rejected = run(request(self.root, [default]))
        self.assertEqual(rejected["outcome"], "error", rejected)
        excluded = run(request(self.root, [main, test]))
        self.assertEqual(excluded["outcome"], "ok", excluded)
        self.assertEqual({x["path"]: x["disposition"] for x in excluded["evidence"]["files"]},
                         {main: "analyzed", test: "excluded_test"})
        included = run(request(self.root, [main, test], include_tests=True))
        self.assertEqual(included["outcome"], "ok", included)
        node(included["graph"], "/class:Check")

    def test_generic_parameter_rename_identity(self):
        """词法类型参数按作用域及位置归一，重命名和遮蔽不改变方法身份。"""
        metadata(self.root)
        path = "src/main/java/C.java"
        graphs = []
        for outer, inner, method in [("T", "U", "V"), ("A", "B", "D")]:
            write(self.root, path, f"class C<{outer}> {{ void f({outer} x){{}} class N<{inner}> {{ void g({outer} x,{inner} y){{}} <{method}> void h({method} x,{inner} y,{outer} z){{}} <{outer}> void shadow({outer} x){{}} }} }}")
            completed = run(request(self.root, [path]))
            self.assertEqual(completed["outcome"], "ok", completed)
            graphs.append(completed["graph"])
        identities = [{n["name"]: n["id"] for n in g["nodes"] if n["native_kind"] == "method"} for g in graphs]
        self.assertEqual(identities[0], identities[1])
        self.assertIn("@method0:0", identities[0]["shadow"])
        self.assertIn("@type0:0", identities[0]["f"])
        self.assertIn("@type1:0,@type0:0", identities[0]["g"])
        self.assertNotEqual(identities[0]["g"].split("(", 1)[1].split(",")[0],
                            identities[0]["g"].split("(", 1)[1].split(",")[1].rstrip(")"))

    def test_record_component_owns_signature_content(self):
        """组件类型编辑只改变组件，父 record 签名和代码均保留组件锚点。"""
        metadata(self.root)
        path = "src/main/java/R.java"
        graphs = []
        for type_name in ["int", "long"]:
            write(self.root, path, f"record R({type_name} value, String other) {{}}")
            result = run(request(self.root, [path]))
            self.assertEqual(result["outcome"], "ok", result)
            graphs.append(result["graph"])
        for field in ["id", "signature", "code"]:
            self.assertEqual(node(graphs[0], "/record:R")[field], node(graphs[1], "/record:R")[field])
        self.assertNotEqual(node(graphs[0], "/record-component:value")["signature"],
                            node(graphs[1], "/record-component:value")["signature"])

class DriverDeadlineTests(unittest.TestCase):
    """用短暂阻塞的假进程验证测试驱动自身的超时与回收。"""

    def test_stalled_ready_and_completed_are_bounded_and_reaped(self):
        """ready 与 completed 都受会话总期限约束，保留诊断且不遗留子进程。"""
        for phase in ["ready", "completed"]:
            with self.subTest(phase=phase):
                command = [sys.executable, "-u", "-c",
                           "import json,sys,threading; "
                           f"sys.stderr.write('stall-{phase}\\n'); sys.stderr.flush(); "
                           + ("print(json.dumps({'type':'ready'}), flush=True); " if phase == "completed" else "")
                           + "threading.Event().wait()"]
                children = []
                popen = subprocess.Popen

                def spawn(*args, **kwargs):
                    """保留真实子进程句柄，用于独立断言回收及故障兜底。"""
                    child = popen(*args, **kwargs)
                    children.append(child)
                    return child

                def rescue():
                    """旧驱动若丢失期限，测试仍在两秒后停止假进程而非挂死 CI。"""
                    for child in children:
                        if child.poll() is None:
                            child.kill()

                guard = threading.Timer(2, rescue)
                guard.start()
                started = time.monotonic()
                try:
                    with mock.patch.dict(run.__globals__, COMMAND=command, RUN_TIMEOUT=0.5), \
                         mock.patch.object(subprocess, "Popen", side_effect=spawn):
                        with self.assertRaises(subprocess.TimeoutExpired) as raised:
                            run({"test": "deadline"})
                    self.assertLess(time.monotonic() - started, 1.5)
                    self.assertIn(f"stall-{phase}", raised.exception.stderr)
                    self.assertEqual(len(children), 1)
                    self.assertIsNotNone(children[0].returncode)
                    self.assertEqual(children[0].wait(timeout=0), children[0].returncode)
                    self.assertTrue(children[0].stdout.closed)
                    self.assertTrue(children[0].stderr.closed)
                finally:
                    guard.cancel()
                    guard.join()
                    rescue()
                    for child in children:
                        child.wait(timeout=2)


if __name__ == "__main__":
    unittest.main()
