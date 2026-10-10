// 本探针只解析临时源码并做类型检查，不执行样例程序，也不调用项目加载命令。
package main

import (
	"encoding/json"
	"fmt"
	"go/ast"
	"go/build"
	"go/parser"
	"go/token"
	"go/types"
	"os"
	"path"
	"path/filepath"
	"sort"
	"strings"
)

// call 保存一个语法调用及其有证据的目标，未知目标不伪装成外部符号。
type call struct {
	Source     string `json:"source"`
	Spelling   string `json:"spelling"`
	Target     string `json:"target"`
	Resolution string `json:"resolution"`
}

// result 保存单个明确平台的探针输出，尚不是生产 CodeGraph DTO。
type result struct {
	Platform         string   `json:"platform"`
	Files            []string `json:"files"`
	Calls            []call   `json:"calls"`
	Diagnostics      []string `json:"diagnostics"`
	StandardPackages []string `json:"standard_packages"`
	RejectedImports  []string `json:"rejected_imports"`
}

// sourceLoader 限定包来源为显式仓库映射和指定 GOROOT 中的源码。
type sourceLoader struct {
	fixture  string
	sdk      string
	context  build.Context
	fset     *token.FileSet
	cache    map[string]*types.Package
	loading  map[string]bool
	standard map[string]bool
	rejected map[string]bool
}

// within 拒绝符号链接把允许的目录或文件引向边界之外。
func within(root, candidate string) error {
	resolvedRoot, err := filepath.EvalSymlinks(root)
	if err != nil {
		return err
	}
	resolved, err := filepath.EvalSymlinks(candidate)
	if err != nil {
		return err
	}
	rel, err := filepath.Rel(resolvedRoot, resolved)
	if err != nil || rel == ".." || strings.HasPrefix(rel, ".."+string(filepath.Separator)) {
		return fmt.Errorf("source escapes allowed root")
	}
	return nil
}

// Import 只加载显式仓库包或目标标准库，不查询模块缓存、网络或外部工具。
func (loader *sourceLoader) Import(importPath string) (*types.Package, error) {
	if importPath == "unsafe" {
		return types.Unsafe, nil
	}
	if pkg, ok := loader.cache[importPath]; ok {
		return pkg, nil
	}
	if loader.loading[importPath] {
		return nil, fmt.Errorf("import cycle: %s", importPath)
	}
	var directory, allowedRoot string
	if importPath == "example.test/app/local" {
		directory = filepath.Join(loader.fixture, "app", "local")
		allowedRoot = loader.fixture
	} else {
		first := strings.Split(importPath, "/")[0]
		if importPath == "C" || first == "cmd" || strings.Contains(first, ".") ||
			path.Clean(importPath) != importPath || strings.HasPrefix(importPath, "/") ||
			strings.Contains(importPath, "\\") {
			loader.rejected[importPath] = true
			return nil, fmt.Errorf("dependency outside repository/stdlib policy: %s", importPath)
		}
		allowedRoot = filepath.Join(loader.sdk, "src")
		directory = filepath.Join(allowedRoot, filepath.FromSlash(importPath))
		if err := within(allowedRoot, directory); err != nil {
			loader.rejected[importPath] = true
			return nil, fmt.Errorf("no admitted standard-library source for %s", importPath)
		}
		loader.standard[importPath] = true
	}
	if err := within(allowedRoot, directory); err != nil {
		return nil, err
	}
	loader.loading[importPath] = true
	defer delete(loader.loading, importPath)
	files, _, err := loader.parse(directory, allowedRoot)
	if err != nil {
		return nil, err
	}
	// 标准库只检查声明所需信息；本探针没有验证标准库函数体。
	configuration := types.Config{Importer: loader, Sizes: types.SizesFor("gc", loader.context.GOARCH), GoVersion: "go1.23", IgnoreFuncBodies: true}
	pkg, err := configuration.Check(importPath, loader.fset, files, nil)
	if err != nil {
		return nil, err
	}
	loader.cache[importPath] = pkg
	return pkg, nil
}

// parse 用标准库的文件选择规则读取一个允许目录，不执行 go list 或构建脚本。
func (loader *sourceLoader) parse(directory, allowedRoot string) ([]*ast.File, []string, error) {
	// IgnoreVendor 同时让此版本 go/build 的 importGo 直接拒绝命令回退。
	metadata, err := loader.context.ImportDir(directory, build.IgnoreVendor)
	if err != nil {
		return nil, nil, err
	}
	var files []*ast.File
	for _, name := range metadata.GoFiles {
		filename := filepath.Join(directory, name)
		if err := within(allowedRoot, filename); err != nil {
			return nil, nil, err
		}
		file, err := parser.ParseFile(loader.fset, filename, nil, parser.ParseComments|parser.SkipObjectResolution)
		if err != nil {
			return nil, nil, err
		}
		files = append(files, file)
	}
	return files, metadata.GoFiles, nil
}

// identity 给实验声明建立源码身份，外部标准库符号使用包路径和名称。
func (loader *sourceLoader) identity(object types.Object) string {
	if object == nil {
		return ""
	}
	position := loader.fset.PositionFor(object.Pos(), false)
	rel, err := filepath.Rel(loader.fixture, position.Filename)
	if err == nil && rel != ".." && !strings.HasPrefix(rel, ".."+string(filepath.Separator)) {
		return "repo:" + filepath.ToSlash(rel) + "#" + object.Name()
	}
	if object.Pkg() != nil {
		return "stdlib:" + object.Pkg().Path() + "#" + object.Name()
	}
	return "builtin:" + object.Name()
}

// normalize 去除随机临时根目录，便于复跑比较输出。
func (loader *sourceLoader) normalize(message string) string {
	message = strings.ReplaceAll(message, loader.fixture, "<fixture>")
	return strings.ReplaceAll(message, loader.sdk, "<goroot>")
}

// keys 返回稳定排序的集合元素。
func keys(values map[string]bool) []string {
	items := make([]string, 0, len(values))
	for value := range values {
		items = append(items, value)
	}
	sort.Strings(items)
	return items
}

// analyze 独立检查一个平台，允许缺依赖诊断但不假装未知调用已被解析。
func analyze(fixture, sdk, platform string) (result, error) {
	context := build.Default
	context.GOROOT, context.GOPATH = sdk, ""
	context.GOOS, context.GOARCH, context.CgoEnabled = platform, "amd64", false
	context.BuildTags = nil
	// 固定本实验 Go 1.23 默认实验标记和 amd64/v1，不继承宿主 arm64 的特征标签。
	context.ToolTags = []string{"goexperiment.regabiwrappers", "goexperiment.regabiargs", "goexperiment.coverageredesign", "amd64.v1"}
	loader := &sourceLoader{fixture: fixture, sdk: sdk, context: context, fset: token.NewFileSet(), cache: map[string]*types.Package{}, loading: map[string]bool{}, standard: map[string]bool{}, rejected: map[string]bool{}}
	files, names, err := loader.parse(filepath.Join(fixture, "app"), fixture)
	if err != nil {
		return result{}, err
	}
	output := result{Platform: platform + "/amd64", Files: names, Calls: []call{}, Diagnostics: []string{}}
	info := &types.Info{Defs: map[*ast.Ident]types.Object{}, Uses: map[*ast.Ident]types.Object{}, Types: map[ast.Expr]types.TypeAndValue{}, Selections: map[*ast.SelectorExpr]*types.Selection{}, Instances: map[*ast.Ident]types.Instance{}}
	configuration := types.Config{Importer: loader, Sizes: types.SizesFor("gc", "amd64"), GoVersion: "go1.23", Error: func(err error) { output.Diagnostics = append(output.Diagnostics, loader.normalize(err.Error())) }}
	_, _ = configuration.Check("example.test/app", loader.fset, files, info)
	for _, file := range files {
		for _, declaration := range file.Decls {
			function, ok := declaration.(*ast.FuncDecl)
			if !ok {
				continue
			}
			source := loader.identity(info.Defs[function.Name])
			ast.Inspect(function.Body, func(node ast.Node) bool {
				invocation, ok := node.(*ast.CallExpr)
				if !ok {
					return true
				}
				var identifier *ast.Ident
				var spelling string
				switch expression := invocation.Fun.(type) {
				case *ast.Ident:
					identifier, spelling = expression, expression.Name
				case *ast.SelectorExpr:
					identifier, spelling = expression.Sel, expression.Sel.Name
					if prefix, ok := expression.X.(*ast.Ident); ok {
						spelling = prefix.Name + "." + spelling
					}
				default:
					return true
				}
				observation := call{Source: source, Spelling: spelling, Resolution: "Unresolved"}
				if target, ok := info.Uses[identifier].(*types.Func); ok {
					observation.Target, observation.Resolution = loader.identity(target), "Resolved"
				}
				output.Calls = append(output.Calls, observation)
				return true
			})
		}
	}
	output.StandardPackages, output.RejectedImports = keys(loader.standard), keys(loader.rejected)
	return output, nil
}

// main 对两个显式平台执行独立分析，仅输出可观察证据。
func main() {
	if len(os.Args) != 3 {
		panic("usage: probe FIXTURE GOROOT")
	}
	var outputs []result
	for _, platform := range []string{"linux", "windows"} {
		output, err := analyze(os.Args[1], os.Args[2], platform)
		if err != nil {
			panic(err)
		}
		outputs = append(outputs, output)
	}
	encoder := json.NewEncoder(os.Stdout)
	encoder.SetIndent("", "  ")
	if err := encoder.Encode(outputs); err != nil {
		panic(err)
	}
}
