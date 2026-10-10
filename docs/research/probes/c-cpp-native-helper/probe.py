#!/usr/bin/env python3
"""用已有 Clang 验证配置并集的源码证据；不是产品 helper 或完整构图实现。"""

import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile


CALLS_SOURCE = """#if FAST
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
"""

CATEGORY_SOURCE = """#if FAST
#define DECL typedef int
#else
#define DECL int
#endif
DECL item;
"""

PARENT_SOURCE = """#if FAST
struct Left {
#else
struct Right {
#endif
    int slot;
};
"""


def run_clang(clang, root, source, language, standard, fast):
    """只在临时夹具上运行固定分析参数，禁止头文件搜索并保留真实失败。"""
    suffix = ".c" if language == "c" else ".cpp"
    path = root / ("fixture" + suffix)
    path.write_text(source, encoding="utf-8")
    environment = os.environ.copy()
    for key in ("CPATH", "C_INCLUDE_PATH", "CPLUS_INCLUDE_PATH", "OBJC_INCLUDE_PATH"):
        environment.pop(key, None)
    environment["LC_ALL"] = "C"
    arguments = [
        clang, "-x", language, "-std=" + standard, "-pedantic-errors",
        "-fsyntax-only", "-nostdinc", "-DFAST=" + str(fast),
        "-Xclang", "-ast-dump=json", str(path),
    ]
    before = {item.name: hashlib.sha256(item.read_bytes()).hexdigest()
              for item in root.iterdir() if item.is_file()}
    result = subprocess.run(arguments, capture_output=True, text=True,
                            timeout=20, cwd=root, env=environment)
    assert result.returncode == 0, result.stderr
    after = {item.name: hashlib.sha256(item.read_bytes()).hexdigest()
             for item in root.iterdir() if item.is_file()}
    assert before == after, "分析调用更改了夹具文件"
    assert all(item.is_file() for item in root.iterdir()), "分析调用创建了目录"
    return json.loads(result.stdout)


def walk(node):
    """遍历 JSON AST；该格式仅用于观察，不当作正式进程协议。"""
    yield node
    for child in node.get("inner", []):
        yield from walk(child)


def physical(location):
    """本夹具取宏调用处的 expansionLoc；不泛化成完整宏源码映射算法。"""
    return location.get("expansionLoc", location)


def fragment(node, source):
    """从同一原始 UTF-8 文件截取声明；偏移只做本次源码证据，不是稳定 NodeId。"""
    begin = physical(node["range"]["begin"])["offset"]
    end_location = physical(node["range"]["end"])
    end = end_location["offset"] + end_location["tokLen"]
    raw = source.encode("utf-8")[begin:end].decode("utf-8")
    return {
        "begin": begin,
        "end": end,
        "name_offset": physical(node["loc"])["offset"],
        "line": source.encode("utf-8")[:begin].count(b"\n") + 1,
        "raw_source": raw,
    }


def named_declaration(ast, name):
    """在这些无重载的夹具中查找唯一有源码位置的具名声明。"""
    matches = [node for node in walk(ast)
               if node.get("name") == name and "loc" in node]
    assert len(matches) == 1, (name, len(matches))
    return matches[0]


def callable_observation(ast, name, source):
    """仅为夹具的直接调用保留 backend referencedDecl 证据，绝不按名字猜目标。"""
    declaration = named_declaration(ast, name)
    declarations = {node["id"]: node for node in walk(ast)
                    if node.get("kind") == "FunctionDecl" and "loc" in node}
    calls = []
    for node in walk(declaration):
        if node.get("kind") != "CallExpr":
            continue
        references = [child["referencedDecl"] for child in walk(node)
                      if child.get("kind") == "DeclRefExpr"]
        assert len(references) == 1, references
        target = declarations[references[0]["id"]]
        calls.append({
            "name": target["name"],
            "source": fragment(target, source),
            "evidence": "CallExpr/DeclRefExpr.referencedDecl.id",
        })
    return {
        "kind": declaration["kind"],
        "type": declaration["type"]["qualType"],
        "source": fragment(declaration, source),
        "calls": calls,
    }


def inspect_language(clang, root, language, standard, order):
    """比较两个显式配置，分别断言可归并关系、独立片段和真实冲突。"""
    observations = {}
    for fast in order:
        ast = run_clang(clang, root, CALLS_SOURCE, language, standard, fast)
        observations[fast] = {
            name: callable_observation(ast, name, CALLS_SOURCE)
            for name in ("run", "branch", "mode", "value")
        }
    inactive, active = observations[0], observations[1]
    assert inactive["run"]["source"] == active["run"]["source"]
    assert inactive["run"]["calls"][0]["name"] == "safe"
    assert active["run"]["calls"][0]["name"] == "fast"
    assert inactive["mode"]["source"] != active["mode"]["source"]
    assert inactive["branch"]["source"] == active["branch"]["source"]
    assert inactive["branch"]["calls"][0]["source"] == inactive["mode"]["source"]
    assert active["branch"]["calls"][0]["source"] == active["mode"]["source"]
    assert inactive["value"]["source"] == active["value"]["source"]
    assert inactive["value"]["kind"] == active["value"]["kind"]
    assert inactive["value"]["type"] != active["value"]["type"]

    categories = {}
    parents = {}
    for fast in order:
        ast = run_clang(clang, root, CATEGORY_SOURCE, language, standard, fast)
        item = named_declaration(ast, "item")
        categories[fast] = {"kind": item["kind"], "source": fragment(item, CATEGORY_SOURCE)}
        ast = run_clang(clang, root, PARENT_SOURCE, language, standard, fast)
        slot = named_declaration(ast, "slot")
        owners = [node for node in walk(ast)
                  if any(child.get("id") == slot["id"] for child in node.get("inner", []))]
        assert len(owners) == 1
        parents[fast] = {"parent": owners[0]["name"], "source": fragment(slot, PARENT_SOURCE)}
    assert categories[0]["source"] == categories[1]["source"]
    assert {categories[0]["kind"], categories[1]["kind"]} == {"VarDecl", "TypedefDecl"}
    assert parents[0]["source"] == parents[1]["source"]
    assert {parents[0]["parent"], parents[1]["parent"]} == {"Left", "Right"}

    return {
        "language": language,
        "standard": standard,
        "compatible_call_union": {
            "run_source": inactive["run"]["source"],
            "resolved_targets": sorted([inactive["run"]["calls"][0], active["run"]["calls"][0]],
                                       key=lambda item: item["name"]),
        },
        "separate_same_name_fragments": {
            "branch_source": inactive["branch"]["source"],
            "resolved_targets": sorted([inactive["branch"]["calls"][0], active["branch"]["calls"][0]],
                                       key=lambda item: item["source"]["begin"]),
            "conclusion": "两个 mode 源声明分别保留；不能按名字或链接名覆盖。",
        },
        "type_only_variation": {
            "source": inactive["value"]["source"],
            "types": sorted([inactive["value"]["type"], active["value"]["type"]]),
            "conclusion": "原始 RESULT 签名一致；不把展开类型强塞进源码内容。完整映射未验收。",
        },
        "category_conflict": {
            "source": categories[0]["source"],
            "native_kinds": sorted([categories[0]["kind"], categories[1]["kind"]]),
            "conclusion": "同一源码声明跨 Type/Value；按 Q5 整次失败，不能任选一个类别。",
        },
        "containment_conflict": {
            "source": parents[0]["source"],
            "parents": sorted([parents[0]["parent"], parents[1]["parent"]]),
            "conclusion": "若字段独立建对象，同一 slot 有两个父对象；按 Q5 整次失败。",
        },
    }


def language_mode_probe(clang, root):
    """验证简单源文件仍须分 C/C++ 模式，不执行或链接被分析程序。"""
    path = root / "language.c"
    path.write_text("void convert(void *raw) { int *p = raw; (void)p; }\n", encoding="utf-8")
    observations = []
    for language, standard, success in (("c", "c17", True), ("c++", "c++20", False)):
        result = subprocess.run([clang, "-x", language, "-std=" + standard,
                                 "-pedantic-errors", "-fsyntax-only", "-nostdinc", str(path)],
                                capture_output=True, text=True, timeout=20, cwd=root)
        assert (result.returncode == 0) == success, result.stderr
        observations.append({"language": language, "standard": standard, "exit_code": result.returncode})
    return observations


def main():
    """输出可复查结果；临时源文件自动清理，只有显式 --output 可写报告。"""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--clang", default=shutil.which("clang"))
    parser.add_argument("--output", type=Path)
    arguments = parser.parse_args()
    if not arguments.clang:
        parser.error("需要已有 clang；本探针不会安装工具")
    version = subprocess.check_output([arguments.clang, "--version"], text=True)
    with tempfile.TemporaryDirectory(prefix="narrant-c-cpp-union-", dir="/tmp") as directory:
        root = Path(directory)
        first = root / "first"
        relocated = root / "relocated"
        first.mkdir()
        relocated.mkdir()
        results = []
        for language, standard in (("c", "c17"), ("c++", "c++20")):
            normal = inspect_language(arguments.clang, first, language, standard, (0, 1))
            reverse = inspect_language(arguments.clang, relocated, language, standard, (1, 0))
            assert normal == reverse, "根目录或配置顺序影响了归一化探针证据"
            results.append(normal)
        mode = language_mode_probe(arguments.clang, first)
    report = {
        "tool_version": version.strip().splitlines(),
        "analysis_arguments": ["-x <c|c++>", "-std=<c17|c++20>", "-pedantic-errors",
                               "-fsyntax-only", "-nostdinc", "-DFAST=<0|1>", "-Xclang", "-ast-dump=json"],
        "mode_check": mode,
        "results": results,
        "checks": {
            "configuration_order_and_temporary_root_independent": True,
            "ast_fixture_content_unchanged_and_no_new_fixture_files": True,
            "analyzed_programs_linked_or_executed": False,
            "libtooling_helper_built_or_tested": False,
            "production_identity_and_full_code_graph_validated": False,
            "automatic_configuration_discovery_validated": False,
        },
    }
    text = json.dumps(report, ensure_ascii=False, indent=2) + "\n"
    if arguments.output:
        arguments.output.write_text(text, encoding="utf-8")
    else:
        print(text, end="")


if __name__ == "__main__":
    main()
