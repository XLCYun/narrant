"""以真实 JDK 标准库验证 release、配置隔离与不执行项目构建的目标选择。"""

import json
import os
import pathlib
import shutil
import subprocess
import sys
import tempfile
import unittest
import zipfile

from test_helper import REPO, RUNTIME, TARGET, metadata, node, platform, request, run, snapshot_files, write


def classic_target():
    """优先使用显式 JDK8 夹具，否则只在仓库工具目录寻找标准目录布局。"""
    configured = os.environ.get("NARRANT_JAVA_TARGET_8")
    if configured:
        return pathlib.Path(configured)
    tools = REPO / ".tools/java-native"
    for candidate in sorted([*tools.glob("jdk8*"), *tools.glob("jdk-8*")]):
        for home in (candidate / "Contents/Home", candidate):
            if (home / "jre/lib/rt.jar").is_file():
                return home
    return tools / "jdk8/Contents/Home"


TARGET8 = classic_target()
TARGET25 = pathlib.Path(os.environ.get("NARRANT_JAVA_TARGET_25", str(RUNTIME)))


def source_method(graph, path):
    """定位指定文件的 run 方法，避免跨配置同名类型造成测试误选。"""
    return node(graph, path + "#file/class:Main/method:run()")


class TargetVersionTests(unittest.TestCase):
    """分别验证真实 Java 8、17、25 系统库和配置级语义环境。"""

    def setUp(self):
        """为每次分析建立临时项目，标准 JDK 始终只读。"""
        self.temp = tempfile.TemporaryDirectory(prefix="narrant-java-target-")
        self.addCleanup(self.temp.cleanup)
        self.root = pathlib.Path(self.temp.name) / "repository"
        self.root.mkdir()
        self.path = "src/main/java/p/Main.java"

    def assert_complete_failure(self, result, code):
        """错误不得夹带部分图或来源证据。"""
        self.assertEqual(result["outcome"], "error", result)
        self.assertEqual(result["error"]["code"], code, result)
        self.assertNotIn("graph", result)
        self.assertNotIn("evidence", result)

    def test_modern_targets_keep_their_own_api(self):
        """相同源码在 17 中保留未知 getFirst，在 25 中绑定真实 JDK API。"""
        write(self.root, self.path,
              "package p; import java.util.List; class Main { Object run(){ return List.of(1).getFirst(); } }")
        for release, target in (("17", TARGET), ("25", TARGET25)):
            with self.subTest(release=release):
                metadata(self.root, release=release)
                before = snapshot_files(self.root)
                result = run(request(self.root, [self.path], target=target))
                self.assertEqual(result["outcome"], "ok", result)
                references = source_method(result["graph"], self.path)["references"]
                unresolved = any(ref["target"] == {"kind": "unresolved", "name": "getFirst"}
                                 for ref in references)
                resolved = any("#getFirst(" in item["symbol"] for item in result["graph"]["external_symbols"])
                self.assertEqual(unresolved, release == "17")
                self.assertEqual(resolved, release == "25")
                self.assertEqual(result["evidence"]["configurations"][0]["platform_ids"], ["jdk" + release])
                self.assertEqual(snapshot_files(self.root), before)

    def test_language_level_and_preview_are_enforced(self):
        """模式 switch 在 17 仍需预览，25 中才按稳定语法接受。"""
        write(self.root, self.path, """package p; class Main {
            int run(Object value) { return switch (value) { case String text -> text.length(); default -> 0; }; }
        }""")
        metadata(self.root, release="17")
        self.assert_complete_failure(run(request(self.root, [self.path])), "object_extraction_failed")
        metadata(self.root, release="25")
        result = run(request(self.root, [self.path], target=TARGET25))
        self.assertEqual(result["outcome"], "ok", result)
        self.assertTrue(any("#length(" in item["symbol"] for item in result["graph"]["external_symbols"]))

    def test_mixed_metadata_targets_have_independent_bindings(self):
        """两个同名模块分别获得自己的平台来源与 API 绑定。"""
        configs = []
        files = []
        for release in ("17", "25"):
            root = "java" + release
            configs.append({"id": root, "release": release, "source_roots": [{"path": root, "role": "main"}]})
            path = root + "/p/Main.java"
            files.append(path)
            write(self.root, path,
                  "package p; class Main { Object run(){ return java.util.List.of(1).getFirst(); } }")
        write(self.root, "narrant-java.json", json.dumps({"schema": "java-static-v1", "configurations": configs}))
        result = run(request(self.root, files, platforms=[platform(TARGET), platform(TARGET25)]))
        self.assertEqual(result["outcome"], "ok", result)
        old_calls = [ref["target"] for ref in source_method(result["graph"], files[0])["references"]]
        new_calls = [ref["target"] for ref in source_method(result["graph"], files[1])["references"]]
        self.assertIn({"kind": "unresolved", "name": "getFirst"}, old_calls)
        self.assertNotIn({"kind": "unresolved", "name": "getFirst"}, new_calls)
        self.assertTrue(any(call["kind"] == "external" and "#getFirst(" in call["symbol"] for call in new_calls))
        self.assertEqual({item["id"]: item["platform_ids"] for item in result["evidence"]["configurations"]},
                         {"java-config:metadata:java17": ["jdk17"], "java-config:metadata:java25": ["jdk25"]})

    def test_shared_source_keeps_cross_release_union(self):
        """同一物理声明保留旧目标的未知关系与新目标的确定关系，配置顺序不影响图。"""
        write(self.root, self.path,
              "package p; class Main { Object run(){ return java.util.List.of(1).getFirst(); } }")
        targets = ([TARGET8] if (TARGET8 / "jre/lib/rt.jar").is_file() else []) + [TARGET, TARGET25]
        platforms = [platform(target) for target in targets]
        configs = [{"id": "java" + release, "release": release,
                    "source_roots": [{"path": "src/main/java", "role": "main"}]}
                   for release in (item["metadata"]["release"] for item in platforms)]
        graphs = []
        for ordered in (configs, list(reversed(configs))):
            write(self.root, "narrant-java.json", json.dumps({"schema": "java-static-v1", "configurations": ordered}))
            result = run(request(self.root, [self.path], platforms=platforms))
            self.assertEqual(result["outcome"], "ok", result)
            graphs.append(result["graph"])
            calls = [ref["target"] for ref in source_method(result["graph"], self.path)["references"]]
            self.assertIn({"kind": "unresolved", "name": "getFirst"}, calls)
            self.assertTrue(any(call["kind"] == "external" and "#getFirst(" in call["symbol"] for call in calls))
            self.assertEqual(len(result["evidence"]["files"][0]["configuration_ids"]), len(configs))
            if len(configs) == 3:
                self.assertIn({"kind": "unresolved", "name": "of"}, calls)
                self.assertTrue(any(call["kind"] == "external" and "#of(" in call["symbol"] for call in calls))
            self.assertEqual(len(result["evidence"]["origins"]), len(result["graph"]["nodes"]))
        self.assertEqual(graphs[0], graphs[1])

    def test_maven_modules_can_declare_distinct_releases(self):
        """聚合 POM 的两个子模块通过字面 release 选择不同目标，无需 Maven。"""
        write(self.root, "pom.xml", """<project><groupId>p</groupId><artifactId>root</artifactId>
          <version>1</version><packaging>pom</packaging><modules><module>a</module><module>b</module></modules></project>""")
        files = []
        for module, release in (("a", "17"), ("b", "25")):
            write(self.root, module + "/pom.xml", "<project><groupId>p</groupId><artifactId>" + module +
                  "</artifactId><version>1</version><properties><maven.compiler.release>" + release +
                  "</maven.compiler.release></properties></project>")
            path = module + "/src/main/java/p/Main.java"
            files.append(path)
            write(self.root, path, "package p; record Main(String value) {}")
        before = snapshot_files(self.root)
        result = run(request(self.root, files, platforms=[platform(TARGET), platform(TARGET25)]))
        self.assertEqual(result["outcome"], "ok", result)
        self.assertEqual({item["id"]: item["platform_ids"] for item in result["evidence"]["configurations"]},
                         {"java-config:a": ["jdk17"], "java-config:b": ["jdk25"]})
        self.assertEqual(snapshot_files(self.root), before)

    def test_missing_wrong_and_ambiguous_target_fail(self):
        """没有相同 major、伪造真实版本、错配 major 或重复目标时均不得回退运行时。"""
        metadata(self.root)
        write(self.root, self.path, "package p; class Main {}")
        wrong_major = platform(TARGET25, "17")
        wrong_version = dict(platform(TARGET), version="17.0.0")
        duplicate = dict(platform(TARGET), id="second-jdk17")
        for platforms, code in (([], "target_unavailable"), ([platform(TARGET25)], "target_unavailable"),
                                ([wrong_major], "target_unavailable"), ([wrong_version], "target_unavailable"),
                                ([platform(TARGET), duplicate], "invalid_input")):
            with self.subTest(platforms=platforms):
                self.assert_complete_failure(run(request(self.root, [self.path], platforms=platforms)), code)

    def test_invalid_or_compiler_unsupported_releases_fail(self):
        """规范整数之外的 release 与超出锁定编译器能力的 release 在选 JDK 前失败。"""
        write(self.root, self.path, "package p; class Main {}")
        for release in ("0", "08", "1.8", "17-preview", "17-ea", " 17", "999", "${java.version}"):
            with self.subTest(release=release):
                metadata(self.root, release=release)
                self.assert_complete_failure(run(request(self.root, [self.path], platforms=[])), "unsupported_configuration")

    @unittest.skipUnless((TARGET8 / "jre/lib/rt.jar").is_file(), "需要 NARRANT_JAVA_TARGET_8 指向真实 JDK8")
    def test_classic_target_loads_standard_jars_without_new_api(self):
        """8 的 rt/jsse/jce 标准库可绑定，但 9 才新增的 List.of 必须保持未知。"""
        metadata(self.root, release="8")
        write(self.root, self.path, """package p; class Main {
          Object run() throws Exception {
            java.util.function.Supplier<String> text = () -> "ok";
            javax.net.ssl.SSLContext.getDefault();
            javax.crypto.Cipher.getInstance("AES");
            java.util.List.of(text.get());
            return java.util.Collections.singletonList(text.get());
          }
        }""")
        before = snapshot_files(self.root)
        result = run(request(self.root, [self.path], target=TARGET8))
        self.assertEqual(result["outcome"], "ok", result)
        calls = [ref["target"] for ref in source_method(result["graph"], self.path)["references"]]
        self.assertIn({"kind": "unresolved", "name": "of"}, calls)
        symbols = [item["symbol"] for item in result["graph"]["external_symbols"]]
        for expected in ("#getDefault(", "#getInstance(", "#singletonList("):
            self.assertTrue(any(expected in symbol for symbol in symbols), (expected, symbols))
        self.assertEqual(snapshot_files(self.root), before)

    @unittest.skipUnless((TARGET8 / "jre/lib/rt.jar").is_file(), "需要 NARRANT_JAVA_TARGET_8 指向真实 JDK8")
    def test_classic_target_rejects_newer_syntax(self):
        """record 不得借固定运行时或最新 AST 层级进入 Java 8 图。"""
        metadata(self.root, release="8")
        write(self.root, self.path, "package p; record Main(String value) {}")
        self.assert_complete_failure(run(request(self.root, [self.path], target=TARGET8)), "object_extraction_failed")
        metadata(self.root, release="17")
        self.assertEqual(run(request(self.root, [self.path]))["outcome"], "ok")

    @unittest.skipUnless((TARGET8 / "jre/lib/rt.jar").is_file(), "需要 NARRANT_JAVA_TARGET_8 指向真实 JDK8")
    def test_classic_target_ignores_extension_and_unlisted_jars(self):
        """标准 JAR 以外的扩展类路径不能变成已解析的项目依赖。"""
        scratch = pathlib.Path(self.temp.name)
        target = scratch / "jdk8"
        (target / "jre/lib/ext").mkdir(parents=True)
        shutil.copyfile(TARGET8 / "release", target / "release")
        for name in ("resources.jar", "rt.jar", "jsse.jar", "jce.jar", "charsets.jar"):
            if (TARGET8 / "jre/lib" / name).is_file():
                shutil.copyfile(TARGET8 / "jre/lib" / name, target / "jre/lib" / name)
        source = scratch / "library-source"
        classes = scratch / "library-classes"
        classes.mkdir()
        write(source, "unlisted/Extra.java", "package unlisted; public class Extra { public static int ping(){ return 1; } }")
        javac = RUNTIME / "bin" / ("javac.exe" if sys.platform == "win32" else "javac")
        subprocess.run([str(javac), "--release", "8", "-proc:none", "-d", str(classes),
                        str(source / "unlisted/Extra.java")], check=True, capture_output=True, text=True, timeout=30)
        for location in (target / "jre/lib/ext/extra.jar", target / "jre/lib/extra.jar"):
            with zipfile.ZipFile(location, "w") as jar:
                jar.write(classes / "unlisted/Extra.class", "unlisted/Extra.class")
        metadata(self.root, release="8")
        write(self.root, self.path, "package p; class Main { int run(){ return unlisted.Extra.ping(); } }")
        result = run(request(self.root, [self.path], target=target))
        self.assertEqual(result["outcome"], "ok", result)
        calls = [ref["target"] for ref in source_method(result["graph"], self.path)["references"]]
        self.assertIn({"kind": "unresolved", "name": "ping"}, calls)
        self.assertFalse(any("unlisted.Extra" in item["symbol"] for item in result["graph"]["external_symbols"]))


if __name__ == "__main__":
    unittest.main()
