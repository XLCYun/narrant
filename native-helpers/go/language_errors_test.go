package main

import (
	"context"
	"strings"
	"testing"
)

// assertLanguageFailure 验证不支持的源码语言由公开入口原子失败，不能留下成功图或配置证据。
func assertLanguageFailure(t *testing.T, result completed, diagnostic, source string) {
	t.Helper()
	if result.Outcome != "error" || result.Graph != nil || result.Evidence != nil || result.Error == nil || result.Error.Code != "object_extraction_failed" || !strings.Contains(result.Error.Message, diagnostic) || !containsString(result.Error.Paths, source) {
		t.Fatalf("language violation did not fail atomically: %+v; error=%+v", result, result.Error)
	}
}

// TestVersionGatedSourceFailsEndToEnd 使用真实旧 SDK 验证类型检查器不同格式的语言门禁及旧目标实验门禁。
func TestVersionGatedSourceFailsEndToEnd(t *testing.T) {
	old, _ := fixtureCrossMinorSDKs(t)
	cases := []struct {
		name, language, source, diagnostic string
	}{
		{"integer_range", "1.21", "func Call(){}\nfunc Run(){for range 3 {Call()}}", "requires go1.22"},
		{"function_range", "1.22", "func Seq(yield func(int)bool){yield(1)}\nfunc Run(){for range Seq {}}", "requires go1.23"},
		{"generic_type", "1.17", "type Box[T any] struct{Value T}", "requires go1.18"},
		{"slice_to_array", "1.19", "func Run(){ s := []int{1}; _ = [1]int(s) }", "requires go1.20"},
		{"generic_alias_experiment", "1.23", "type Alias[T any] = []T", "requires GOEXPERIMENT=aliastypeparams"},
	}
	for _, test := range cases {
		t.Run(test.name, func(t *testing.T) {
			root := t.TempDir()
			writeFixture(t, root, "go.mod", "module example.test/app\ngo "+test.language+"\ntoolchain go"+old.Version+"\n")
			writeFixture(t, root, "unit.go", "package app\n"+test.source+"\n")
			req := fixtureVersionRequest(t, root, []platform{old}, "unit.go")
			assertLanguageFailure(t, analyze(context.Background(), req), test.diagnostic, "unit.go")
		})
	}
}

// TestRepositoryContextLanguageFailure 验证未选中的同包文件、仓内导入和 workspace 成员包不能吞掉新版语法。
func TestRepositoryContextLanguageFailure(t *testing.T) {
	old, newer := fixtureCrossMinorSDKs(t)
	for _, layout := range []string{"same_package", "imported_package", "workspace_module", "imported_alias"} {
		t.Run(layout, func(t *testing.T) {
			root := t.TempDir()
			selected, invalid := "unit.go", "context.go"
			language, diagnostic := "1.21", "requires go1.22"
			source := "package app\nfunc Call(){ for range 3 {} }\n"
			entry := "package app\nfunc Run(){ Call() }\n"
			if layout == "imported_package" || layout == "imported_alias" {
				invalid = "lib/context.go"
				entry = "package app\nimport \"example.test/app/lib\"\nfunc Run(){lib.Call()}\n"
			}
			if layout == "imported_alias" {
				language, diagnostic = "1.23", "requires GOEXPERIMENT=aliastypeparams"
				source = "package lib\ntype Alias[T any] = []T\nfunc Call(){}\n"
			} else if layout == "imported_package" {
				source = strings.Replace(source, "package app", "package lib", 1)
			}
			catalog := []platform{old}
			if layout == "workspace_module" {
				selected, invalid = "app/unit.go", "lib/context.go"
				writeFixture(t, root, "go.work", "go "+newer.Version+"\nuse (\n ./app\n ./lib\n)\n")
				writeFixture(t, root, "app/go.mod", "module example.test/app\ngo "+newer.Version+"\n")
				writeFixture(t, root, "lib/go.mod", "module example.test/lib\ngo 1.21\n")
				entry = "package app\nimport \"example.test/lib\"\nfunc Run(){lib.Call()}\n"
				source = strings.Replace(source, "package app", "package lib", 1)
				catalog = []platform{newer}
			} else {
				writeFixture(t, root, "go.mod", "module example.test/app\ngo "+language+"\ntoolchain go"+old.Version+"\n")
			}
			writeFixture(t, root, selected, entry)
			writeFixture(t, root, invalid, source)
			result := analyze(context.Background(), fixtureVersionRequest(t, root, catalog, selected))
			assertLanguageFailure(t, result, diagnostic, invalid)
		})
	}
}

// TestLanguageCompatibilityKeepsNativeExceptions 验证 release 标签与新 SDK 的原生别名语义，不把缺依赖或普通类型错误当成版本错误。
func TestLanguageCompatibilityKeepsNativeExceptions(t *testing.T) {
	old, newer := fixtureCrossMinorSDKs(t)
	root := t.TempDir()
	writeFixture(t, root, "go.mod", "module example.test/app\ngo 1.21\ntoolchain go"+old.Version+"\n")
	writeFixture(t, root, "unit.go", "//go:build go1.22\n\npackage app\nfunc Run(){for range 3 {}}\n")
	if result := analyze(context.Background(), fixtureVersionRequest(t, root, []platform{old}, "unit.go")); result.Outcome != "ok" {
		t.Fatalf("native per-file version override rejected: %+v", result)
	}
	writeFixture(t, root, "go.mod", "module example.test/app\ngo 1.23\ntoolchain go"+newer.Version+"\n")
	writeFixture(t, root, "unit.go", "package app\ntype Alias[T any] = []T\n")
	if result := analyze(context.Background(), fixtureVersionRequest(t, root, []platform{newer}, "unit.go")); result.Outcome != "ok" {
		t.Fatalf("new SDK inherited old alias experiment restriction: %+v", result)
	}
	writeFixture(t, root, "go.mod", "module example.test/app\ngo 1.23\n")
	writeFixture(t, root, "unit.go", "package app\nimport (\"strings\"; \"example.test/missing\")\nfunc Run(){ missing.Call(); _ = strings.SplitSeq(\"a,b\", \",\"); var _ int = \"requires go1.22 or later\" }\n")
	result := analyze(context.Background(), fixtureVersionRequest(t, root, []platform{old}, "unit.go"))
	if result.Outcome != "ok" {
		t.Fatalf("ordinary missing context became a language failure: %+v", result)
	}
	run := findNode(result.Graph, "go:unit.go#func:Run")
	if run == nil || len(run.References) != 2 {
		t.Fatalf("missing uncertain call observations: %+v", run)
	}
	for _, ref := range run.References {
		if ref.Target.Kind != "unresolved" {
			t.Fatalf("missing dependency or old API was resolved: %+v", ref)
		}
	}
}

// TestImportedPackageCacheTestScope 验证先导入的包不会借用不含测试的缓存，漏掉选中测试对象或其语言错误。
func TestImportedPackageCacheTestScope(t *testing.T) {
	old, _ := fixtureCrossMinorSDKs(t)
	orders := []struct {
		name  string
		files []string
	}{
		{"app_first", []string{"app.go", "lib/lib_test.go"}},
		{"test_first", []string{"lib/lib_test.go", "app.go"}},
	}
	cases := []struct {
		name, body string
		invalid    bool
	}{
		{"ordinary_call", "Call()", false},
		{"forbidden_range", "for range 3 { Call() }", true},
	}
	for _, order := range orders {
		for _, test := range cases {
			t.Run(order.name+"_"+test.name, func(t *testing.T) {
				root := t.TempDir()
				writeFixture(t, root, "go.mod", "module example.test/app\ngo 1.21\ntoolchain go"+old.Version+"\n")
				writeFixture(t, root, "app.go", "package app\nimport \"example.test/app/lib\"\nfunc Run(){lib.Call()}\n")
				writeFixture(t, root, "lib/lib.go", "package lib\nfunc Call(){}\n")
				writeFixture(t, root, "lib/lib_test.go", "package lib\nfunc TestCall(){"+test.body+"}\n")
				req := fixtureVersionRequest(t, root, []platform{old}, order.files...)
				if result := analyze(context.Background(), req); result.Outcome != "ok" || findNode(result.Graph, "go:lib/lib_test.go#func:TestCall") != nil {
					t.Fatalf("disabled test changed production scope: %+v", result)
				}
				req.Input.IncludeTests = true
				result := analyze(context.Background(), req)
				if test.invalid {
					assertLanguageFailure(t, result, "requires go1.22", "lib/lib_test.go")
					return
				}
				if result.Outcome != "ok" {
					t.Fatalf("valid selected test rejected: %+v", result)
				}
				node := findNode(result.Graph, "go:lib/lib_test.go#func:TestCall")
				if node == nil || len(node.References) != 1 || node.References[0].Target.Symbol != "go:lib/lib.go#func:Call" {
					t.Fatalf("selected test object or call lost from imported package cache: %+v", node)
				}
				covered := false
				for _, file := range result.Evidence.Files {
					if file.Path == "lib/lib_test.go" && file.Disposition == "analyzed" && len(file.ConfigurationIDs) == 1 {
						covered = true
					}
				}
				if !covered {
					t.Fatalf("test node lacks matching analyzed evidence: %+v", result.Evidence.Files)
				}
			})
		}
	}
}
