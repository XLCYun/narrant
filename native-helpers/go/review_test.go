package main

import (
	"context"
	"encoding/json"
	"io"
	"os"
	"path/filepath"
	"runtime"
	"strings"
	"testing"
	"time"
)

// TestBlankInitializerOwnership 验证空白变量的独立求值和共享求值只拥有一份内容及调用。
func TestBlankInitializerOwnership(t *testing.T) {
	root := t.TempDir()
	writeFixture(t, root, "go.mod", "module example.test/app\ngo 1.23.0\n")
	source := "package app\nfunc effect(x int)int{return x}\nfunc pair()(int,int){return 1,2}\nvar _ = effect(%s)\nvar _, A = effect(3), effect(4)\nvar _, B = pair()\n"
	var first *graph
	for _, argument := range []string{"1", "2"} {
		writeFixture(t, root, "a.go", strings.ReplaceAll(source, "%s", argument))
		result := analyze(context.Background(), fixtureRequest(t, root, "a.go"))
		if result.Outcome != "ok" {
			t.Fatal(result.Error)
		}
		calls := 0
		var owner *node
		for i := range result.Graph.Nodes {
			n := &result.Graph.Nodes[i]
			for _, r := range n.References {
				if r.Semantic == "code.calls" {
					calls++
				}
			}
			if n.Code != nil && *n.Code == "effect ( "+argument+" )" {
				owner = n
			}
		}
		if calls != 4 || owner == nil {
			t.Fatalf("initializer ownership missing: calls=%d owner=%+v", calls, owner)
		}
		if len(owner.References) != 1 || owner.References[0].Semantic != "code.calls" || owner.References[0].Target.ID != "go:a.go#func:effect" {
			t.Fatalf("blank initializer call target lost: %+v", owner)
		}
		if first != nil {
			before := findNode(first, owner.ID)
			if before == nil || before.Code == nil || *before.Code == *owner.Code {
				t.Fatal("blank edit lost stable owner/content")
			}
		}
		first = result.Graph
	}
}

// TestImportedContextFailures 验证仓内导入的未知配置及损坏源码都必须原子失败。
func TestImportedContextFailures(t *testing.T) {
	for _, source := range []string{"//go:build feature\n\npackage lib\nfunc Call(){}\n", "package lib\nfunc Call( {\n"} {
		root := t.TempDir()
		writeFixture(t, root, "go.mod", "module example.test/app\ngo 1.23.0\n")
		writeFixture(t, root, "a.go", "package app\nimport \"example.test/app/lib\"\nfunc Run(){lib.Call()}\n")
		writeFixture(t, root, "lib/a.go", source)
		result := analyze(context.Background(), fixtureRequest(t, root, "a.go"))
		if result.Error == nil || result.Graph != nil {
			t.Fatalf("admitted failure downgraded: %+v", result)
		}
	}
}

// TestImportedExcludedTests 验证依赖测试未参与时忽略其配置，但已启用的必要测试仍严格校验。
func TestImportedExcludedTests(t *testing.T) {
	root := t.TempDir()
	writeFixture(t, root, "go.mod", "module example.test/app\ngo 1.23.0\n")
	writeFixture(t, root, "a.go", "package app\nimport \"example.test/app/lib\"\nfunc Run(){lib.Call()}\n")
	writeFixture(t, root, "lib/a.go", "package lib\nfunc Call(){}\n")
	writeFixture(t, root, "lib/optional_test.go", "//go:build integration\n\npackage lib\nfunc Optional(){}\n")
	req := fixtureRequest(t, root, "a.go")
	for _, includeTests := range []bool{false, true} {
		req.Input.IncludeTests = includeTests
		result := analyze(context.Background(), req)
		if result.Outcome != "ok" {
			t.Fatalf("unused dependency test rejected production: %+v", result.Error)
		}
		run := findNode(result.Graph, "go:a.go#func:Run")
		if run == nil || len(run.References) != 1 || run.References[0].Semantic != "code.calls" ||
			run.References[0].Target.Kind != "external" || run.References[0].Target.Symbol != "go:lib/a.go#func:Call" {
			t.Fatalf("confirmed repository call lost: %+v", run)
		}
	}
	req.Input.Files = []string{"lib/a.go"}
	result := analyze(context.Background(), req)
	if result.Graph != nil || result.Error == nil || result.Error.Code != "unsupported_configuration" {
		t.Fatalf("required enabled test configuration ignored: %+v", result)
	}
}

// TestImportedTargetDiscovery 验证传递仓内导入的目标条件决定配置并集与缺失目标报错。
func TestImportedTargetDiscovery(t *testing.T) {
	root := t.TempDir()
	writeFixture(t, root, "go.mod", "module example.test/app\ngo 1.23.0\n")
	writeFixture(t, root, "a.go", "package app\nimport \"example.test/app/lib\"\nfunc Run(){lib.Call()}\n")
	writeFixture(t, root, "lib/a.go", "package lib\nimport \"example.test/app/impl\"\nfunc Call(){impl.Call()}\n")
	writeFixture(t, root, "impl/a_linux.go", "package impl\nfunc Call(){}\n")
	writeFixture(t, root, "impl/a_windows.go", "package impl\nfunc Call(){}\n")
	req := fixtureRequest(t, root, "a.go")
	req.Environment.Platforms = req.Environment.Platforms[:2]
	result := analyze(context.Background(), req)
	if result.Outcome != "ok" || len(result.Evidence.Configurations) != 2 {
		t.Fatalf("import targets lost: %+v", result)
	}
	req.Environment.Platforms = req.Environment.Platforms[:1]
	result = analyze(context.Background(), req)
	if result.Error == nil || result.Error.Code != "target_unavailable" {
		t.Fatalf("missing import target ignored: %+v", result)
	}
}

// TestGenericReceiverOwner 验证泛型值与指针 receiver 都指向基础类型而非类型参数。
func TestGenericReceiverOwner(t *testing.T) {
	root := t.TempDir()
	writeFixture(t, root, "go.mod", "module example.test/app\ngo 1.23.0\n")
	writeFixture(t, root, "a.go", "package app\ntype Box[T any] struct{ V T }\nfunc(b Box[T]) Read()T{return b.V}\nfunc(b *Box[T]) Write(v T){b.V=v}\n")
	result := analyze(context.Background(), fixtureRequest(t, root, "a.go"))
	if result.Outcome != "ok" {
		t.Fatal(result.Error)
	}
	count := 0
	for _, n := range result.Graph.Nodes {
		for _, r := range n.References {
			if r.Semantic == "code.member_of" {
				count++
				if r.Target.Kind != "node" || r.Target.ID != "go:a.go#type:Box" {
					t.Fatalf("wrong owner: %+v", r)
				}
			}
		}
	}
	if count != 2 {
		t.Fatalf("member count: %d", count)
	}
	for _, external := range result.Graph.ExternalSymbols {
		if strings.HasSuffix(external.Symbol, "#T") {
			t.Fatal("invented dependency T")
		}
	}
}

// TestContextPlatformDiscovery 验证未选同包平台文件仍决定分析配置及调用并集。
func TestContextPlatformDiscovery(t *testing.T) {
	root := t.TempDir()
	writeFixture(t, root, "go.mod", "module example.test/app\ngo 1.23.0\n")
	writeFixture(t, root, "common.go", "package app\nfunc Run(){ platform() }\n")
	writeFixture(t, root, "impl_linux.go", "package app\nfunc platform(){}\n")
	writeFixture(t, root, "impl_windows.go", "package app\nfunc platform(){}\n")
	req := fixtureRequest(t, root, "common.go")
	req.Environment.Platforms = req.Environment.Platforms[:2]
	response := analyze(context.Background(), req)
	if response.Outcome != "ok" {
		t.Fatalf("analysis: %+v", response.Error)
	}
	if len(response.Evidence.Configurations) != 2 {
		t.Fatalf("context targets lost: %+v", response.Evidence.Configurations)
	}
	run := findNode(response.Graph, "go:common.go#func:Run")
	if run == nil || len(run.References) != 2 {
		t.Fatalf("context call union lost: %+v", run)
	}
	for _, ref := range run.References {
		if ref.Target.Kind != "external" || !strings.Contains(ref.Target.Symbol, "impl_") {
			t.Fatalf("wrong context target: %+v", ref)
		}
	}
	req.Environment.Platforms = req.Environment.Platforms[:1]
	missing := analyze(context.Background(), req)
	if missing.Error == nil || missing.Error.Code != "target_unavailable" || missing.Graph != nil {
		t.Fatalf("missing Windows context target was silently ignored: %+v", missing)
	}
}

// TestOrdinaryUnderscoreFilename 验证非末尾的 linux 片段不扩大全部 catalog 目标。
func TestOrdinaryUnderscoreFilename(t *testing.T) {
	root := t.TempDir()
	writeFixture(t, root, "go.mod", "module example.test/app\ngo 1.23.0\n")
	writeFixture(t, root, "foo_linux_helper.go", "package app\nfunc Helper(){}\n")
	req := fixtureRequest(t, root, "foo_linux_helper.go")
	response := analyze(context.Background(), req)
	if response.Outcome != "ok" {
		t.Fatalf("analysis: %+v", response.Error)
	}
	if len(response.Evidence.Configurations) == 0 {
		t.Fatal("missing native baseline")
	}
	for _, config := range response.Evidence.Configurations {
		found := false
		for _, candidate := range req.Environment.Platforms {
			if candidate.ID == config.PlatformIDs[0] && candidate.Metadata.GOOS == runtime.GOOS && candidate.Metadata.GOARCH == runtime.GOARCH {
				found = true
			}
		}
		if !found {
			t.Fatalf("ordinary filename expanded target set: %+v", response.Evidence.Configurations)
		}
	}
}

// TestBooleanBuildSelection 验证布尔标签按目标整体求值，而非 OS/架构笛卡尔积。
func TestBooleanBuildSelection(t *testing.T) {
	root := t.TempDir()
	writeFixture(t, root, "go.mod", "module example.test/app\ngo 1.23.0\n")
	writeFixture(t, root, "common.go", "package app\n")
	writeFixture(t, root, "choice.go", "//go:build linux || amd64\n\npackage app\n")
	sdk := runtime.GOROOT()
	catalog := []platform{{ID: "la", Kind: "go-sdk", Version: "1.23.8", Root: sdk, Fingerprint: "x", Metadata: platformMetadata{GOOS: "linux", GOARCH: "amd64", GOAMD64: "v1"}}, {ID: "ll", Kind: "go-sdk", Version: "1.23.8", Root: sdk, Fingerprint: "x", Metadata: platformMetadata{GOOS: "linux", GOARCH: "arm64"}}, {ID: "wa", Kind: "go-sdk", Version: "1.23.8", Root: sdk, Fingerprint: "x", Metadata: platformMetadata{GOOS: "windows", GOARCH: "amd64", GOAMD64: "v1"}}, {ID: "wl", Kind: "go-sdk", Version: "1.23.8", Root: sdk, Fingerprint: "x", Metadata: platformMetadata{GOOS: "windows", GOARCH: "arm64"}}}
	p, problem := loadProject(root, []string{"common.go"})
	if problem != nil {
		t.Fatal(problem)
	}
	chosen, problem := discoverPlatforms(p, []string{"common.go"}, false, catalog)
	if problem != nil {
		t.Fatal(problem)
	}
	if len(chosen) != 3 {
		t.Fatalf("linux || amd64 selected %v", chosen)
	}
	writeFixture(t, root, "choice.go", "//go:build (linux && amd64) || (windows && arm64)\n\npackage app\n")
	chosen, problem = discoverPlatforms(p, []string{"common.go"}, false, catalog)
	if problem != nil {
		t.Fatal(problem)
	}
	if len(chosen) != 2 || chosen[0].ID != "la" || chosen[1].ID != "wl" {
		t.Fatalf("wrong Boolean target set: %v", chosen)
	}
}

// TestWorkspaceReplacementPrecedence 验证 work 优先、模块冲突和最长前缀匹配。
func TestWorkspaceReplacementPrecedence(t *testing.T) {
	root := t.TempDir()
	writeFixture(t, root, "go.work", "go 1.23.0\nuse (\n ./app\n ./lib\n)\nreplace example.test/dep => ./override\nreplace example.test/dep/sub => ./deep\n")
	writeFixture(t, root, "app/go.mod", "module example.test/app\ngo 1.23.0\nreplace example.test/dep => ../one\n")
	writeFixture(t, root, "lib/go.mod", "module example.test/lib\ngo 1.23.0\nreplace example.test/dep => ../two\n")
	writeFixture(t, root, "app/a.go", "package app\n")
	for _, dir := range []string{"one", "two", "override", "deep"} {
		if err := os.MkdirAll(filepath.Join(root, dir), 0755); err != nil {
			t.Fatal(err)
		}
	}
	p, problem := loadProject(root, []string{"app/a.go"})
	if problem != nil {
		t.Fatal(problem)
	}
	got, ok, err := p.importDirectory("example.test/dep/item")
	if err != nil || !ok || got != filepath.Join(root, "override", "item") {
		t.Fatalf("work override lost: %q %v %v", got, ok, err)
	}
	got, ok, err = p.importDirectory("example.test/dep/sub/item")
	if err != nil || !ok || got != filepath.Join(root, "deep", "item") {
		t.Fatalf("longest replace lost: %q %v %v", got, ok, err)
	}
	writeFixture(t, root, "go.work", "go 1.23.0\nuse (\n ./app\n ./lib\n)\n")
	if _, problem := loadProject(root, []string{"app/a.go"}); problem == nil || problem.Code != "unsupported_configuration" {
		t.Fatalf("module replace conflict accepted: %+v", problem)
	}
}

// TestStandardMethodIdentity 验证同包不同 receiver 的 SDK 方法不会混为一个 External。
func TestStandardMethodIdentity(t *testing.T) {
	root := t.TempDir()
	writeFixture(t, root, "go.mod", "module example.test/app\ngo 1.23.0\n")
	writeFixture(t, root, "unit.go", "package app\nimport \"sync\"\nfunc Run(m *sync.Mutex,r *sync.RWMutex){ m.Lock(); r.Lock() }\n")
	req := fixtureRequest(t, root, "unit.go")
	req.Environment.Platforms = req.Environment.Platforms[2:]
	response := analyze(context.Background(), req)
	if response.Outcome != "ok" {
		t.Fatalf("SDK: %+v", response.Error)
	}
	run := findNode(response.Graph, "go:unit.go#func:Run")
	seen := map[string]bool{}
	for _, ref := range run.References {
		if ref.Semantic == "code.calls" {
			seen[ref.Target.Symbol] = true
		}
	}
	if len(seen) != 2 {
		t.Fatalf("SDK method identities collapsed: %v", seen)
	}
}

// TestTypeUsesAndGenericSelector 验证跨文件字段类型、嵌入类型和跨包泛型调用。
func TestTypeUsesAndGenericSelector(t *testing.T) {
	root := t.TempDir()
	writeFixture(t, root, "go.work", "go 1.23.0\nuse (\n ./app\n ./lib\n)\n")
	writeFixture(t, root, "app/go.mod", "module example.test/app\ngo 1.23.0\n")
	writeFixture(t, root, "lib/go.mod", "module example.test/lib\ngo 1.23.0\n")
	writeFixture(t, root, "lib/lib.go", "package lib\nfunc Id[T any](x T)T{return x}\n")
	writeFixture(t, root, "app/other.go", "package app\ntype Other struct{}\ntype J interface{ M() }\n")
	writeFixture(t, root, "app/use.go", "package app\nimport \"example.test/lib\"\ntype S struct{ Value Other; *Other }\ntype I interface{ J }\nfunc Run(){ _=lib.Id[int](1) }\n")
	req := fixtureRequest(t, root, "app/use.go")
	req.Environment.Platforms = req.Environment.Platforms[2:]
	response := analyze(context.Background(), req)
	if response.Outcome != "ok" {
		t.Fatalf("analysis: %+v", response.Error)
	}
	field := findNode(response.Graph, "go:app/use.go#field:S.Value")
	if field == nil || len(field.References) == 0 || field.References[0].Target.Symbol != "go:app/other.go#type:Other" {
		t.Fatalf("field type relation missing: %+v", field)
	}
	iface := findNode(response.Graph, "go:app/use.go#type:I")
	if iface == nil || len(iface.References) == 0 || iface.References[0].Target.Symbol != "go:app/other.go#type:J" {
		t.Fatalf("interface embedding relation missing: %+v", iface)
	}
	run := findNode(response.Graph, "go:app/use.go#func:Run")
	if run == nil || len(run.References) == 0 || run.References[0].Target.Symbol != "go:lib/lib.go#func:Id" {
		t.Fatalf("generic selector call unresolved: %+v", run)
	}
}

// TestTimeoutOutcome 验证到期前后均以 resource_limit 原子终结。
func TestTimeoutOutcome(t *testing.T) {
	root := t.TempDir()
	writeFixture(t, root, "go.mod", "module example.test/app\ngo 1.23.0\n")
	writeFixture(t, root, "unit.go", "package app\nimport \"strings\"\nfunc F(){_ = strings.Contains(\"a\",\"b\")}\n")
	req := fixtureRequest(t, root, "unit.go")
	req.Environment.Platforms = req.Environment.Platforms[2:]
	ctx, cancel := context.WithDeadline(context.Background(), time.Now().Add(-time.Second))
	defer cancel()
	response := analyze(ctx, req)
	if response.Error == nil || response.Error.Code != "resource_limit" {
		t.Fatalf("expired deadline: %+v", response)
	}
	req.Limits.AnalysisTimeoutMS = 1
	response = analyze(context.Background(), req)
	if response.Error == nil || response.Error.Code != "resource_limit" {
		t.Fatalf("local timeout: %+v", response)
	}
}

// TestNestedModuleBoundary 验证未 use 的嵌套 module 不能按外层包导入或选中。
func TestNestedModuleBoundary(t *testing.T) {
	root := t.TempDir()
	writeFixture(t, root, "go.work", "go 1.23.0\nuse ./app\n")
	writeFixture(t, root, "app/go.mod", "module example.test/app\ngo 1.23.0\n")
	writeFixture(t, root, "app/main.go", "package app\nimport \"example.test/app/nested\"\nfunc Run(){nested.F()}\n")
	writeFixture(t, root, "app/nested/go.mod", "module example.test/nested\ngo 1.23.0\n")
	writeFixture(t, root, "app/nested/n.go", "package nested\nfunc F(){}\n")
	req := fixtureRequest(t, root, "app/main.go")
	req.Environment.Platforms = req.Environment.Platforms[2:]
	response := analyze(context.Background(), req)
	if response.Error == nil || response.Error.Code != "unsupported_configuration" {
		t.Fatalf("nested import crossed workspace use: %+v", response)
	}
	req.Input.Files = []string{"app/nested/n.go"}
	response = analyze(context.Background(), req)
	if response.Error == nil || response.Error.Code != "unsupported_configuration" {
		t.Fatalf("nested selected file admitted: %+v", response)
	}
}

// TestProtocolCancelWhileRunning 验证匹配 ID 取消、错 ID 拒绝与入口自行退出。
func TestProtocolCancelWhileRunning(t *testing.T) {
	for _, matching := range []bool{true, false} {
		t.Run(map[bool]string{true: "matching", false: "wrong_id"}[matching], func(t *testing.T) {
			root := t.TempDir()
			writeFixture(t, root, "go.mod", "module example.test/app\ngo 1.23.0\n")
			writeFixture(t, root, "unit.go", "package app\nimport \"strings\"\nfunc F(){_ = strings.Contains(\"a\",\"b\")}\n")
			req := fixtureRequest(t, root, "unit.go")
			req.Environment.Platforms = req.Environment.Platforms[2:]
			read, write := io.Pipe()
			defer write.Close()
			output := &strings.Builder{}
			done := make(chan error, 1)
			go func() { done <- runProtocol(read, output) }()
			payload, _ := json.Marshal(req)
			if _, err := write.Write(append(payload, '\n')); err != nil {
				t.Fatal(err)
			}
			cancelID := req.ID
			if !matching {
				cancelID = "wrong"
			}
			control, _ := json.Marshal(map[string]any{"protocol": protocol, "type": "cancel", "id": cancelID})
			if _, err := write.Write(append(control, '\n')); err != nil {
				t.Fatal(err)
			}
			select {
			case err := <-done:
				if matching {
					if err != nil {
						t.Fatal(err)
					}
					if !strings.Contains(output.String(), `"outcome":"cancelled"`) {
						t.Fatalf("missing cancelled terminal: %s", output.String())
					}
				} else if err == nil {
					t.Fatalf("wrong ID accepted: %s", output.String())
				}
			case <-time.After(5 * time.Second):
				t.Fatal("cancel did not stop helper")
			}
		})
	}
}
