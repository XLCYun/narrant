package main

import (
	"bytes"
	"context"
	"encoding/json"
	"io"
	"io/fs"
	"os"
	"path/filepath"
	"reflect"
	"runtime"
	"strings"
	"testing"
	"time"
)

// snapshotFixture 记录临时仓库每个路径和文件字节，以验证分析阶段只读。
func snapshotFixture(t *testing.T, root string) map[string]string {
	t.Helper()
	snapshot := map[string]string{}
	err := filepath.WalkDir(root, func(filename string, entry fs.DirEntry, walkErr error) error {
		if walkErr != nil {
			return walkErr
		}
		rel, _ := filepath.Rel(root, filename)
		if entry.IsDir() {
			snapshot[rel] = "<dir>"
			return nil
		}
		data, err := os.ReadFile(filename)
		if err != nil {
			return err
		}
		snapshot[rel] = string(data)
		return nil
	})
	if err != nil {
		t.Fatal(err)
	}
	return snapshot
}

// writeFixture 在测试临时目录写入自建源码，不接触真实项目。
func writeFixture(t *testing.T, root, relative, content string) {
	t.Helper()
	filename := filepath.Join(root, filepath.FromSlash(relative))
	if err := os.MkdirAll(filepath.Dir(filename), 0755); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filename, []byte(content), 0644); err != nil {
		t.Fatal(err)
	}
}

// fixtureRequest 为已安装的固定 Go SDK 构造协议请求。
func fixtureRequest(t *testing.T, root string, files ...string) request {
	t.Helper()
	sdk := runtime.GOROOT()
	hostMeta := platformMetadata{GOOS: runtime.GOOS, GOARCH: runtime.GOARCH}
	if runtime.GOARCH == "amd64" {
		hostMeta.GOAMD64 = "v1"
	}
	if !strings.HasSuffix(sdk, "go") {
		t.Logf("SDK: %s", sdk)
	}
	return request{Protocol: protocol, Type: "analyze", ID: "test", Input: input{RepositoryRoot: root, Files: files, Profile: "go-static-v1"}, Environment: environment{ScratchRoot: t.TempDir(), Platforms: []platform{
		{ID: "linux", Kind: "go-sdk", Version: "1.23.8", Root: sdk, Fingerprint: "test-sdk", Metadata: platformMetadata{GOOS: "linux", GOARCH: "amd64", GOAMD64: "v1"}},
		{ID: "windows", Kind: "go-sdk", Version: "1.23.8", Root: sdk, Fingerprint: "test-sdk", Metadata: platformMetadata{GOOS: "windows", GOARCH: "amd64", GOAMD64: "v1"}},
		{ID: "host", Kind: "go-sdk", Version: "1.23.8", Root: sdk, Fingerprint: "test-sdk", Metadata: hostMeta},
	}}, Limits: limits{AnalysisTimeoutMS: 120000, MaxResultBytes: 134217728, MaxDiagnosticBytes: 262144}}
}

// findNode 在已完成图中按 ID 搜索测试对象。
func findNode(g *graph, id string) *node {
	if g == nil {
		return nil
	}
	for i := range g.Nodes {
		if g.Nodes[i].ID == id {
			return &g.Nodes[i]
		}
	}
	return nil
}

// TestWorkspaceUnion 验证真实 workspace、两目标并集与仓内/标准库/缺依赖关系。
func TestWorkspaceUnion(t *testing.T) {
	root := t.TempDir()
	writeFixture(t, root, "go.work", "go 1.23.0\nuse (\n ./app\n ./lib\n)\n")
	writeFixture(t, root, "app/go.mod", "module example.test/app\ngo 1.23.0\nrequire example.test/lib v0.0.0\n")
	writeFixture(t, root, "lib/go.mod", "module example.test/lib\ngo 1.23.0\n")
	writeFixture(t, root, "lib/lib.go", "package lib\nfunc Local() { Hidden() }\nfunc Hidden(){}\n")
	writeFixture(t, root, "app/common.go", "package app\nimport (\"example.test/lib\";\"strings\";\"example.test/missing\")\nfunc Run() { platform(); lib.Local(); _ = strings.Contains(\"x\",\"y\"); missing.Remote() }\n")
	writeFixture(t, root, "app/platform_linux.go", "//go:build linux\n\npackage app\nfunc platform(){ LinuxTarget() }\nfunc LinuxTarget(){}\n")
	writeFixture(t, root, "app/platform_windows.go", "//go:build windows\n\npackage app\nfunc platform(){ WindowsTarget() }\nfunc WindowsTarget(){}\n")
	req := fixtureRequest(t, root, "app/common.go", "app/platform_linux.go", "app/platform_windows.go")
	// 此夹具只验证 Linux/Windows 两个目标；Windows 宿主的 host 条目会与 windows 重合。
	req.Environment.Platforms = req.Environment.Platforms[:2]
	before := snapshotFixture(t, root)
	response := analyze(context.Background(), req)
	if response.Outcome != "ok" {
		t.Fatalf("unexpected response: %+v", response.Error)
	}
	run := findNode(response.Graph, "go:app/common.go#func:Run")
	if run == nil {
		t.Fatalf("missing Run: %+v", response.Graph.Nodes)
	}
	var targets []string
	for _, ref := range run.References {
		if ref.Semantic == "code.calls" {
			targets = append(targets, ref.Target.ID+ref.Target.Symbol+ref.Target.Name)
		}
	}
	for _, want := range []string{"platform_linux.go", "platform_windows.go", "lib.go", "strings#Contains", "missing.Remote"} {
		if !strings.Contains(strings.Join(targets, "|"), want) {
			t.Errorf("missing target %s in %v", want, targets)
		}
	}
	if len(response.Evidence.Configurations) != 2 {
		t.Fatalf("expected two configurations, got %+v", response.Evidence.Configurations)
	}
	for _, item := range response.Graph.ExternalSymbols {
		if item.Scope != "repository" && item.Scope != "dependency" {
			t.Errorf("invalid scope: %+v", item)
		}
	}
	req.Input.Files = append(req.Input.Files, "lib/lib.go")
	withDependencySelected := analyze(context.Background(), req)
	if withDependencySelected.Outcome != "ok" {
		t.Fatalf("selected dependency: %+v", withDependencySelected.Error)
	}
	local := findNode(withDependencySelected.Graph, "go:lib/lib.go#func:Local")
	if local == nil || len(local.References) == 0 || local.References[0].Target.ID != "go:lib/lib.go#func:Hidden" {
		t.Fatalf("cached dependency body or node membership lost: %+v", local)
	}
	after := snapshotFixture(t, root)
	if !reflect.DeepEqual(before, after) {
		t.Fatalf("analysis changed repository paths or bytes: before=%v after=%v", before, after)
	}
}

// TestBodyProjection 验证代码正文归属、子声明隔离和注释排版不产生自身变更。
func TestBodyProjection(t *testing.T) {
	root := t.TempDir()
	writeFixture(t, root, "go.mod", "module example.test/app\ngo 1.23.0\n")
	writeFixture(t, root, "unit.go", "package app\ntype T struct { Field int }\nfunc F() int { return 1 }\n")
	req := fixtureRequest(t, root, "unit.go")
	req.Environment.Platforms = req.Environment.Platforms[2:]
	first := analyze(context.Background(), req)
	if first.Outcome != "ok" {
		t.Fatalf("first: %+v", first.Error)
	}
	writeFixture(t, root, "unit.go", "package app\n// comment\ntype T struct { Field int }\nfunc F() int { return 2 }\n")
	second := analyze(context.Background(), req)
	if second.Outcome != "ok" {
		t.Fatalf("second: %+v", second.Error)
	}
	a, b := findNode(first.Graph, "go:unit.go#func:F"), findNode(second.Graph, "go:unit.go#func:F")
	if a == nil || b == nil || *a.Code == *b.Code {
		t.Fatal("body change was lost")
	}
	writeFixture(t, root, "unit.go", "package app\n// comment\ntype T struct { Field int }\nfunc F() int {\n return 2\n}\n")
	formatted := analyze(context.Background(), req)
	if formatted.Outcome != "ok" {
		t.Fatalf("format: %+v", formatted.Error)
	}
	formattedF := findNode(formatted.Graph, "go:unit.go#func:F")
	if formattedF == nil || *formattedF.Code != *b.Code {
		t.Fatalf("pure body formatting changed code: %q vs %q", *formattedF.Code, *b.Code)
	}
	ta, tb := findNode(first.Graph, "go:unit.go#type:T"), findNode(second.Graph, "go:unit.go#type:T")
	if ta == nil || tb == nil || *ta.Signature != *tb.Signature {
		t.Fatal("type own content changed for unrelated comment/body")
	}
	writeFixture(t, root, "unit.go", "package app\ntype T struct { Other int }\nfunc F() int { return 2 }\n")
	third := analyze(context.Background(), req)
	if third.Outcome != "ok" {
		t.Fatalf("third: %+v", third.Error)
	}
	tc := findNode(third.Graph, "go:unit.go#type:T")
	if tc == nil || *tb.Signature != *tc.Signature {
		t.Fatal("child field changed parent own declaration")
	}
	if strings.Join(tb.Contains, "|") == strings.Join(tc.Contains, "|") {
		t.Fatal("field child membership failed to change")
	}
}

// TestInvalidSelection 验证重复选集和未知标签以明确错误终结。
func TestInvalidSelection(t *testing.T) {
	root := t.TempDir()
	writeFixture(t, root, "go.mod", "module example.test/app\ngo 1.23.0\n")
	writeFixture(t, root, "unit.go", "package app\n")
	req := fixtureRequest(t, root, "unit.go", "unit.go")
	if got := analyze(context.Background(), req); got.Error == nil || got.Error.Code != "invalid_input" {
		t.Fatalf("duplicate: %+v", got)
	}
	writeFixture(t, root, "unit.go", "//go:build proprietary\npackage app\n")
	req = fixtureRequest(t, root, "unit.go")
	if got := analyze(context.Background(), req); got.Error == nil || got.Error.Code != "unsupported_configuration" {
		t.Fatalf("tag: %+v", got)
	}
}

// TestProtocolCompletion 验证进程协议在一次终结后不等待 stdin EOF。
func TestProtocolCompletion(t *testing.T) {
	root := t.TempDir()
	writeFixture(t, root, "go.mod", "module example.test/app\ngo 1.23.0\n")
	writeFixture(t, root, "unit.go", "package app\nfunc F(){}\n")
	req := fixtureRequest(t, root, "unit.go")
	req.Environment.Platforms = req.Environment.Platforms[2:]
	read, write := io.Pipe()
	defer write.Close()
	var output bytes.Buffer
	finished := make(chan error, 1)
	go func() { finished <- runProtocol(read, &output) }()
	encoded, err := json.Marshal(req)
	if err != nil {
		t.Fatal(err)
	}
	if _, err := write.Write(append(encoded, '\n')); err != nil {
		t.Fatal(err)
	}
	select {
	case err := <-finished:
		if err != nil {
			t.Fatal(err)
		}
	case <-time.After(5 * time.Second):
		t.Fatal("helper waited for stdin EOF after completed")
	}
	lines := bytes.Split(bytes.TrimSpace(output.Bytes()), []byte{'\n'})
	if len(lines) != 2 {
		t.Fatalf("expected ready and one completed: %s", output.String())
	}
	var response completed
	if err := json.Unmarshal(lines[1], &response); err != nil {
		t.Fatal(err)
	}
	if response.Outcome != "ok" {
		t.Fatalf("unexpected completed: %+v", response)
	}
}

// TestStrictProtocol 验证递归重复键和未知字段均被拒绝。
func TestStrictProtocol(t *testing.T) {
	for _, raw := range []string{`{"x":1,"x":2}`, `{"protocol":"narrant.native-analyzer/1","type":"cancel","id":"x","extra":1}`} {
		var value struct {
			Protocol string `json:"protocol"`
			Type     string `json:"type"`
			ID       string `json:"id"`
		}
		if err := strictJSON([]byte(raw), &value); err == nil {
			t.Fatalf("accepted invalid protocol: %s", raw)
		}
	}
}

// TestCancellation 验证取消上下文不会发布已完成图。
func TestCancellation(t *testing.T) {
	root := t.TempDir()
	writeFixture(t, root, "go.mod", "module example.test/app\ngo 1.23.0\n")
	writeFixture(t, root, "unit.go", "package app\nfunc F(){}\n")
	req := fixtureRequest(t, root, "unit.go")
	req.Environment.Platforms = req.Environment.Platforms[2:]
	ctx, cancel := context.WithCancel(context.Background())
	cancel()
	response := analyze(ctx, req)
	if response.Outcome != "cancelled" || response.Graph != nil {
		t.Fatalf("cancel: %+v", response)
	}
}

// TestInterfaceAndInitializer 验证接口静态槽位、未知动态实现及共享初始化归属。
func TestInterfaceAndInitializer(t *testing.T) {
	root := t.TempDir()
	writeFixture(t, root, "go.mod", "module example.test/app\ngo 1.23.0\n")
	writeFixture(t, root, "unit.go", "package app\ntype I interface { M() }\nfunc Call(x I){ x.M() }\nfunc Build() (int,int) { return 1,2 }\nvar A,B = Build()\nfunc Id[T any](x T) T { return x }\nfunc Use(){ _ = Id[int](A) }\n")
	req := fixtureRequest(t, root, "unit.go")
	req.Environment.Platforms = req.Environment.Platforms[2:]
	response := analyze(context.Background(), req)
	if response.Outcome != "ok" {
		t.Fatalf("analysis: %+v", response.Error)
	}
	call := findNode(response.Graph, "go:unit.go#func:Call")
	if call == nil {
		t.Fatal("missing Call")
	}
	static, dynamic := false, false
	for _, ref := range call.References {
		if ref.Semantic == "code.calls" && strings.Contains(ref.Target.ID, "interface_method:I.M") {
			static = true
		}
		if ref.Semantic == "go.dynamic_dispatch" && ref.Target.Kind == "unresolved" {
			dynamic = true
		}
	}
	if !static || !dynamic {
		t.Fatalf("interface evidence lost: %+v", call.References)
	}
	if findNode(response.Graph, "go:unit.go#initializer:A") == nil {
		t.Fatal("shared initializer execution region missing")
	}
	use := findNode(response.Graph, "go:unit.go#func:Use")
	if use == nil {
		t.Fatal("missing Use")
	}
	generic := false
	for _, ref := range use.References {
		if ref.Semantic == "code.calls" && ref.Target.ID == "go:unit.go#func:Id" {
			generic = true
		}
	}
	if !generic {
		t.Fatalf("generic call lost: %+v", use.References)
	}
}

// TestTestFileScope 验证默认测试排除和显式恢复都保留准确覆盖。
func TestTestFileScope(t *testing.T) {
	root := t.TempDir()
	writeFixture(t, root, "go.mod", "module example.test/app\ngo 1.23.0\n")
	writeFixture(t, root, "unit.go", "package app\nfunc Prod(){}\n")
	writeFixture(t, root, "unit_test.go", "package app\nfunc TestProd(){ Prod() }\n")
	req := fixtureRequest(t, root, "unit.go", "unit_test.go")
	req.Environment.Platforms = req.Environment.Platforms[2:]
	excluded := analyze(context.Background(), req)
	if excluded.Outcome != "ok" {
		t.Fatalf("exclude: %+v", excluded.Error)
	}
	if findNode(excluded.Graph, "go:unit_test.go#func:TestProd") != nil || excluded.Evidence.Files[1].Disposition != "excluded_test" {
		t.Fatalf("test exclusion failed: %+v", excluded.Evidence.Files)
	}
	req.Input.IncludeTests = true
	included := analyze(context.Background(), req)
	if included.Outcome != "ok" {
		t.Fatalf("include: %+v", included.Error)
	}
	if findNode(included.Graph, "go:unit_test.go#func:TestProd") == nil || included.Evidence.Files[1].Disposition != "analyzed" {
		t.Fatalf("test inclusion failed: %+v", included.Evidence.Files)
	}
}

// TestOutsideSymlink 验证仓内路径不能通过符号链接读取仓外源码。
func TestOutsideSymlink(t *testing.T) {
	root := t.TempDir()
	outside := t.TempDir()
	writeFixture(t, root, "go.mod", "module example.test/app\ngo 1.23.0\n")
	writeFixture(t, outside, "escape.go", "package app\nfunc Escape(){}\n")
	if err := os.Symlink(filepath.Join(outside, "escape.go"), filepath.Join(root, "escape.go")); err != nil {
		t.Fatal(err)
	}
	req := fixtureRequest(t, root, "escape.go")
	req.Environment.Platforms = req.Environment.Platforms[2:]
	response := analyze(context.Background(), req)
	if response.Error == nil || response.Error.Code != "invalid_input" {
		t.Fatalf("outside symlink accepted: %+v", response)
	}
}
