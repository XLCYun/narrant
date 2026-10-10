package main

import (
	"context"
	"fmt"
	"go/version"
	"os"
	"path/filepath"
	"runtime"
	"sort"
	"strconv"
	"strings"
	"testing"
)

// fixtureSDKVersion 直接读取真实 SDK 的 VERSION，禁止给同一份源码伪造另一个版本。
func fixtureSDKVersion(t *testing.T, root string) string {
	t.Helper()
	data, err := os.ReadFile(filepath.Join(root, "VERSION"))
	if err != nil {
		t.Fatal(err)
	}
	line, _, _ := strings.Cut(string(data), "\n")
	value := strings.TrimPrefix(strings.TrimSpace(line), "go")
	if !validSDKVersion(value) {
		t.Fatalf("test SDK %s is not a stable Go distribution: %q", root, value)
	}
	return value
}

// fixtureLanguage 让通用图回归使用实际编译器 SDK，不把其源码冒充为旧版本。
func fixtureLanguage(t *testing.T) string {
	t.Helper()
	return strings.TrimPrefix(version.Lang("go"+fixtureSDKVersion(t, runtime.GOROOT())), "go")
}

// fixtureRealSDKs 从显式测试目录和本机已知安装读取真实 SDK，不下载或执行 Go。
func fixtureRealSDKs(t *testing.T) []platform {
	t.Helper()
	roots := append(filepath.SplitList(os.Getenv("NARRANT_GO_TEST_SDKS")), runtime.GOROOT(), "/usr/local/go")
	seen := map[string]bool{}
	var sdks []platform
	for _, root := range roots {
		if root == "" {
			continue
		}
		resolved, err := filepath.EvalSymlinks(root)
		if err != nil {
			continue
		}
		if seen[resolved] {
			continue
		}
		seen[resolved] = true
		meta := platformMetadata{GOOS: runtime.GOOS, GOARCH: runtime.GOARCH}
		if runtime.GOARCH == "amd64" {
			meta.GOAMD64 = "v1"
		}
		sdk := platform{ID: fmt.Sprintf("real-sdk-%d", len(sdks)), Kind: "go-sdk", Version: fixtureSDKVersion(t, resolved), Root: resolved, Metadata: meta}
		if err := validatePlatform(sdk); err != nil {
			t.Fatal(err)
		}
		sdks = append(sdks, sdk)
	}
	sort.Slice(sdks, func(i, j int) bool { return version.Compare("go"+sdks[i].Version, "go"+sdks[j].Version) < 0 })
	return sdks
}

// fixtureCrossMinorSDKs 要求真实 1.23 样本与至少 1.24 的另一份源码，缺失时明确跳过。
func fixtureCrossMinorSDKs(t *testing.T) (platform, platform) {
	t.Helper()
	var old, newer platform
	for _, sdk := range fixtureRealSDKs(t) {
		language := version.Lang("go" + sdk.Version)
		if language == "go1.23" {
			old = sdk
		}
		if version.Compare(language, "go1.24") >= 0 && version.Compare(language, compilerLanguage()) <= 0 {
			newer = sdk
		}
	}
	if old.Root == "" || newer.Root == "" {
		t.Skip("cross-minor regression requires real Go 1.23 and >= 1.24 SDKs; set NARRANT_GO_TEST_SDKS")
	}
	t.Logf("real targets: Go %s at %s; Go %s at %s; helper %s", old.Version, old.Root, newer.Version, newer.Root, runtime.Version())
	return old, newer
}

// fixtureVersionRequest 将真实 SDK catalog 放入标准协议请求。
func fixtureVersionRequest(t *testing.T, root string, sdks []platform, files ...string) request {
	t.Helper()
	req := fixtureRequest(t, root, files...)
	req.Environment.Platforms = sdks
	return req
}

// fixtureNextPatch 构造高于真实 SDK 的最低 patch，用于验证缺失版本不能被降级满足。
func fixtureNextPatch(t *testing.T, value string) string {
	t.Helper()
	parts := strings.Split(value, ".")
	if len(parts) == 2 {
		return value + ".1"
	}
	patch, err := strconv.Atoi(parts[2])
	if err != nil {
		t.Fatal(err)
	}
	return strings.Join(parts[:2], ".") + "." + strconv.Itoa(patch+1)
}

// TestActualSDKPatchRequirements 验证真实 SDK 能满足同 minor 的较低/相等 patch，但不能满足更高 patch。
func TestActualSDKPatchRequirements(t *testing.T) {
	for _, sdk := range fixtureRealSDKs(t) {
		if version.Compare(version.Lang("go"+sdk.Version), compilerLanguage()) > 0 {
			continue
		}
		t.Run(sdk.Version, func(t *testing.T) {
			root := t.TempDir()
			writeFixture(t, root, "unit.go", "package app\nfunc F(){}\n")
			for _, required := range []string{strings.TrimPrefix(version.Lang("go"+sdk.Version), "go"), sdk.Version, fixtureNextPatch(t, sdk.Version)} {
				writeFixture(t, root, "go.mod", "module example.test/app\ngo "+required+"\n")
				result := analyze(context.Background(), fixtureVersionRequest(t, root, []platform{sdk}, "unit.go"))
				if required == fixtureNextPatch(t, sdk.Version) {
					if result.Graph != nil || result.Error == nil || result.Error.Code != "target_unavailable" {
						t.Fatalf("newer patch was downgraded: required=%s result=%+v", required, result)
					}
				} else if result.Outcome != "ok" || len(result.Evidence.Configurations) != 1 {
					t.Fatalf("compatible real patch rejected: required=%s error=%+v", required, result.Error)
				}
			}
		})
	}
}

// TestRealTargetSDKAPIAndReleaseTags 验证两份真实标准库的 API、release 标签和单一配置不会互相污染。
func TestRealTargetSDKAPIAndReleaseTags(t *testing.T) {
	old, newer := fixtureCrossMinorSDKs(t)
	root := t.TempDir()
	writeFixture(t, root, "unit.go", "package app\nimport \"strings\"\nfunc Run(){_ = strings.SplitSeq(\"a,b\", \",\"); platform()}\n")
	writeFixture(t, root, "old.go", "//go:build !go1.24\n\npackage app\nfunc platform(){}\n")
	writeFixture(t, root, "new.go", "//go:build go1.24\n\npackage app\nfunc platform(){}\n")
	writeFixture(t, root, "future.go", "//go:build go1.999\n\npackage app\nfunc thisFileMustNotBeParsed( {\n")
	for _, sdk := range []platform{old, newer} {
		writeFixture(t, root, "go.mod", "module example.test/app\ngo "+sdk.Version+"\n")
		duplicate := sdk
		duplicate.ID += "-duplicate"
		result := analyze(context.Background(), fixtureVersionRequest(t, root, []platform{newer, old, duplicate}, "unit.go"))
		if result.Outcome != "ok" || len(result.Evidence.Configurations) != 1 || result.Evidence.Configurations[0].PlatformIDs[0] != sdk.ID {
			t.Fatalf("SDK versions were mixed or duplicated for %s: %+v", sdk.Version, result)
		}
		run := findNode(result.Graph, "go:unit.go#func:Run")
		if run == nil {
			t.Fatal("missing Run")
		}
		api, platformFile := "", ""
		for _, ref := range run.References {
			if ref.Semantic != "code.calls" {
				continue
			}
			if strings.Contains(ref.Target.Symbol+ref.Target.Name, "SplitSeq") {
				api = ref.Target.Kind
			}
			if strings.Contains(ref.Target.Symbol, "#func:platform") {
				platformFile = ref.Target.Symbol
			}
		}
		wantAPI, wantFile := "external", "go:new.go#func:platform"
		if sdk.Version == old.Version {
			wantAPI, wantFile = "unresolved", "go:old.go#func:platform"
		}
		if api != wantAPI || platformFile != wantFile {
			t.Fatalf("wrong API or release tags for %s: api=%s platform=%s refs=%+v", sdk.Version, api, platformFile, run.References)
		}
	}
	writeFixture(t, root, "go.mod", "module example.test/app\ngo "+old.Version+"\n")
	missing := analyze(context.Background(), fixtureVersionRequest(t, root, []platform{newer}, "unit.go"))
	if missing.Graph != nil || missing.Error == nil || missing.Error.Code != "target_unavailable" {
		t.Fatalf("missing old minor silently borrowed newer SDK: %+v", missing)
	}
}

// TestDeclaredToolchainSelection 验证正式 toolchain、default 和缺失 patch 的明确选择语义。
func TestDeclaredToolchainSelection(t *testing.T) {
	old, newer := fixtureCrossMinorSDKs(t)
	root := t.TempDir()
	writeFixture(t, root, "unit.go", "package app\nfunc Run(){}\n")
	cases := []struct {
		name, toolchain, wantID, wantError string
	}{
		{name: "implicit", wantID: old.ID},
		{name: "default", toolchain: "default", wantID: old.ID},
		{name: "explicit_older", toolchain: "go" + old.Version, wantID: old.ID},
		{name: "explicit_newer", toolchain: "go" + newer.Version, wantID: newer.ID},
		{name: "missing_patch", toolchain: "go" + fixtureNextPatch(t, newer.Version), wantError: "target_unavailable"},
	}
	for _, test := range cases {
		t.Run(test.name, func(t *testing.T) {
			config := "module example.test/app\ngo " + old.Version + "\n"
			if test.toolchain != "" {
				config += "toolchain " + test.toolchain + "\n"
			}
			writeFixture(t, root, "go.mod", config)
			result := analyze(context.Background(), fixtureVersionRequest(t, root, []platform{newer, old}, "unit.go"))
			if test.wantError != "" {
				if result.Graph != nil || result.Error == nil || result.Error.Code != test.wantError {
					t.Fatalf("expected %s: %+v", test.wantError, result)
				}
			} else if result.Outcome != "ok" || len(result.Evidence.Configurations) != 1 || result.Evidence.Configurations[0].PlatformIDs[0] != test.wantID {
				t.Fatalf("wrong toolchain selection: %+v", result)
			}
		})
	}
}

// TestIndependentModuleSDKs 验证独立 module 各选 SDK，且不能因为一起选中就获得另一 module 的导入证据。
func TestIndependentModuleSDKs(t *testing.T) {
	old, newer := fixtureCrossMinorSDKs(t)
	root := t.TempDir()
	writeFixture(t, root, "old/go.mod", "module example.test/old\ngo "+old.Version+"\n")
	writeFixture(t, root, "old/unit.go", "package old\nimport \"example.test/new\"\nfunc Run(){ new.Call() }\n")
	writeFixture(t, root, "new/go.mod", "module example.test/new\ngo "+newer.Version+"\n")
	writeFixture(t, root, "new/unit.go", "package new\nfunc Call(){}\n")
	result := analyze(context.Background(), fixtureVersionRequest(t, root, []platform{old, newer}, "old/unit.go", "new/unit.go"))
	if result.Outcome != "ok" || len(result.Evidence.Configurations) != 2 {
		t.Fatalf("independent module selection failed: %+v", result)
	}
	run := findNode(result.Graph, "go:old/unit.go#func:Run")
	if run == nil || len(run.References) != 1 || run.References[0].Target.Kind != "unresolved" {
		t.Fatalf("independent module became an implicit workspace import: %+v", run)
	}
	for _, file := range result.Evidence.Files {
		if len(file.ConfigurationIDs) != 1 {
			t.Fatalf("file was analyzed under unrelated module SDK: %+v", file)
		}
	}
}

// TestWorkspacePackageLanguageVersions 验证 workspace 共用 SDK，但旧包的新版语法必须通过公开 analyze 原子失败。
func TestWorkspacePackageLanguageVersions(t *testing.T) {
	_, newer := fixtureCrossMinorSDKs(t)
	root := t.TempDir()
	writeFixture(t, root, "go.work", "go "+newer.Version+"\ntoolchain default\nuse (\n ./old\n ./new\n)\n")
	writeFixture(t, root, "old/go.mod", "module example.test/old\ngo 1.21\ntoolchain go1.999.0\n")
	writeFixture(t, root, "new/go.mod", "module example.test/new\ngo "+newer.Version+"\n")
	for _, dir := range []string{"old", "new"} {
		writeFixture(t, root, dir+"/unit.go", "package app\nfunc Run(){ for i := range 3 { _ = i } }\n")
	}
	req := fixtureVersionRequest(t, root, []platform{newer}, "old/unit.go", "new/unit.go")
	assertLanguageFailure(t, analyze(context.Background(), req), "requires go1.22", "old/unit.go")
	req.Input.Files = []string{"new/unit.go"}
	if result := analyze(context.Background(), req); result.Outcome != "ok" {
		t.Fatalf("new module adopted old module language rules: %+v", result)
	}
	writeFixture(t, root, "old/go.mod", "module example.test/old\ngo 1.22\ntoolchain go1.999.0\n")
	req.Input.Files = []string{"old/unit.go", "new/unit.go"}
	if result := analyze(context.Background(), req); result.Outcome != "ok" {
		t.Fatalf("version-compatible workspace rejected: %+v", result)
	}
	writeFixture(t, root, "go.work", "go 1.21\nuse (\n ./old\n ./new\n)\n")
	if _, problem := loadProject(root, []string{"old/unit.go"}); problem == nil || problem.Code != "unsupported_configuration" {
		t.Fatalf("workspace older than used module accepted: %+v", problem)
	}
}

// TestWorkspaceToolchainAndReplacementLanguage 验证未选中的本地 replace 函数体也保留自身语言门禁与配置来源。
func TestWorkspaceToolchainAndReplacementLanguage(t *testing.T) {
	old, newer := fixtureCrossMinorSDKs(t)
	root := t.TempDir()
	writeFixture(t, root, "go.work", "go "+old.Version+"\ntoolchain go"+newer.Version+"\nuse ./app\nreplace example.test/dep => ./dep\n")
	writeFixture(t, root, "app/go.mod", "module example.test/app\ngo "+old.Version+"\n")
	writeFixture(t, root, "app/unit.go", "package app\nimport \"example.test/dep\"\nfunc Run(){dep.Call()}\n")
	writeFixture(t, root, "dep/go.mod", "module example.test/dep\ngo 1.21\ntoolchain go1.999.0\n")
	writeFixture(t, root, "dep/unit.go", "package dep\nfunc Call(){for i := range 3 {_ = i}}\n")
	req := fixtureVersionRequest(t, root, []platform{old, newer}, "app/unit.go")
	assertLanguageFailure(t, analyze(context.Background(), req), "requires go1.22", "dep/unit.go")
	writeFixture(t, root, "dep/go.mod", "module example.test/dep\ngo 1.22\ntoolchain go1.999.0\n")
	result := analyze(context.Background(), req)
	if result.Outcome != "ok" || len(result.Evidence.Configurations) != 1 || result.Evidence.Configurations[0].PlatformIDs[0] != newer.ID {
		t.Fatalf("workspace toolchain did not select real newer SDK: %+v", result)
	}
	if !containsString(result.Evidence.Configurations[0].SourcePaths, "dep/go.mod") {
		t.Fatalf("replacement language source was omitted: %+v", result.Evidence.Configurations)
	}
}

// TestFutureVersionAndCatalogTruth 验证真实编译器能力边界、严格声明和 catalog 与磁盘版本一致性。
func TestFutureVersionAndCatalogTruth(t *testing.T) {
	root := t.TempDir()
	writeFixture(t, root, "unit.go", "package app\nfunc Run(){}\n")
	minor, err := strconv.Atoi(strings.TrimPrefix(compilerLanguage(), "go1."))
	if err != nil {
		t.Fatal(err)
	}
	future := fmt.Sprintf("1.%d.0", minor+1)
	for _, configuration := range []string{"go " + future, "go " + fixtureLanguage(t) + "\ntoolchain go" + future} {
		writeFixture(t, root, "go.mod", "module example.test/app\n"+configuration+"\n")
		result := analyze(context.Background(), fixtureRequest(t, root, "unit.go"))
		if result.Graph != nil || result.Error == nil || result.Error.Code != "unsupported_configuration" || !strings.Contains(result.Error.Message, runtime.Version()) {
			t.Fatalf("future compiler requirement was not explicit: %+v", result)
		}
	}
	for _, declaration := range []string{"go 1.23rc1", "go 1.023", "go 1.23\ngo 1.23", "go 1.23\ntoolchain go1.24rc1", "go 1.23\ntoolchain default\ntoolchain default"} {
		writeFixture(t, root, "go.mod", "module example.test/app\n"+declaration+"\n")
		if _, err := parseMod(root, filepath.Join(root, "go.mod")); err == nil {
			t.Fatalf("invalid declaration accepted: %s", declaration)
		}
	}
	writeFixture(t, root, "go.mod", "module example.test/app\ngo "+fixtureLanguage(t)+"\n")
	req := fixtureRequest(t, root, "unit.go")
	req.Environment.Platforms[0].Version = fixtureNextPatch(t, req.Environment.Platforms[0].Version)
	result := analyze(context.Background(), req)
	if result.Graph != nil || result.Error == nil || result.Error.Code != "invalid_input" || !strings.Contains(result.Error.Message, "VERSION") {
		t.Fatalf("catalog version was trusted over real VERSION: %+v", result)
	}
}
