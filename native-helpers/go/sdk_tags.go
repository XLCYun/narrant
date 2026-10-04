package main

import (
	"fmt"
	"go/ast"
	"go/parser"
	"go/token"
	"path/filepath"
	"sort"
	"strconv"
	"strings"
)

// sdkExperimentTags 静态读取目标 SDK 的默认实验开关，不执行工具链或继承 helper 的实验配置。
func sdkExperimentTags(sdk platform) ([]string, error) {
	bootstrap, err := readSDKAST(sdk.Root, "src/internal/buildcfg/zbootstrap.go")
	if err != nil {
		return nil, err
	}
	foundDefault := false
	for _, declaration := range bootstrap.Decls {
		group, ok := declaration.(*ast.GenDecl)
		if !ok || group.Tok != token.CONST {
			continue
		}
		for _, spec := range group.Specs {
			value := spec.(*ast.ValueSpec)
			if len(value.Names) == 1 && value.Names[0].Name == "defaultGOEXPERIMENT" {
				foundDefault = true
				if len(value.Values) != 1 {
					return nil, fmt.Errorf("unsupported defaultGOEXPERIMENT declaration")
				}
				defaultValue, err := sdkConstant(value.Values[0], nil)
				if err != nil || defaultValue != "" {
					return nil, fmt.Errorf("SDK with custom defaultGOEXPERIMENT is unsupported")
				}
			}
		}
	}
	if !foundDefault {
		return nil, fmt.Errorf("SDK defaultGOEXPERIMENT declaration was not found")
	}
	file, err := readSDKAST(sdk.Root, "src/internal/buildcfg/exp.go")
	if err != nil {
		return nil, err
	}
	values := map[string]any{"goos": sdk.Metadata.GOOS, "goarch": sdk.Metadata.GOARCH, "true": true, "false": false}
	for _, declaration := range file.Decls {
		function, ok := declaration.(*ast.FuncDecl)
		if !ok || function.Name.Name != "ParseGOEXPERIMENT" || function.Body == nil {
			continue
		}
		for _, statement := range function.Body.List {
			if assignment, ok := statement.(*ast.AssignStmt); ok && len(assignment.Lhs) == 1 && len(assignment.Rhs) == 1 {
				name, named := assignment.Lhs[0].(*ast.Ident)
				if named && name.Name == "baseline" {
					literal, ok := assignment.Rhs[0].(*ast.CompositeLit)
					if !ok {
						return nil, fmt.Errorf("SDK experiment baseline is not a literal")
					}
					kind, ok := literal.Type.(*ast.SelectorExpr)
					if !ok || kind.Sel.Name != "Flags" {
						return nil, fmt.Errorf("SDK experiment baseline is not goexperiment.Flags")
					}
					owner, ok := kind.X.(*ast.Ident)
					if !ok || owner.Name != "goexperiment" {
						return nil, fmt.Errorf("SDK experiment baseline is not goexperiment.Flags")
					}
					var tags []string
					for _, element := range literal.Elts {
						field, ok := element.(*ast.KeyValueExpr)
						if !ok {
							return nil, fmt.Errorf("SDK experiment baseline has an unnamed field")
						}
						name, ok := field.Key.(*ast.Ident)
						if !ok {
							return nil, fmt.Errorf("SDK experiment baseline has an invalid field")
						}
						value, err := sdkConstant(field.Value, values)
						if err != nil {
							return nil, err
						}
						enabled, ok := value.(bool)
						if !ok {
							return nil, fmt.Errorf("SDK experiment %s is not boolean", name.Name)
						}
						if enabled {
							tags = append(tags, "goexperiment."+strings.ToLower(name.Name))
						}
					}
					sort.Strings(tags)
					return tags, nil
				}
			}
			if err := readSDKDefaults(statement, values); err != nil {
				return nil, err
			}
		}
	}
	return nil, fmt.Errorf("SDK experiment baseline was not found")
}

// readSDKAST 只在已批准 SDK 根内解析其声明文件。
func readSDKAST(root, relative string) (*ast.File, error) {
	filename := filepath.Join(root, filepath.FromSlash(relative))
	data, err := safeFile(root, filename)
	if err != nil {
		return nil, err
	}
	return parser.ParseFile(token.NewFileSet(), filename, data, parser.SkipObjectResolution)
}

// sdkConstant 只解释布尔、字符串和比较表达式，拒绝函数调用及其他动态构造。
func sdkConstant(expression ast.Expr, values map[string]any) (any, error) {
	switch value := expression.(type) {
	case *ast.ParenExpr:
		return sdkConstant(value.X, values)
	case *ast.Ident:
		if constant, ok := values[value.Name]; ok {
			return constant, nil
		}
	case *ast.BasicLit:
		if value.Kind == token.STRING {
			return strconv.Unquote(value.Value)
		}
	case *ast.UnaryExpr:
		constant, err := sdkConstant(value.X, values)
		if flag, ok := constant.(bool); err == nil && ok && value.Op == token.NOT {
			return !flag, nil
		}
	case *ast.BinaryExpr:
		left, leftErr := sdkConstant(value.X, values)
		right, rightErr := sdkConstant(value.Y, values)
		if leftErr != nil || rightErr != nil {
			break
		}
		switch value.Op {
		case token.EQL:
			return left == right, nil
		case token.NEQ:
			return left != right, nil
		case token.LAND, token.LOR:
			leftFlag, leftOK := left.(bool)
			rightFlag, rightOK := right.(bool)
			if leftOK && rightOK {
				if value.Op == token.LAND {
					return leftFlag && rightFlag, nil
				}
				return leftFlag || rightFlag, nil
			}
		}
	}
	return nil, fmt.Errorf("unsupported static SDK default expression %T", expression)
}

// readSDKDefaults 解释 baseline 前的有限声明及目标分支，任何未知语句都显式失败。
func readSDKDefaults(statement ast.Stmt, values map[string]any) error {
	switch current := statement.(type) {
	case *ast.DeclStmt:
		declaration, ok := current.Decl.(*ast.GenDecl)
		if !ok || declaration.Tok != token.VAR {
			break
		}
		for _, spec := range declaration.Specs {
			value, ok := spec.(*ast.ValueSpec)
			if !ok || len(value.Values) != 0 {
				return fmt.Errorf("unsupported SDK default variable declaration")
			}
			kind, ok := value.Type.(*ast.Ident)
			if !ok || kind.Name != "bool" {
				return fmt.Errorf("SDK default variable is not boolean")
			}
			for _, name := range value.Names {
				values[name.Name] = false
			}
		}
		return nil
	case *ast.AssignStmt:
		if (current.Tok != token.ASSIGN && current.Tok != token.DEFINE) || len(current.Lhs) != len(current.Rhs) {
			break
		}
		for index, expression := range current.Rhs {
			name, ok := current.Lhs[index].(*ast.Ident)
			if !ok {
				return fmt.Errorf("unsupported SDK default assignment")
			}
			value, err := sdkConstant(expression, values)
			if err != nil {
				return err
			}
			values[name.Name] = value
		}
		return nil
	case *ast.SwitchStmt:
		if current.Init != nil || current.Tag == nil {
			break
		}
		value, err := sdkConstant(current.Tag, values)
		if err != nil {
			return err
		}
		var fallback []ast.Stmt
		for _, clause := range current.Body.List {
			branch := clause.(*ast.CaseClause)
			if branch.List == nil {
				fallback = branch.Body
			}
			for _, expression := range branch.List {
				match, err := sdkConstant(expression, values)
				if err != nil {
					return err
				}
				if value == match {
					return readSDKDefaultBranch(branch.Body, values)
				}
			}
		}
		return readSDKDefaultBranch(fallback, values)
	}
	return fmt.Errorf("unsupported static SDK default statement %T", statement)
}

// readSDKDefaultBranch 仅访问命中的目标分支，保持 SDK 原有默认布尔值。
func readSDKDefaultBranch(statements []ast.Stmt, values map[string]any) error {
	for _, statement := range statements {
		if err := readSDKDefaults(statement, values); err != nil {
			return err
		}
	}
	return nil
}
