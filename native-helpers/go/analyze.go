package main

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"go/ast"
	"go/build"
	"go/parser"
	"go/scanner"
	"go/token"
	"go/types"
	"os"
	"path"
	"path/filepath"
	"sort"
	"strings"
	"time"
)

// fileUnit 保存一个原位解析文件及其真实路径。
type fileUnit struct {
	Rel   string
	Abs   string
	AST   *ast.File
	Bytes []byte
}

// packageUnit 保存一个配置下的包语法、类型证据和声明 ID。
type packageUnit struct {
	Path          string
	Files         []*fileUnit
	Info          *types.Info
	Package       *types.Package
	ObjectIDs     map[types.Object]string
	Missing       map[string]bool
	BodiesChecked bool
	TestsIncluded bool
}

// sourceLoader 只导入仓内 module 和目标 SDK 标准库源码。
type sourceLoader struct {
	Ctx       context.Context
	Project   *project
	SDK       platform
	Build     build.Context
	Fset      *token.FileSet
	Cache     map[string]*packageUnit
	Loading   map[string]bool
	ObjectIDs map[types.Object]string
	Fatal     *analysisError
}

// Import 满足 go/types.Importer，失败的第三方依赖永不查询缓存或网络。
func (l *sourceLoader) Import(importPath string) (*types.Package, error) {
	if importPath == "unsafe" {
		return types.Unsafe, nil
	}
	if l.Ctx.Err() != nil {
		return nil, l.Ctx.Err()
	}
	if item := l.Cache[importPath]; item != nil {
		return item.Package, nil
	}
	unit, err := l.load(importPath, false, false)
	if err != nil {
		return nil, err
	}
	return unit.Package, nil
}

// load 读取一个可证明来源的包，并递归检查必要声明。
func (l *sourceLoader) load(importPath string, checkBodies, includeTests bool) (loaded *packageUnit, loadErr error) {
	if unit := l.Cache[importPath]; unit != nil {
		if (!checkBodies || unit.BodiesChecked) && (!includeTests || unit.TestsIncluded) {
			return unit, nil
		}
		delete(l.Cache, importPath)
	}
	if l.Loading[importPath] {
		return nil, fmt.Errorf("import cycle: %s", importPath)
	}
	if path.Clean(importPath) != importPath || strings.HasPrefix(importPath, "/") || strings.Contains(importPath, "\\") || importPath == "C" {
		return nil, fmt.Errorf("rejected import %q", importPath)
	}
	dir, local, routeErr := l.Project.importDirectory(importPath)
	if routeErr != nil {
		l.Fatal = fail("unsupported_configuration", routeErr.Error())
		return nil, routeErr
	}
	// 仓内导入包也必须检查函数体中的语言门禁，标准库仍只检查所需声明。
	checkBodies = checkBodies || local
	allowed := l.Project.Root
	// 仓内已准入源码的读取或解析失败不能退化成缺少第三方绑定。
	defer func() {
		if local && loadErr != nil && l.Fatal == nil {
			l.Fatal = fail("object_extraction_failed", loadErr.Error())
		}
	}()
	if !local {
		first := strings.Split(importPath, "/")[0]
		if strings.Contains(first, ".") || first == "cmd" {
			return nil, fmt.Errorf("third-party dependency not admitted: %s", importPath)
		}
		allowed = filepath.Join(l.SDK.Root, "src")
		dir = filepath.Join(allowed, filepath.FromSlash(importPath))
	}
	if err := within(allowed, dir); err != nil {
		return nil, err
	}
	if local {
		entries, err := os.ReadDir(dir)
		if err != nil {
			return nil, err
		}
		var contextFiles []string
		for _, entry := range entries {
			if !strings.HasSuffix(entry.Name(), ".go") || (!includeTests && strings.HasSuffix(entry.Name(), "_test.go")) {
				continue
			}
			filename := filepath.Join(dir, entry.Name())
			if _, err := safeFile(allowed, filename); err != nil {
				return nil, err
			}
			rel, _ := filepath.Rel(l.Project.Root, filename)
			contextFiles = append(contextFiles, filepath.ToSlash(rel))
		}
		for _, rel := range contextFiles {
			if _, problem := hasTargetConstraint(l.Project.Root, rel); problem != nil {
				l.Fatal = problem
				return nil, fmt.Errorf("package %s: %s", importPath, problem.Message)
			}
		}
	}
	meta, err := l.Build.ImportDir(dir, build.IgnoreVendor)
	if err != nil {
		return nil, fmt.Errorf("package %s: %w", importPath, err)
	}
	names := append([]string{}, meta.GoFiles...)
	if includeTests {
		names = append(names, meta.TestGoFiles...)
	}
	sort.Strings(names)
	if len(names) == 0 {
		return nil, fmt.Errorf("package %s contains no selected Go files", importPath)
	}
	unit := &packageUnit{Path: importPath, Info: &types.Info{Defs: map[*ast.Ident]types.Object{}, Uses: map[*ast.Ident]types.Object{}, Types: map[ast.Expr]types.TypeAndValue{}, Selections: map[*ast.SelectorExpr]*types.Selection{}, Instances: map[*ast.Ident]types.Instance{}}, ObjectIDs: map[types.Object]string{}, Missing: map[string]bool{}, BodiesChecked: checkBodies, TestsIncluded: includeTests}
	for _, name := range names {
		filename := filepath.Join(dir, name)
		data, err := safeFile(allowed, filename)
		if err != nil {
			return nil, err
		}
		file, err := parser.ParseFile(l.Fset, filename, data, parser.ParseComments|parser.SkipObjectResolution)
		if err != nil {
			return nil, err
		}
		rel := ""
		if local {
			rel, _ = filepath.Rel(l.Project.Root, filename)
			rel = filepath.ToSlash(rel)
		}
		unit.Files = append(unit.Files, &fileUnit{Rel: rel, Abs: filename, AST: file, Bytes: data})
	}
	l.Loading[importPath] = true
	defer delete(l.Loading, importPath)
	astFiles := make([]*ast.File, 0, len(unit.Files))
	for _, f := range unit.Files {
		astFiles = append(astFiles, f.AST)
	}
	goVersion := "go" + l.SDK.Version
	if local {
		m, err := l.Project.moduleForPackage(dir)
		if err != nil {
			l.Fatal = fail("unsupported_configuration", err.Error())
			return nil, err
		}
		if problem := m.validateSDKVersion(l.SDK); problem != nil {
			l.Fatal = problem
			return nil, fmt.Errorf("%s", problem.Message)
		}
		goVersion = "go" + m.GoVersion
	}
	config := types.Config{Importer: l, Sizes: types.SizesFor("gc", l.SDK.Metadata.GOARCH), GoVersion: goVersion, IgnoreFuncBodies: !checkBodies, Error: func(err error) {
		unit.Missing[err.Error()] = true
		if diagnostic := languageVersionError(err); local && l.Fatal == nil && diagnostic != nil {
			source, _ := filepath.Rel(l.Project.Root, l.Fset.Position(diagnostic.Pos).Filename)
			l.Fatal = fail("object_extraction_failed", diagnostic.Msg, filepath.ToSlash(source))
		}
	}}
	pkg, checkErr := config.Check(importPath, l.Fset, astFiles, unit.Info)
	if local && l.Fatal == nil {
		l.Fatal = rejectDisabledAliasTypeParams(l.SDK, unit.Files)
	}
	if l.Fatal != nil {
		return nil, errors.New(l.Fatal.Message)
	}
	if pkg == nil {
		return nil, fmt.Errorf("package %s could not be checked: %w", importPath, checkErr)
	}
	unit.Package = pkg
	if !local && checkErr != nil {
		return nil, fmt.Errorf("target SDK package %s failed type check: %w", importPath, checkErr)
	}
	l.assignObjectIDs(unit)
	if l.ObjectIDs == nil {
		l.ObjectIDs = map[types.Object]string{}
	}
	for obj, id := range unit.ObjectIDs {
		l.ObjectIDs[obj] = id
	}
	l.Cache[importPath] = unit
	return unit, nil
}

// assignObjectIDs 为同包声明建立与绝对路径和代码行号无关的身份。
func (l *sourceLoader) assignObjectIDs(unit *packageUnit) {
	for _, f := range unit.Files {
		if f.Rel == "" {
			continue
		}
		occurrence := map[string]int{}
		for _, decl := range f.AST.Decls {
			switch d := decl.(type) {
			case *ast.FuncDecl:
				kind := "func"
				name := d.Name.Name
				if d.Recv != nil {
					kind = "method"
					name = receiverSpelling(l.Fset, f.Bytes, d.Recv) + "." + name
				}
				key := kind + ":" + name
				occurrence[key]++
				if obj := unit.Info.Defs[d.Name]; obj != nil {
					unit.ObjectIDs[obj] = sourceID(f.Rel, key, occurrence[key])
				}
			case *ast.GenDecl:
				for _, spec := range d.Specs {
					switch s := spec.(type) {
					case *ast.TypeSpec:
						key := "type:" + s.Name.Name
						occurrence[key]++
						if obj := unit.Info.Defs[s.Name]; obj != nil {
							unit.ObjectIDs[obj] = sourceID(f.Rel, key, occurrence[key])
						}
						if iface, ok := s.Type.(*ast.InterfaceType); ok {
							for _, member := range iface.Methods.List {
								for _, name := range member.Names {
									methodKey := "interface_method:" + s.Name.Name + "." + name.Name
									occurrence[methodKey]++
									if obj := unit.Info.Defs[name]; obj != nil {
										unit.ObjectIDs[obj] = sourceID(f.Rel, methodKey, occurrence[methodKey])
									}
								}
							}
						}
					case *ast.ValueSpec:
						for _, name := range s.Names {
							key := strings.ToLower(d.Tok.String()) + ":" + name.Name
							occurrence[key]++
							if obj := unit.Info.Defs[name]; obj != nil {
								unit.ObjectIDs[obj] = sourceID(f.Rel, key, occurrence[key])
							}
						}
					}
				}
			}
		}
	}
}

// sourceID 使用文件、声明种类、名称和同名槽位标识源码声明。
func sourceID(rel, key string, slot int) string {
	if slot == 1 {
		return "go:" + rel + "#" + key
	}
	return fmt.Sprintf("go:%s#%s~%d", rel, key, slot)
}

// receiverSpelling 只使用接收者类型建立方法身份，参数名仍属于签名内容。
func receiverSpelling(fset *token.FileSet, data []byte, fields *ast.FieldList) string {
	if fields == nil || len(fields.List) == 0 {
		return ""
	}
	return normalizedNode(fset, data, fields.List[0].Type)
}

// hasIota 只识别真正的 iota 标识符，避免字符串字面量误触发序号投影。
func hasIota(expression ast.Expr) bool {
	found := false
	ast.Inspect(expression, func(n ast.Node) bool {
		if ident, ok := n.(*ast.Ident); ok && ident.Name == "iota" {
			found = true
		}
		return !found
	})
	return found
}

// normalizedNode 通过 Go token 流保留字面量和操作符，忽略排版与普通注释。
func normalizedNode(fset *token.FileSet, data []byte, n ast.Node) string {
	if n == nil {
		return ""
	}
	file := fset.File(n.Pos())
	if file == nil {
		return ""
	}
	start, end := file.Offset(n.Pos()), file.Offset(n.End())
	if start < 0 || end > len(data) || start > end {
		return ""
	}
	var lex scanner.Scanner
	lex.Init(file, data, nil, 0)
	var pieces []string
	for {
		pos, tok, lit := lex.Scan()
		if tok == token.EOF {
			break
		}
		offset := file.Offset(pos)
		if offset < start {
			continue
		}
		if offset >= end {
			break
		}
		if tok == token.SEMICOLON {
			lit = ";"
		}
		if lit == "" {
			lit = tok.String()
		}
		pieces = append(pieces, lit)
	}
	stable := make([]string, 0, len(pieces))
	for i, piece := range pieces {
		if piece == ";" && i+1 < len(pieces) && pieces[i+1] == "}" {
			continue
		}
		stable = append(stable, piece)
	}
	return strings.Join(stable, " ")
}

// sourceNode 返回 AST 声明对应的原始源码文本。
func sourceNode(fset *token.FileSet, data []byte, n ast.Node) *string {
	if n == nil {
		return nil
	}
	file := fset.File(n.Pos())
	if file == nil {
		return nil
	}
	start, end := file.Offset(n.Pos()), file.Offset(n.End())
	if start < 0 || end > len(data) || start > end {
		return nil
	}
	return stringPtr(string(data[start:end]))
}

// graphBuilder 合并多配置对象、来源和已确认关系。
type graphBuilder struct {
	Nodes   map[string]*node
	Parent  map[string]string
	Origins map[string]map[string]bool
	RefSets map[string]map[string]reference
	Root    map[string]bool
}

// newGraphBuilder 初始化去重索引。
func newGraphBuilder() *graphBuilder {
	return &graphBuilder{Nodes: map[string]*node{}, Parent: map[string]string{}, Origins: map[string]map[string]bool{}, RefSets: map[string]map[string]reference{}, Root: map[string]bool{}}
}

// addNode 验证同一源码身份在各配置中的自身内容与父级兼容。
func (b *graphBuilder) addNode(n node, parent, origin string) *analysisError {
	if old := b.Nodes[n.ID]; old != nil {
		if !sameNodeContent(*old, n) || b.Parent[n.ID] != parent {
			return fail("union_conflict", "incompatible declaration across Go configurations", origin)
		}
	} else {
		copy := n
		b.Nodes[n.ID] = &copy
		b.Parent[n.ID] = parent
	}
	if b.Origins[n.ID] == nil {
		b.Origins[n.ID] = map[string]bool{}
	}
	b.Origins[n.ID][origin] = true
	if parent == "" {
		b.Root[n.ID] = true
	}
	return nil
}

// addReference 保留所有配置下真实观察到的关系事实。
func (b *graphBuilder) addReference(source, semantic string, t target) {
	if b.RefSets[source] == nil {
		b.RefSets[source] = map[string]reference{}
	}
	key := semantic + "\x00" + t.Kind + "\x00" + t.ID + "\x00" + t.Symbol + "\x00" + t.Name
	b.RefSets[source][key] = reference{Semantic: semantic, Target: t}
}

// objectTarget 将类型检查证据映射到源码身份或稳定的 SDK 符号。
func (l *sourceLoader) objectTarget(obj types.Object) target {
	if obj == nil {
		return target{Kind: "unresolved", Name: "unknown"}
	}
	if fn, ok := obj.(*types.Func); ok {
		obj = fn.Origin()
	}
	if id := l.ObjectIDs[obj]; id != "" {
		return target{Kind: "external", Symbol: id, Scope: "repository", Display: obj.Name()}
	}
	if obj.Pkg() != nil {
		symbol := obj.Name()
		if fn, ok := obj.(*types.Func); ok {
			if signature, ok := fn.Type().(*types.Signature); ok && signature.Recv() != nil {
				receiver := types.TypeString(signature.Recv().Type(), func(pkg *types.Package) string { return pkg.Path() })
				symbol = receiver + "." + symbol
			}
		}
		return target{Kind: "external", Symbol: "go:external:" + obj.Pkg().Path() + "#" + symbol, Scope: "dependency", Display: obj.Name()}
	}
	return target{Kind: "unresolved", Name: obj.Name()}
}

// callsIn 提取所属对象表达式中有类型证据的直接调用及动态未知观察。
func (l *sourceLoader) callsIn(b *graphBuilder, unit *packageUnit, owner string, tree ast.Node) {
	if tree == nil {
		return
	}
	ast.Inspect(tree, func(n ast.Node) bool {
		call, ok := n.(*ast.CallExpr)
		if !ok {
			return true
		}
		ident, selection, spelling := callTarget(call.Fun)
		if ident == nil {
			b.addReference(owner, "code.calls", target{Kind: "unresolved", Name: spelling})
			return true
		}
		obj := unit.Info.Uses[ident]
		if fn, ok := obj.(*types.Func); ok {
			t := l.objectTarget(fn)
			b.addReference(owner, "code.calls", t)
			if selection != nil {
				if sel := unit.Info.Selections[selection]; sel != nil && sel.Kind() == types.MethodVal {
					if _, isInterface := sel.Recv().Underlying().(*types.Interface); isInterface {
						b.addReference(owner, "go.dynamic_dispatch", target{Kind: "unresolved", Name: spelling})
					}
				}
			}
		} else if obj == nil || isFunctionValue(obj) {
			b.addReference(owner, "code.calls", target{Kind: "unresolved", Name: spelling})
		}
		return true
	})
}

// typeUses 仅从字段及显式嵌入等类型位置提取已确认类型目标。
func (l *sourceLoader) typeUses(b *graphBuilder, unit *packageUnit, owner string, expression ast.Expr) {
	if expression == nil {
		return
	}
	ast.Inspect(expression, func(n ast.Node) bool {
		if selector, ok := n.(*ast.SelectorExpr); ok && unit.Info.Uses[selector.Sel] == nil {
			name := selector.Sel.Name
			if prefix, ok := selector.X.(*ast.Ident); ok {
				name = prefix.Name + "." + name
			}
			b.addReference(owner, "code.type_use", target{Kind: "unresolved", Name: name})
			return false
		}
		ident, ok := n.(*ast.Ident)
		if !ok {
			return true
		}
		obj := unit.Info.Uses[ident]
		if obj == nil {
			b.addReference(owner, "code.type_use", target{Kind: "unresolved", Name: ident.Name})
			return true
		}
		typeName, ok := obj.(*types.TypeName)
		if !ok {
			return true
		}
		if typeName.Parent() == types.Universe {
			return true
		}
		if _, isParam := typeName.Type().(*types.TypeParam); isParam {
			return true
		}
		b.addReference(owner, "code.type_use", l.objectTarget(typeName))
		return true
	})
}

// callTarget 递归解开泛型实例化与括号，保留真实 selector 绑定。
func callTarget(expr ast.Expr) (*ast.Ident, *ast.SelectorExpr, string) {
	switch e := expr.(type) {
	case *ast.Ident:
		return e, nil, e.Name
	case *ast.SelectorExpr:
		name := e.Sel.Name
		if prefix, ok := e.X.(*ast.Ident); ok {
			name = prefix.Name + "." + name
		}
		return e.Sel, e, name
	case *ast.IndexExpr:
		return callTarget(e.X)
	case *ast.IndexListExpr:
		return callTarget(e.X)
	case *ast.ParenExpr:
		return callTarget(e.X)
	}
	return nil, nil, "call"
}

// isFunctionValue 区分函数值与类型转换等非直接调用。
func isFunctionValue(obj types.Object) bool {
	if v, ok := obj.(*types.Var); ok {
		_, yes := v.Type().Underlying().(*types.Signature)
		return yes
	}
	return false
}

// extractFile 把选中文件的显式声明归入六类公共节点。
func (l *sourceLoader) extractFile(b *graphBuilder, unit *packageUnit, f *fileUnit) *analysisError {
	fileID := "go:file:" + f.Rel
	pkgName := f.AST.Name.Name
	fileNode := node{Category: "container", ID: fileID, Language: "go", NativeKind: "file", Name: stringPtr(f.Rel), Signature: stringPtr("package " + pkgName), Source: sourceNode(l.Fset, f.Bytes, f.AST.Name), Contains: []string{}, References: []reference{}, Extensions: map[string]any{}}
	fileNode.Extensions["go.initialization_order"] = initializationOrder(f.AST)
	for _, line := range strings.Split(string(f.Bytes), "\n") {
		line = strings.TrimSpace(line)
		if strings.HasPrefix(line, "//go:build ") {
			fileNode.Code = stringPtr(line)
			break
		}
		if line != "" && !strings.HasPrefix(line, "//") {
			break
		}
	}
	if err := b.addNode(fileNode, "", f.Rel); err != nil {
		return err
	}
	occurrence := map[string]int{}
	initOrdinal := 0
	for _, decl := range f.AST.Decls {
		switch d := decl.(type) {
		case *ast.FuncDecl:
			kind := "func"
			name := d.Name.Name
			parent := fileID
			if d.Recv != nil {
				kind = "method"
				name = receiverSpelling(l.Fset, f.Bytes, d.Recv) + "." + name
			}
			if d.Name.Name == "init" {
				initOrdinal++
				name = fmt.Sprintf("init~%d", initOrdinal)
			}
			key := kind + ":" + name
			occurrence[key]++
			id := sourceID(f.Rel, key, occurrence[key])
			if obj := unit.Info.Defs[d.Name]; obj != nil && d.Name.Name != "init" {
				if mapped := unit.ObjectIDs[obj]; mapped != "" {
					id = mapped
				}
			}
			if d.Name.Name != "init" && unit.Info.Defs[d.Name] == nil {
				return fail("object_extraction_failed", "function declaration lacks type object", f.Rel)
			}
			sig := normalizedNode(l.Fset, f.Bytes, d)
			if d.Body != nil {
				sig = normalizedNode(l.Fset, f.Bytes, d.Type)
				if d.Recv != nil {
					sig = "func " + normalizedNode(l.Fset, f.Bytes, d.Recv) + " " + d.Name.Name + " " + sig
				} else {
					sig = "func " + d.Name.Name + " " + sig
				}
			}
			var code *string
			if d.Body != nil {
				code = stringPtr(normalizedNode(l.Fset, f.Bytes, d.Body))
			}
			n := node{Category: "callable", ID: id, Language: "go", NativeKind: kind, Name: stringPtr(d.Name.Name), Signature: stringPtr(sig), Code: code, Source: sourceNode(l.Fset, f.Bytes, d), Contains: []string{}, References: []reference{}, Extensions: map[string]any{}}
			if err := b.addNode(n, parent, f.Rel); err != nil {
				return err
			}
			l.callsIn(b, unit, id, d.Body)
			if d.Recv != nil {
				if receiver := receiverTypeObject(unit, d.Recv); receiver != nil {
					b.addReference(id, "code.member_of", l.objectTarget(receiver))
				}
			}
		case *ast.GenDecl:
			var inherited []ast.Expr
			constSlot := 0
			for _, spec := range d.Specs {
				switch s := spec.(type) {
				case *ast.ImportSpec:
					name := s.Path.Value
					key := "import:" + name
					occurrence[key]++
					id := sourceID(f.Rel, key, occurrence[key])
					n := node{Category: "extension", ID: id, Language: "go", NativeKind: "import", Name: stringPtr(name), Signature: stringPtr(normalizedNode(l.Fset, f.Bytes, s)), Source: sourceNode(l.Fset, f.Bytes, s), Contains: []string{}, References: []reference{}, Extensions: map[string]any{}}
					if err := b.addNode(n, fileID, f.Rel); err != nil {
						return err
					}
				case *ast.TypeSpec:
					key := "type:" + s.Name.Name
					occurrence[key]++
					id := sourceID(f.Rel, key, occurrence[key])
					if obj := unit.Info.Defs[s.Name]; obj == nil {
						return fail("object_extraction_failed", "type declaration lacks type object", f.Rel)
					} else if mapped := unit.ObjectIDs[obj]; mapped != "" {
						id = mapped
					}
					sig := normalizedNode(l.Fset, f.Bytes, s)
					if st, ok := s.Type.(*ast.StructType); ok {
						sig = "type " + s.Name.Name + " struct"
						if s.TypeParams != nil {
							sig += " " + normalizedNode(l.Fset, f.Bytes, s.TypeParams)
						}
						_ = st
					}
					if _, ok := s.Type.(*ast.InterfaceType); ok {
						sig = "type " + s.Name.Name + " interface"
						if s.TypeParams != nil {
							sig += " " + normalizedNode(l.Fset, f.Bytes, s.TypeParams)
						}
						iface := s.Type.(*ast.InterfaceType)
						for _, member := range iface.Methods.List {
							if len(member.Names) == 0 {
								sig += " embeds " + normalizedNode(l.Fset, f.Bytes, member.Type)
							}
						}
					}
					n := node{Category: "type", ID: id, Language: "go", NativeKind: typeKind(s), Name: stringPtr(s.Name.Name), Signature: stringPtr(sig), Source: sourceNode(l.Fset, f.Bytes, s), Contains: []string{}, References: []reference{}, Extensions: map[string]any{}}
					if st, ok := s.Type.(*ast.StructType); ok {
						fieldOrder := []string{}
						for _, field := range st.Fields.List {
							if len(field.Names) == 0 {
								fieldOrder = append(fieldOrder, normalizedNode(l.Fset, f.Bytes, field.Type))
							} else {
								for _, name := range field.Names {
									fieldOrder = append(fieldOrder, name.Name)
								}
							}
						}
						n.Extensions["go.field_order"] = fieldOrder
					}
					if err := b.addNode(n, fileID, f.Rel); err != nil {
						return err
					}
					if _, isStruct := s.Type.(*ast.StructType); !isStruct {
						if _, isInterface := s.Type.(*ast.InterfaceType); !isInterface {
							l.typeUses(b, unit, id, s.Type)
						}
					}
					if st, ok := s.Type.(*ast.StructType); ok {
						for _, field := range st.Fields.List {
							names := field.Names
							if len(names) == 0 {
								names = []*ast.Ident{{Name: normalizedNode(l.Fset, f.Bytes, field.Type)}}
							}
							for _, name := range names {
								fid := sourceID(f.Rel, "field:"+s.Name.Name+"."+name.Name, 1)
								signature := name.Name + " " + normalizedNode(l.Fset, f.Bytes, field.Type)
								if field.Tag != nil {
									signature += " " + normalizedNode(l.Fset, f.Bytes, field.Tag)
								}
								fn := node{Category: "value", ID: fid, Language: "go", NativeKind: "field", Name: stringPtr(name.Name), Signature: stringPtr(signature), Source: sourceNode(l.Fset, f.Bytes, field), Contains: []string{}, References: []reference{}, Extensions: map[string]any{}}
								if err := b.addNode(fn, id, f.Rel); err != nil {
									return err
								}
								l.typeUses(b, unit, fid, field.Type)
							}
						}
					}
					if iface, ok := s.Type.(*ast.InterfaceType); ok {
						for _, member := range iface.Methods.List {
							if len(member.Names) == 0 {
								l.typeUses(b, unit, id, member.Type)
							}
							for _, name := range member.Names {
								methodKey := "interface_method:" + s.Name.Name + "." + name.Name
								occurrence[methodKey]++
								methodID := sourceID(f.Rel, methodKey, occurrence[methodKey])
								method := node{Category: "callable", ID: methodID, Language: "go", NativeKind: "interface_method", Name: stringPtr(name.Name), Signature: stringPtr(normalizedNode(l.Fset, f.Bytes, member)), Source: sourceNode(l.Fset, f.Bytes, member), Contains: []string{}, References: []reference{}, Extensions: map[string]any{}}
								if problem := b.addNode(method, id, f.Rel); problem != nil {
									return problem
								}
							}
						}
					}
				case *ast.ValueSpec:
					effective := s.Values
					if d.Tok == token.CONST {
						if len(effective) == 0 {
							effective = inherited
						} else {
							inherited = effective
						}
						if len(effective) == 0 {
							return fail("object_extraction_failed", "constant has no inherited initializer", f.Rel)
						}
					}
					shared := len(s.Names) > 1 && len(effective) == 1
					var regionID string
					if shared {
						key := "initializer:" + normalizedNode(l.Fset, f.Bytes, s.Names[0])
						occurrence[key]++
						regionID = sourceID(f.Rel, key, occurrence[key])
						code := normalizedNode(l.Fset, f.Bytes, effective[0])
						if d.Tok == token.CONST && hasIota(effective[0]) {
							code += fmt.Sprintf(" @iota=%d", constSlot)
						}
						region := node{Category: "execution_region", ID: regionID, Language: "go", NativeKind: "shared_initializer", Name: nil, Signature: nil, Code: stringPtr(code), Source: sourceNode(l.Fset, f.Bytes, effective[0]), Contains: []string{}, References: []reference{}, Extensions: map[string]any{}}
						if err := b.addNode(region, fileID, f.Rel); err != nil {
							return err
						}
						l.callsIn(b, unit, regionID, effective[0])
					}
					for i, name := range s.Names {
						if name.Name == "_" {
							if d.Tok == token.VAR && !shared && i < len(effective) {
								key := "blank_initializer"
								occurrence[key]++
								id := sourceID(f.Rel, key, occurrence[key])
								region := node{Category: "execution_region", ID: id, Language: "go", NativeKind: "blank_initializer", Code: stringPtr(normalizedNode(l.Fset, f.Bytes, effective[i])), Source: sourceNode(l.Fset, f.Bytes, effective[i]), Contains: []string{}, References: []reference{}, Extensions: map[string]any{}}
								if problem := b.addNode(region, fileID, f.Rel); problem != nil {
									return problem
								}
								l.callsIn(b, unit, id, effective[i])
							}
							continue
						}
						key := strings.ToLower(d.Tok.String()) + ":" + name.Name
						occurrence[key]++
						id := sourceID(f.Rel, key, occurrence[key])
						if obj := unit.Info.Defs[name]; obj == nil {
							return fail("object_extraction_failed", "value declaration lacks type object", f.Rel)
						} else if mapped := unit.ObjectIDs[obj]; mapped != "" {
							id = mapped
						}
						var code *string
						if !shared && len(effective) > 0 {
							index := i
							if len(effective) == 1 {
								index = 0
							}
							if index < len(effective) {
								valueCode := normalizedNode(l.Fset, f.Bytes, effective[index])
								if d.Tok == token.CONST && hasIota(effective[index]) {
									valueCode += fmt.Sprintf(" @iota=%d", constSlot)
								}
								code = stringPtr(valueCode)
							}
						}
						signature := d.Tok.String() + " " + name.Name
						if s.Type != nil {
							signature += " " + normalizedNode(l.Fset, f.Bytes, s.Type)
						}
						n := node{Category: "value", ID: id, Language: "go", NativeKind: strings.ToLower(d.Tok.String()), Name: stringPtr(name.Name), Signature: stringPtr(signature), Code: code, Source: sourceNode(l.Fset, f.Bytes, s), Contains: []string{}, References: []reference{}, Extensions: map[string]any{}}
						if err := b.addNode(n, fileID, f.Rel); err != nil {
							return err
						}
						l.typeUses(b, unit, id, s.Type)
						if code != nil {
							index := i
							if len(effective) == 1 {
								index = 0
							}
							if index < len(effective) {
								l.callsIn(b, unit, id, effective[index])
							}
						}
					}
					if d.Tok == token.CONST {
						constSlot++
					}
				}
			}
		}
	}
	return nil
}

// initializationOrder 保留文件内显式包变量初始化和 init 声明的顺序。
func initializationOrder(file *ast.File) []string {
	order := []string{}
	for _, decl := range file.Decls {
		switch d := decl.(type) {
		case *ast.FuncDecl:
			if d.Name.Name == "init" {
				order = append(order, "init")
			}
		case *ast.GenDecl:
			if d.Tok != token.VAR {
				continue
			}
			for _, spec := range d.Specs {
				if value, ok := spec.(*ast.ValueSpec); ok && len(value.Values) > 0 {
					names := []string{}
					for _, name := range value.Names {
						names = append(names, name.Name)
					}
					order = append(order, strings.Join(names, ","))
				}
			}
		}
	}
	return order
}

// typeKind 区分 alias、struct、interface 和其他显式类型。
func typeKind(spec *ast.TypeSpec) string {
	if spec.Assign.IsValid() {
		return "type_alias"
	}
	switch spec.Type.(type) {
	case *ast.StructType:
		return "struct"
	case *ast.InterfaceType:
		return "interface"
	}
	return "type"
}

// receiverTypeObject 找到 receiver 的类型对象用于确定成员归属。
func receiverTypeObject(unit *packageUnit, field *ast.FieldList) types.Object {
	if field == nil || len(field.List) == 0 {
		return nil
	}
	expression := field.List[0].Type
	for {
		switch value := expression.(type) {
		case *ast.StarExpr:
			expression = value.X
		case *ast.ParenExpr:
			expression = value.X
		case *ast.IndexExpr:
			expression = value.X
		case *ast.IndexListExpr:
			expression = value.X
		case *ast.Ident:
			return unit.Info.Uses[value]
		default:
			return nil
		}
	}
}

// finish 对节点和关系做最终成员分类、排序与 evidence 汇总。
func (b *graphBuilder) finish(configs []configurationEvidence, files []fileEvidence) (graph, evidence) {
	ids := make([]string, 0, len(b.Nodes))
	for id := range b.Nodes {
		ids = append(ids, id)
	}
	sort.Strings(ids)
	result := graph{RootContains: []string{}, Nodes: []node{}, ExternalSymbols: []externalSymbol{}}
	ev := evidence{Configurations: configs, Files: files, Origins: []originEvidence{}}
	external := map[string]externalSymbol{}
	for _, id := range ids {
		n := *b.Nodes[id]
		n.Contains = []string{}
		n.References = []reference{}
		if b.Root[id] {
			result.RootContains = append(result.RootContains, id)
		}
		for child, parent := range b.Parent {
			if parent == id {
				n.Contains = append(n.Contains, child)
			}
		}
		sort.Strings(n.Contains)
		for _, r := range b.RefSets[id] {
			if r.Target.Kind == "external" {
				if _, ok := b.Nodes[r.Target.Symbol]; ok {
					r.Target = target{Kind: "node", ID: r.Target.Symbol}
				} else {
					external[r.Target.Symbol] = externalSymbol{Symbol: r.Target.Symbol, Name: r.Target.Display, Scope: r.Target.Scope}
					r.Target = target{Kind: "external", Symbol: r.Target.Symbol}
				}
			}
			n.References = append(n.References, r)
		}
		sort.Slice(n.References, func(i, j int) bool {
			a, c := n.References[i], n.References[j]
			return a.Semantic+a.Target.Kind+a.Target.ID+a.Target.Symbol+a.Target.Name < c.Semantic+c.Target.Kind+c.Target.ID+c.Target.Symbol+c.Target.Name
		})
		result.Nodes = append(result.Nodes, n)
		paths := []string{}
		for p := range b.Origins[id] {
			paths = append(paths, p)
		}
		sort.Strings(paths)
		ev.Origins = append(ev.Origins, originEvidence{NodeID: id, SelectedPaths: paths})
	}
	for _, item := range external {
		result.ExternalSymbols = append(result.ExternalSymbols, item)
	}
	sort.Slice(result.ExternalSymbols, func(i, j int) bool { return result.ExternalSymbols[i].Symbol < result.ExternalSymbols[j].Symbol })
	return result, ev
}

// analyze 校验输入、加载真实仓内项目并生成原子单图结果。
func analyze(ctx context.Context, req request) completed {
	base := completed{Protocol: protocol, Type: "completed", ID: req.ID}
	returnError := func(err *analysisError) completed { base.Outcome = "error"; base.Error = err; return base }
	if req.Input.Profile != "go-static-v1" || !filepath.IsAbs(req.Input.RepositoryRoot) || !filepath.IsAbs(req.Environment.ScratchRoot) || req.Limits.AnalysisTimeoutMS <= 0 || req.Limits.MaxResultBytes <= 0 || req.Limits.MaxDiagnosticBytes <= 0 {
		return returnError(fail("invalid_input", "invalid Go profile, absolute roots, or limits"))
	}
	if len(req.Input.Files) == 0 {
		return returnError(fail("invalid_input", "empty Go selection"))
	}
	root := req.Input.RepositoryRoot
	seen := map[string]bool{}
	for _, rel := range req.Input.Files {
		if seen[rel] {
			return returnError(fail("invalid_input", "duplicate selected file", rel))
		}
		seen[rel] = true
		if _, err := relativeFile(root, rel); err != nil {
			return returnError(fail("invalid_input", err.Error(), rel))
		}
	}
	if problem := rejectSpecialSources(root, req.Input.Files, req.Input.IncludeTests); problem != nil {
		return returnError(problem)
	}
	ids := map[string]bool{}
	for _, p := range req.Environment.Platforms {
		if ids[p.ID] {
			return returnError(fail("invalid_input", "duplicate platform id"))
		}
		ids[p.ID] = true
		if err := validatePlatform(p); err != nil {
			return returnError(fail("invalid_input", err.Error()))
		}
	}
	groups, problem := loadProjectSelections(root, req.Input.Files)
	if problem != nil {
		return returnError(problem)
	}
	plans := []analysisConfiguration{}
	for index, group := range groups {
		discoveryFiles := []string{}
		for _, rel := range group.Files {
			if req.Input.IncludeTests || !strings.HasSuffix(rel, "_test.go") {
				discoveryFiles = append(discoveryFiles, rel)
			}
		}
		platforms, problem := discoverPlatforms(group.Project, discoveryFiles, req.Input.IncludeTests, req.Environment.Platforms)
		if problem != nil {
			return returnError(problem)
		}
		for _, sdk := range platforms {
			configID := "go-" + sdk.ID
			if len(groups) > 1 {
				configID = fmt.Sprintf("go-%d-%s", index+1, sdk.ID)
			}
			plans = append(plans, analysisConfiguration{Project: group.Project, Files: group.Files, SDK: sdk, ID: configID})
		}
	}
	ctx, cancel := context.WithTimeout(ctx, time.Duration(req.Limits.AnalysisTimeoutMS)*time.Millisecond)
	defer cancel()
	stopped := func() (completed, bool) {
		if errors.Is(ctx.Err(), context.DeadlineExceeded) {
			return returnError(fail("resource_limit", "Go analysis exceeded analysis_timeout_ms")), true
		}
		if errors.Is(ctx.Err(), context.Canceled) {
			base.Outcome = "cancelled"
			return base, true
		}
		return completed{}, false
	}
	builder := newGraphBuilder()
	active := map[string][]string{}
	ownedConfigs := map[string][]string{}
	configs := []configurationEvidence{}
	for _, plan := range plans {
		if response, yes := stopped(); yes {
			return response
		}
		project, sdk, configID := plan.Project, plan.SDK, plan.ID
		configs = append(configs, configurationEvidence{ID: configID, SourcePaths: project.projectSourcePaths(), PlatformIDs: []string{sdk.ID}})
		for _, rel := range plan.Files {
			ownedConfigs[rel] = append(ownedConfigs[rel], configID)
		}
		loader := &sourceLoader{Ctx: ctx, Project: project, SDK: sdk, Build: standardTarget(sdk), Fset: token.NewFileSet(), Cache: map[string]*packageUnit{}, Loading: map[string]bool{}}
		selectedPackages := map[string]bool{}
		for _, rel := range plan.Files {
			if response, yes := stopped(); yes {
				return response
			}
			if strings.HasSuffix(rel, "_test.go") && !req.Input.IncludeTests {
				continue
			}
			filename := filepath.Join(root, filepath.FromSlash(rel))
			m := project.moduleForFile(filename)
			if m == nil {
				return returnError(fail("unsupported_configuration", "selected file is outside workspace modules", rel))
			}
			dir := filepath.Dir(filename)
			entries, readErr := os.ReadDir(dir)
			if readErr != nil {
				return returnError(fail("object_extraction_failed", readErr.Error(), rel))
			}
			for _, entry := range entries {
				if strings.HasSuffix(entry.Name(), ".go") {
					if _, readErr := safeFile(root, filepath.Join(dir, entry.Name())); readErr != nil {
						return returnError(fail("unsupported_configuration", readErr.Error(), rel))
					}
				}
			}
			suffix, _ := filepath.Rel(m.Dir, dir)
			importPath := m.Path
			if suffix != "." {
				importPath += "/" + filepath.ToSlash(suffix)
			}
			meta, err := loader.Build.ImportDir(dir, build.IgnoreVendor)
			if err != nil {
				if _, ok := err.(*build.NoGoError); !ok {
					return returnError(fail("object_extraction_failed", err.Error(), rel))
				}
			}
			fileName := filepath.Base(filename)
			if req.Input.IncludeTests && containsString(meta.XTestGoFiles, fileName) {
				return returnError(fail("unsupported_configuration", "external _test package is outside the verified static profile", rel))
			}
			if !containsString(meta.GoFiles, fileName) && !(req.Input.IncludeTests && containsString(meta.TestGoFiles, fileName)) {
				continue
			}
			active[rel] = append(active[rel], configID)
			selectedPackages[importPath] = true
		}
		packagePaths := []string{}
		for p := range selectedPackages {
			packagePaths = append(packagePaths, p)
		}
		sort.Strings(packagePaths)
		for _, importPath := range packagePaths {
			if response, yes := stopped(); yes {
				return response
			}
			unit, err := loader.load(importPath, true, req.Input.IncludeTests)
			if err != nil {
				if response, yes := stopped(); yes {
					return response
				}
				if loader.Fatal != nil {
					return returnError(loader.Fatal)
				}
				return returnError(fail("object_extraction_failed", err.Error()))
			}
			if response, yes := stopped(); yes {
				return response
			}
			if loader.Fatal != nil {
				return returnError(loader.Fatal)
			}
			for _, f := range unit.Files {
				if !seen[f.Rel] || !containsString(active[f.Rel], configID) {
					continue
				}
				if problem := loader.extractFile(builder, unit, f); problem != nil {
					return returnError(problem)
				}
			}
		}
	}
	if response, yes := stopped(); yes {
		return response
	}
	fileEvidenceList := []fileEvidence{}
	for _, rel := range req.Input.Files {
		disposition := "analyzed"
		configIDs := active[rel]
		if strings.HasSuffix(rel, "_test.go") && !req.Input.IncludeTests {
			disposition = "excluded_test"
			configIDs = ownedConfigs[rel]
		} else if len(configIDs) == 0 {
			return returnError(fail("target_unavailable", "selected Go file is inactive in every admitted target", rel))
		}
		fileEvidenceList = append(fileEvidenceList, fileEvidence{Path: rel, Disposition: disposition, ConfigurationIDs: configIDs})
	}
	result, ev := builder.finish(configs, fileEvidenceList)
	if response, yes := stopped(); yes {
		return response
	}
	base.Outcome = "ok"
	base.Graph = &result
	base.Evidence = &ev
	encoded, err := json.Marshal(base)
	if err != nil {
		return returnError(fail("internal_failure", err.Error()))
	}
	if len(encoded)+1 > req.Limits.MaxResultBytes {
		return returnError(fail("resource_limit", "Go graph exceeds max_result_bytes"))
	}
	if response, yes := stopped(); yes {
		return response
	}
	return base
}

// containsString 判定一个文件或配置是否属于已排序集合。
func containsString(values []string, want string) bool {
	for _, value := range values {
		if value == want {
			return true
		}
	}
	return false
}
