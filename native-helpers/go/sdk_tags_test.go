package main

import (
	"go/ast"
	"go/parser"
	"go/token"
	"go/version"
	"testing"
)

// TestRealSDKExperimentDefaults 验证真实 SDK 的默认实验项随 SDK 和目标系统变化。
func TestRealSDKExperimentDefaults(t *testing.T) {
	for _, sdk := range fixtureRealSDKs(t) {
		if version.Compare(version.Lang("go"+sdk.Version), compilerLanguage()) > 0 {
			continue
		}
		for _, goos := range []string{"darwin", "linux", "windows"} {
			sdk.Metadata = platformMetadata{GOOS: goos, GOARCH: "amd64", GOAMD64: "v1"}
			tags, err := sdkExperimentTags(sdk)
			if err != nil {
				t.Fatalf("real Go %s/%s defaults: %s", sdk.Version, goos, err)
			}
			if !containsString(tags, "goexperiment.regabiargs") || !containsString(tags, "goexperiment.regabiwrappers") {
				t.Fatalf("register ABI defaults lost: %v", tags)
			}
			if version.Lang("go"+sdk.Version) == "go1.23" && (!containsString(tags, "goexperiment.coverageredesign") || containsString(tags, "goexperiment.jsonv2")) {
				t.Fatalf("new compiler defaults leaked into old SDK: %v", tags)
			}
			if version.Lang("go"+sdk.Version) == "go1.27" {
				if !containsString(tags, "goexperiment.jsonv2") || !containsString(tags, "goexperiment.greenteagc") {
					t.Fatalf("new SDK defaults lost: %v", tags)
				}
				if containsString(tags, "goexperiment.dwarf5") != (goos != "darwin") {
					t.Fatalf("target-specific default was mixed: %s %v", goos, tags)
				}
			}
		}
	}
}

// TestSDKDefaultsRejectDynamicExpressions 验证 SDK 默认值读取器不会扩张为执行调用或任意 Go 语句。
func TestSDKDefaultsRejectDynamicExpressions(t *testing.T) {
	for _, source := range []string{"enabled()", "os.Getenv(\"GOEXPERIMENT\")", "1 + 2"} {
		expression, err := parser.ParseExpr(source)
		if err != nil {
			t.Fatal(err)
		}
		if _, err := sdkConstant(expression, nil); err == nil {
			t.Fatalf("dynamic expression accepted: %s", source)
		}
	}
	file, err := parser.ParseFile(token.NewFileSet(), "defaults.go", "package defaults\nfunc f(){for {}}", 0)
	if err != nil {
		t.Fatal(err)
	}
	function := file.Decls[0].(*ast.FuncDecl)
	if err := readSDKDefaults(function.Body.List[0], map[string]any{}); err == nil {
		t.Fatal("SDK loop was accepted")
	}
}
