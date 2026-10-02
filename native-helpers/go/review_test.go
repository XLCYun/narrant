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
