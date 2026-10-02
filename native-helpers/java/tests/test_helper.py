"""Exercise the real JDT helper against disposable read-only project fixtures."""

import hashlib
import json
import os
import pathlib
import subprocess
import sys
import tempfile
import unittest

HELPER = pathlib.Path(__file__).resolve().parents[1]
REPO = HELPER.parents[1]
RUNTIME = pathlib.Path(os.environ.get("NARRANT_JAVA_RUNTIME", REPO / ".tools/java-native/jdk-25.0.4.1+1/Contents/Home"))
TARGET = pathlib.Path(os.environ.get("NARRANT_JAVA_TARGET", REPO / ".tools/java-native/jdk-17.0.20.1+1/Contents/Home"))
LIB = pathlib.Path(os.environ.get("NARRANT_JAVA_LIB", REPO / ".tools/java-native/lib"))
CP = subprocess.check_output([sys.executable, str(HELPER / "tools/classpath.py"), str(LIB)], text=True).strip()
COMMAND = [str(RUNTIME / "bin" / ("java.exe" if sys.platform == "win32" else "java")),
           "-cp", str(HELPER / "build/classes") + os.pathsep + CP,
           "dev.narrant.java.Main"]


def request(root, files, include_tests=False, target=TARGET, timeout=30000):
    """Create a closed v1 request with explicit exact target JDK metadata."""
    return {"protocol": "narrant.native-analyzer/1", "type": "analyze", "id": "fixture",
            "input": {"repository_root": str(root), "files": files, "include_tests": include_tests,
                      "profile": "java-static-v1"},
            "environment": {"scratch_root": str(root.parent / "scratch"),
                            "platforms": [{"id": "jdk17", "kind": "java-jdk", "version": "17.0.20.1+1",
                                           "root": str(target), "fingerprint": "fixture-jdk17",
                                           "metadata": {"release": "17"}}]},
            "limits": {"analysis_timeout_ms": timeout, "max_result_bytes": 134217728,
                       "max_diagnostic_bytes": 262144}}


def run(message, cancel=False):
    """Run one helper process and return its exact terminal frame."""
    payload = json.dumps(message, ensure_ascii=False) + "\n"
    if cancel:
        payload += json.dumps({"protocol": "narrant.native-analyzer/1", "type": "cancel", "id": "fixture"}) + "\n"
    environment = dict(os.environ)
    for key in ("CLASSPATH", "JAVA_TOOL_OPTIONS", "JDK_JAVA_OPTIONS", "_JAVA_OPTIONS"):
        environment.pop(key, None)
    result = subprocess.run(COMMAND, input=payload, text=True, capture_output=True, timeout=40,
                            env=environment)
    if result.returncode:
        raise AssertionError((result.returncode, result.stderr, result.stdout))
    rows = [json.loads(line) for line in result.stdout.splitlines()]
    assert len(rows) == 2 and rows[0]["type"] == "ready" and rows[1]["type"] == "completed", rows
    return rows[1]


def write(root, path, source):
    """Write one disposable source/config fixture."""
    target = root / path
    target.parent.mkdir(parents=True, exist_ok=True)
    target.write_text(source, encoding="utf-8")


def metadata(root, tests=False):
    """Declare an exact Java 17 standalone source-root model."""
    roots = [{"path": "src/main/java", "role": "main"}]
    if tests:
        roots.append({"path": "src/test/java", "role": "test"})
    write(root, "narrant-java.json", json.dumps({"schema": "java-static-v1", "release": "17",
                                                   "source_roots": roots}))


def node(graph, suffix):
    """Find one stable source identity by its ending."""
    matches = [entry for entry in graph["nodes"] if entry["id"].endswith(suffix)]
    assert len(matches) == 1, (suffix, matches)
    return matches[0]


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


if __name__ == "__main__":
    unittest.main()
