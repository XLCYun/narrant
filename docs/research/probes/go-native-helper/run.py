#!/usr/bin/env python3
"""用已有 Go SDK 复跑独立源码导入探针，不安装工具或下载模块。"""

import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile


def snapshot(root):
    """记录 fixture 的全部路径、文件字节和链接目标，不以权限位证明只读。"""
    result = {}
    for item in sorted(root.rglob("*")):
        relative = item.relative_to(root).as_posix()
        if item.is_symlink():
            result[relative] = ["symlink", os.readlink(item)]
        elif item.is_dir():
            result[relative] = ["directory"]
        else:
            result[relative] = ["file", hashlib.sha256(item.read_bytes()).hexdigest()]
    return result


def main():
    """构建自写分析器后禁用 PATH，再分析临时 fixture 并检验关系与原位内容。"""
    go = os.environ.get("NARRANT_GO_PROBE_BIN") or shutil.which("go")
    if not go:
        raise SystemExit("需要已有 Go SDK；不会自动安装")
    go = str(Path(go).resolve())
    with tempfile.TemporaryDirectory(prefix="narrant-go-native-") as directory:
        root = Path(directory)
        fixture = root / "fixture"
        (fixture / "app").mkdir(parents=True)
        (fixture / "app/local").mkdir()
        contents = {
            "go.work": "go 1.23.0\nuse ./app\n",
            "app/go.mod": "module example.test/app\n\ngo 1.23.0\nrequire example.test/missing v1.0.0\n",
            "app/common.go": '''package app
import (
    "cmp"
    "strings"
    "example.test/app/local"
    "example.test/missing"
)
// Run 包含平台分派、仓库调用、标准库调用和不可用依赖调用。
func Run() {
    platform()
    local.Local()
    _ = cmp.Compare(1, 2)
    _ = strings.Contains("a", "b")
    missing.Remote()
}
''',
            "app/platform_linux.go": '''//go:build linux

package app
// platform 使用 Linux 的目标。
func platform() { LinuxTarget() }
// LinuxTarget 是 Linux 独有声明。
func LinuxTarget() {}
''',
            "app/platform_windows.go": '''//go:build windows

package app
// platform 使用 Windows 的目标。
func platform() { WindowsTarget() }
// WindowsTarget 是 Windows 独有声明。
func WindowsTarget() {}
''',
            "app/local/local.go": "package local\n// Local 是允许读取的仓库声明。\nfunc Local() {}\n",
        }
        for filename, content in contents.items():
            (fixture / filename).write_text(content, encoding="utf-8")
        # 在专用 GOPATH 放置可读第三方源码，确认它不会因存在而被纳入。
        trap = root / "gopath/src/example.test/missing"
        trap.mkdir(parents=True)
        (trap / "missing.go").write_text("package missing\n// Remote 不应被纳入分析。\nfunc Remote() {}\n")
        environment = {
            **os.environ,
            "GOTOOLCHAIN": "local", "GOENV": "off", "GO111MODULE": "off",
            "GOWORK": "off", "GOFLAGS": "", "GOEXPERIMENT": "",
            "GOPACKAGESDRIVER": "off", "GOTELEMETRY": "off", "CGO_ENABLED": "0",
            "GOPROXY": "off", "GOSUMDB": "off", "GONOPROXY": "none",
            "GOPRIVATE": "", "GONOSUMDB": "", "GOCACHE": str(root / "cache"),
            "GOPATH": str(root / "gopath"), "GOMODCACHE": str(root / "modcache"),
            "GOTMPDIR": str(root),
        }
        for name in ["GOROOT", "GOOS", "GOARCH", "GOAMD64", "GOARM64"]:
            environment.pop(name, None)
        sdk = json.loads(subprocess.check_output(
            [go, "env", "-json", "GOROOT", "GOHOSTOS", "GOHOSTARCH", "GOVERSION"],
            cwd=root, env=environment, text=True, timeout=30,
        ))
        if not sdk["GOVERSION"].startswith("go1.23."):
            raise SystemExit("本探针固定 Go 1.23；其他 SDK 需要先调整版本断言并重新验证")
        binary = root / "probe"
        subprocess.run(
            [go, "build", "-o", str(binary), str(Path(__file__).with_name("probe.go"))],
            cwd=root, env=environment, check=True, capture_output=True, text=True, timeout=120,
        )
        before = snapshot(fixture)
        # 分析阶段没有可发现的命令；自写源码也没有 os/exec 或 importer.Default。
        analysis_environment = {**environment, "PATH": str(root / "no-tools")}
        analysis = subprocess.run(
            [str(binary), str(fixture), sdk["GOROOT"]], cwd=fixture,
            env=analysis_environment, check=True, capture_output=True, text=True, timeout=60,
        )
        profiles = json.loads(analysis.stdout)
        after = snapshot(fixture)
        assert before == after, "fixture 文件或路径被分析阶段修改"
        union = set()
        for profile in profiles:
            platform = profile["platform"].split("/")[0]
            other = "windows" if platform == "linux" else "linux"
            assert f"platform_{platform}.go" in profile["files"]
            assert f"platform_{other}.go" not in profile["files"]
            assert profile["rejected_imports"] == ["example.test/missing"]
            assert len(profile["diagnostics"]) == 1, profile["diagnostics"]
            assert "outside repository/stdlib policy" in profile["diagnostics"][0]
            calls = {item["spelling"]: item for item in profile["calls"]}
            assert calls["platform"]["target"] == f"repo:app/platform_{platform}.go#platform"
            assert calls["local.Local"]["target"] == "repo:app/local/local.go#Local"
            assert calls["cmp.Compare"]["target"] == "stdlib:cmp#Compare"
            assert calls["strings.Contains"]["target"] == "stdlib:strings#Contains"
            assert calls["missing.Remote"]["resolution"] == "Unresolved"
            assert calls["missing.Remote"]["target"] == ""
            for call in profile["calls"]:
                union.add((call["source"], call["spelling"], call["target"], call["resolution"]))
        assert len([item for item in union if item[1] == "platform"]) == 2
        output = {
            "sdk": {key: value for key, value in sdk.items() if key != "GOROOT"},
            "analysis_external_command_path": "none_by_implementation_review_not_process_trace",
            "fixture_unchanged": before == after,
            "fixture_paths_checked": len(before),
            "third_party_source_present_but_rejected": True,
            "union_observations": [dict(zip(["source", "spelling", "target", "resolution"], item)) for item in sorted(union)],
            "profiles": profiles,
        }
        print(json.dumps(output, ensure_ascii=False, indent=2))


if __name__ == "__main__":
    main()
