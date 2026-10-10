package main

import (
	"bufio"
	"fmt"
	"go/build"
	"go/build/constraint"
	"go/parser"
	"go/token"
	"go/version"
	"os"
	"path"
	"path/filepath"
	"runtime"
	"sort"
	"strconv"
	"strings"
)

// module 描述仓内静态可解释的 Go module。
type module struct {
	Path      string
	Dir       string
	GoVersion string
	Toolchain string
	Source    string
	Replaces  map[string]string
}

// project 持有只读仓库模块索引和配置文件来源。
type project struct {
	Root           string
	Modules        []*module
	WorkSource     string
	WorkGo         string
	WorkToolchain  string
	Replace        map[string]string
	WorkReplace    map[string]string
	ContextModules map[string]*module
}

// within 拒绝词法及符号链接层面的仓外路径。
func within(root, candidate string) error {
	r, err := filepath.EvalSymlinks(root)
	if err != nil {
		return err
	}
	c, err := filepath.EvalSymlinks(candidate)
	if err != nil {
		return err
	}
	rel, err := filepath.Rel(r, c)
	if err != nil || rel == ".." || strings.HasPrefix(rel, ".."+string(filepath.Separator)) {
		return fmt.Errorf("path escapes allowed root: %s", candidate)
	}
	return nil
}

// safeFile 读取边界内普通文件，绝不追随指向仓外的链接。
func safeFile(root, filename string) ([]byte, error) {
	if err := within(root, filename); err != nil {
		return nil, err
	}
	info, err := os.Stat(filename)
	if err != nil {
		return nil, err
	}
	if !info.Mode().IsRegular() {
		return nil, fmt.Errorf("not a regular file: %s", filename)
	}
	return os.ReadFile(filename)
}

// relativeFile 验证协议路径是可往返的仓库相对普通文件。
func relativeFile(root, relative string) (string, error) {
	if relative == "" || strings.ContainsRune(relative, 0) || strings.Contains(relative, "\\") || strings.HasPrefix(relative, "/") || path.Clean(relative) != relative || relative == ".." || strings.HasPrefix(relative, "../") || strings.Contains(relative, ":") {
		return "", fmt.Errorf("invalid relative path %q", relative)
	}
	abs := filepath.Join(root, filepath.FromSlash(relative))
	if err := within(root, abs); err != nil {
		return "", err
	}
	return abs, nil
}

// scanConfig 将 go.mod/go.work 的有限声明式子集分组成指令。
func scanConfig(data []byte) ([][]string, error) {
	var directives [][]string
	var block string
	scanner := bufio.NewScanner(strings.NewReader(string(data)))
	for scanner.Scan() {
		line := strings.TrimSpace(scanner.Text())
		if line == "" || strings.HasPrefix(line, "//") {
			continue
		}
		if i := strings.Index(line, "//"); i >= 0 {
			line = strings.TrimSpace(line[:i])
		}
		if line == ")" {
			if block == "" {
				return nil, fmt.Errorf("unexpected closing parenthesis")
			}
			block = ""
			continue
		}
		parts := strings.Fields(line)
		if len(parts) == 0 {
			continue
		}
		if block == "" && len(parts) == 2 && parts[1] == "(" {
			block = parts[0]
			continue
		}
		if block != "" {
			parts = append([]string{block}, parts...)
		}
		directives = append(directives, parts)
	}
	if err := scanner.Err(); err != nil {
		return nil, err
	}
	if block != "" {
		return nil, fmt.Errorf("unclosed %s block", block)
	}
	return directives, nil
}

// localDirectivePath 只接受可证明在仓库内的本地目录引用。
func localDirectivePath(root, base, spelling string) (string, error) {
	if !strings.HasPrefix(spelling, "./") && !strings.HasPrefix(spelling, "../") && spelling != "." {
		return "", fmt.Errorf("nonlocal path %q", spelling)
	}
	place := filepath.Clean(filepath.Join(base, filepath.FromSlash(spelling)))
	if err := within(root, place); err != nil {
		return "", err
	}
	info, err := os.Stat(place)
	if err != nil {
		return "", err
	}
	if !info.IsDir() {
		return "", fmt.Errorf("not a directory: %s", spelling)
	}
	return place, nil
}

// parseMod 解释 module、go、require 和仓内 replace；未知构建语义失败。
func parseMod(root, filename string) (*module, error) {
	data, err := safeFile(root, filename)
	if err != nil {
		return nil, err
	}
	directives, err := scanConfig(data)
	if err != nil {
		return nil, err
	}
	m := &module{Dir: filepath.Dir(filename), Replaces: map[string]string{}}
	for _, p := range directives {
		switch p[0] {
		case "module":
			if len(p) != 2 || m.Path != "" {
				return nil, fmt.Errorf("invalid module directive")
			}
			m.Path = strings.Trim(p[1], "\"")
		case "go":
			if len(p) != 2 || m.GoVersion != "" || !validGoVersion(p[1]) {
				return nil, fmt.Errorf("invalid go directive")
			}
			m.GoVersion = p[1]
		case "toolchain":
			if len(p) != 2 || m.Toolchain != "" || !validToolchain(p[1]) {
				return nil, fmt.Errorf("unsupported toolchain %v", p)
			}
			m.Toolchain = p[1]
		case "require", "exclude", "retract": // 版本求解不触发依赖读取；缺失依赖在 importer 中保留未知。
		case "replace":
			arrow := -1
			for i, x := range p {
				if x == "=>" {
					arrow = i
					break
				}
			}
			if arrow != 2 || arrow+2 != len(p) {
				return nil, fmt.Errorf("unsupported replace directive %v", p)
			}
			old := p[1]
			place, err := localDirectivePath(root, m.Dir, p[arrow+1])
			if err != nil {
				return nil, err
			}
			if previous, ok := m.Replaces[old]; ok && previous != place {
				return nil, fmt.Errorf("conflicting module replace for %s", old)
			}
			m.Replaces[old] = place
		default:
			return nil, fmt.Errorf("unsupported go.mod directive %q", p[0])
		}
	}
	if m.Path == "" || m.GoVersion == "" {
		return nil, fmt.Errorf("module and go version required")
	}
	rel, _ := filepath.Rel(root, filename)
	m.Source = filepath.ToSlash(rel)
	return m, nil
}

// loadProject 静态发现仓内 module/workspace，不查询 go 命令或用户缓存。
func loadProject(root string, selected []string) (*project, *analysisError) {
	p := &project{Root: root, Replace: map[string]string{}, WorkReplace: map[string]string{}, ContextModules: map[string]*module{}}
	work := filepath.Join(root, "go.work")
	var useDirs []string
	if _, err := os.Lstat(work); err == nil {
		data, err := safeFile(root, work)
		if err != nil {
			return nil, fail("unsupported_configuration", err.Error(), "go.work")
		}
		directives, err := scanConfig(data)
		if err != nil {
			return nil, fail("unsupported_configuration", err.Error(), "go.work")
		}
		p.WorkSource = "go.work"
		for _, parts := range directives {
			switch parts[0] {
			case "go":
				if len(parts) != 2 || p.WorkGo != "" || !validGoVersion(parts[1]) {
					return nil, fail("unsupported_configuration", "invalid go.work go directive", "go.work")
				}
				p.WorkGo = parts[1]
			case "toolchain":
				if len(parts) != 2 || p.WorkToolchain != "" || !validToolchain(parts[1]) {
					return nil, fail("unsupported_configuration", "unsupported go.work toolchain", "go.work")
				}
				p.WorkToolchain = parts[1]
			case "use":
				if len(parts) != 2 {
					return nil, fail("unsupported_configuration", "invalid use directive", "go.work")
				}
				dir, err := localDirectivePath(root, root, strings.Trim(parts[1], "\""))
				if err != nil {
					return nil, fail("unsupported_configuration", err.Error(), "go.work")
				}
				useDirs = append(useDirs, dir)
			case "replace":
				arrow := -1
				for i, x := range parts {
					if x == "=>" {
						arrow = i
						break
					}
				}
				if arrow != 2 || arrow+2 != len(parts) {
					return nil, fail("unsupported_configuration", "unsupported go.work replace", "go.work")
				}
				dir, err := localDirectivePath(root, root, parts[arrow+1])
				if err != nil {
					return nil, fail("unsupported_configuration", err.Error(), "go.work")
				}
				if previous, ok := p.WorkReplace[parts[1]]; ok && previous != dir {
					return nil, fail("unsupported_configuration", "conflicting go.work replace for "+parts[1], "go.work")
				}
				p.WorkReplace[parts[1]] = dir
			default:
				return nil, fail("unsupported_configuration", "unsupported go.work directive: "+parts[0], "go.work")
			}
		}
		if p.WorkGo == "" || len(useDirs) == 0 {
			return nil, fail("unsupported_configuration", "go.work requires a go version and at least one use", "go.work")
		}
	} else if !os.IsNotExist(err) {
		return nil, fail("unsupported_configuration", err.Error(), "go.work")
	}
	if p.WorkSource == "" {
		seen := map[string]bool{}
		for _, rel := range selected {
			dir := filepath.Dir(filepath.Join(root, filepath.FromSlash(rel)))
			for {
				if err := within(root, dir); err != nil {
					break
				}
				mod := filepath.Join(dir, "go.mod")
				if _, err := os.Lstat(mod); err == nil {
					if !seen[dir] {
						useDirs = append(useDirs, dir)
						seen[dir] = true
					}
					break
				}
				if dir == root {
					break
				}
				dir = filepath.Dir(dir)
			}
		}
	}
	if len(useDirs) == 0 {
		return nil, fail("unsupported_configuration", "selected files are outside a go.mod/go.work module", selected...)
	}
	if p.WorkSource == "" && len(useDirs) > 1 {
		return nil, fail("unsupported_configuration", "multiple modules require an explicit go.work", selected...)
	}
	sort.Strings(useDirs)
	paths := map[string]bool{}
	for _, dir := range useDirs {
		m, err := parseMod(root, filepath.Join(dir, "go.mod"))
		if err != nil {
			return nil, fail("unsupported_configuration", err.Error(), filepath.ToSlash(strings.TrimPrefix(dir, root+string(filepath.Separator)))+"/go.mod")
		}
		if paths[m.Path] {
			return nil, fail("unsupported_configuration", "duplicate module path: "+m.Path, m.Source)
		}
		paths[m.Path] = true
		p.Modules = append(p.Modules, m)
		if p.WorkSource != "" && version.Compare("go"+p.WorkGo, "go"+m.GoVersion) < 0 {
			return nil, fail("unsupported_configuration", "go.work go version is older than module go version "+m.GoVersion, p.WorkSource, m.Source)
		}
		if problem := requireCompilerLanguage(m.GoVersion, m.Source); problem != nil {
			return nil, problem
		}
	}
	for _, m := range p.Modules {
		for old, dir := range m.Replaces {
			if previous, ok := p.Replace[old]; ok && previous != dir {
				if _, override := p.WorkReplace[old]; !override {
					return nil, fail("unsupported_configuration", "conflicting go.mod replace for "+old, m.Source)
				}
			}
			p.Replace[old] = dir
		}
	}
	for old, dir := range p.WorkReplace {
		p.Replace[old] = dir
	}
	return p, nil
}

// moduleForFile 查找包含文件的最近模块。
func (p *project) moduleForFile(filename string) *module {
	var best *module
	for _, m := range p.Modules {
		if err := within(m.Dir, filename); err == nil && !crossedModuleBoundary(m.Dir, filename) && (best == nil || len(m.Dir) > len(best.Dir)) {
			best = m
		}
	}
	return best
}

// moduleForPackage 为已准入的仓内包读取自身 module 的语言声明，包括本地 replace。
func (p *project) moduleForPackage(dir string) (*module, error) {
	if m := p.moduleForFile(dir); m != nil {
		return m, nil
	}
	base := ""
	for _, replacement := range p.Replace {
		if within(replacement, dir) == nil && !crossedModuleBoundary(replacement, dir) && len(replacement) > len(base) {
			base = replacement
		}
	}
	if base == "" {
		return nil, fmt.Errorf("package is outside admitted module: %s", dir)
	}
	if m := p.ContextModules[base]; m != nil {
		return m, nil
	}
	m, err := parseMod(p.Root, filepath.Join(base, "go.mod"))
	if err != nil {
		return nil, fmt.Errorf("local replacement go.mod: %w", err)
	}
	p.ContextModules[base] = m
	return m, nil
}

// crossedModuleBoundary 拒绝把未显式 use 的嵌套 go.mod 当作外层包。
func crossedModuleBoundary(base, candidate string) bool {
	dir := candidate
	if info, err := os.Stat(candidate); err == nil && !info.IsDir() {
		dir = filepath.Dir(candidate)
	}
	for dir != base {
		if dir == filepath.Dir(dir) {
			return true
		}
		if _, err := os.Lstat(filepath.Join(dir, "go.mod")); err == nil {
			return true
		}
		dir = filepath.Dir(dir)
	}
	return false
}

// importDirectory 将导入路径只映射到仓内 module 或批准的本地 replace。
func (p *project) importDirectory(importPath string) (string, bool, error) {
	var best *module
	for _, m := range p.Modules {
		if importPath == m.Path || strings.HasPrefix(importPath, m.Path+"/") {
			if best == nil || len(m.Path) > len(best.Path) {
				best = m
			}
		}
	}
	bestReplacement := ""
	for old := range p.Replace {
		if importPath == old || strings.HasPrefix(importPath, old+"/") {
			if len(old) > len(bestReplacement) {
				bestReplacement = old
			}
		}
	}
	if bestReplacement != "" && (best == nil || len(bestReplacement) > len(best.Path)) {
		base := p.Replace[bestReplacement]
		dir := filepath.Join(base, filepath.FromSlash(strings.TrimPrefix(importPath, bestReplacement)))
		if crossedModuleBoundary(base, dir) {
			return "", false, fmt.Errorf("nested module outside explicit workspace mapping: %s", importPath)
		}
		return dir, true, nil
	}
	if best != nil {
		dir := filepath.Join(best.Dir, filepath.FromSlash(strings.TrimPrefix(importPath, best.Path)))
		if crossedModuleBoundary(best.Dir, dir) {
			return "", false, fmt.Errorf("nested module outside explicit workspace use: %s", importPath)
		}
		return dir, true, nil
	}
	return "", false, nil
}

// standardTarget 使用实际 SDK 的发布标签选择源码，关闭 cgo 与自定义 tag。
func standardTarget(sdk platform) build.Context {
	meta := sdk.Metadata
	bc := build.Default
	bc.GOROOT = sdk.Root
	bc.GOPATH = ""
	bc.GOOS = meta.GOOS
	bc.GOARCH = meta.GOARCH
	bc.CgoEnabled = false
	bc.Compiler = "gc"
	bc.BuildTags = nil
	bc.ReleaseTags = nil
	for minor := 1; version.Compare(fmt.Sprintf("go1.%d", minor), version.Lang("go"+sdk.Version)) <= 0; minor++ {
		bc.ReleaseTags = append(bc.ReleaseTags, fmt.Sprintf("go1.%d", minor))
	}
	bc.ToolTags = append([]string{}, sdk.ToolTags...)
	if meta.GOARCH == "amd64" {
		bc.ToolTags = append(bc.ToolTags, "amd64.v1")
	} else if meta.GOARCH == "arm64" {
		bc.ToolTags = append(bc.ToolTags, "arm64.v8.0")
	}
	return bc
}

// validatePlatform 核对 SDK 版本与受支持的目标组合。
func validatePlatform(p platform) error {
	if p.ID == "" || p.Kind != "go-sdk" || !validSDKVersion(p.Version) || !filepath.IsAbs(p.Root) {
		return fmt.Errorf("invalid Go SDK catalog entry")
	}
	if p.Metadata.GOOS == "" || p.Metadata.GOARCH == "" {
		return fmt.Errorf("missing Go target dimensions")
	}
	if p.Metadata.GOAMD64 != "" && (p.Metadata.GOARCH != "amd64" || p.Metadata.GOAMD64 != "v1") {
		return fmt.Errorf("only amd64.v1 is verified")
	}
	if p.Metadata.GOARCH == "amd64" && p.Metadata.GOAMD64 != "v1" {
		return fmt.Errorf("explicit goamd64 v1 required")
	}
	if p.Metadata.GOOS != "darwin" && p.Metadata.GOOS != "linux" && p.Metadata.GOOS != "windows" {
		return fmt.Errorf("unsupported GOOS %s", p.Metadata.GOOS)
	}
	if p.Metadata.GOARCH != "arm64" && p.Metadata.GOARCH != "amd64" {
		return fmt.Errorf("unsupported GOARCH %s", p.Metadata.GOARCH)
	}
	data, err := safeFile(p.Root, filepath.Join(p.Root, "VERSION"))
	if err != nil {
		return err
	}
	firstLine, _, _ := strings.Cut(string(data), "\n")
	if strings.TrimSpace(firstLine) != "go"+p.Version {
		return fmt.Errorf("SDK VERSION does not match catalog Go %s", p.Version)
	}
	if _, err := os.Stat(filepath.Join(p.Root, "src", "builtin", "builtin.go")); err != nil {
		return err
	}
	return nil
}

// discoverPlatforms 根据明确条件选择 catalog 候选；无条件时仅取宿主基线。
func discoverPlatforms(project *project, selected []string, includeTests bool, candidates []platform) ([]platform, *analysisError) {
	candidates, problem := selectSDKVersions(project, candidates)
	if problem != nil {
		return nil, problem
	}
	contextFiles, problem := projectContextFiles(project, selected, includeTests, candidates[0])
	if problem != nil {
		return nil, problem
	}
	explicit := []string{}
	selectedSet := map[string]bool{}
	for _, rel := range selected {
		selectedSet[rel] = true
	}
	for _, rel := range contextFiles {
		hasTarget, problem := hasTargetConstraint(project.Root, rel)
		if problem != nil {
			return nil, problem
		}
		if hasTarget {
			supported, problem := matchesSupportedTarget(project.Root, rel, candidates[0])
			if problem != nil {
				return nil, problem
			}
			if supported {
				explicit = append(explicit, rel)
			} else if selectedSet[rel] {
				return nil, fail("target_unavailable", "selected file has no supported Go target", rel)
			}
		}
	}
	chosen := []platform{}
	matched := map[string]bool{}
	for _, candidate := range candidates {
		if len(explicit) == 0 {
			if candidate.Metadata.GOOS == runtime.GOOS && candidate.Metadata.GOARCH == runtime.GOARCH {
				chosen = append(chosen, candidate)
			}
			continue
		}
		buildContext := standardTarget(candidate)
		for _, rel := range explicit {
			matches, err := buildContext.MatchFile(filepath.Join(project.Root, filepath.Dir(rel)), filepath.Base(rel))
			if err != nil {
				return nil, fail("unsupported_configuration", err.Error(), rel)
			}
			if matches {
				matched[rel] = true
				if !containsPlatform(chosen, candidate.ID) {
					chosen = append(chosen, candidate)
				}
			}
		}
	}
	for _, rel := range explicit {
		if !matched[rel] {
			return nil, fail("target_unavailable", "missing approved Go SDK target for explicit source condition", rel)
		}
	}
	if len(chosen) == 0 {
		return nil, fail("target_unavailable", "no approved Go SDK target satisfies discovered source constraints")
	}
	sort.Slice(chosen, func(i, j int) bool { return chosen[i].ID < chosen[j].ID })
	return chosen, nil
}

// containsPlatform 避免一个目标匹配多个文件时重复分析同一配置。
func containsPlatform(platforms []platform, id string) bool {
	for _, candidate := range platforms {
		if candidate.ID == id {
			return true
		}
	}
	return false
}

// matchesSupportedTarget 独立于 catalog 判断源码条件是否属于固定 profile 的目标矩阵。
func matchesSupportedTarget(root, rel string, sdk platform) (bool, *analysisError) {
	for _, goos := range []string{"darwin", "linux", "windows"} {
		for _, goarch := range []string{"arm64", "amd64"} {
			meta := platformMetadata{GOOS: goos, GOARCH: goarch}
			if goarch == "amd64" {
				meta.GOAMD64 = "v1"
			}
			sdk.Metadata = meta
			target := standardTarget(sdk)
			matches, err := target.MatchFile(filepath.Join(root, filepath.Dir(rel)), filepath.Base(rel))
			if err != nil {
				return false, fail("unsupported_configuration", err.Error(), rel)
			}
			if matches {
				return true, nil
			}
		}
	}
	return false, nil
}

// projectContextFiles 沿仓内显式导入闭包搜集允许源码及平台条件，绝不读取第三方依赖。
func projectContextFiles(p *project, selected []string, includeTests bool, sdk platform) ([]string, *analysisError) {
	dirs := map[string]bool{}
	for _, rel := range selected {
		if !includeTests && strings.HasSuffix(rel, "_test.go") {
			continue
		}
		filename := filepath.Join(p.Root, filepath.FromSlash(rel))
		if p.moduleForFile(filename) == nil {
			return nil, fail("unsupported_configuration", "selected file is outside admitted module", rel)
		}
		dirs[filepath.Dir(filename)] = true
	}
	files := []string{}
	queue := []string{}
	for dir := range dirs {
		queue = append(queue, dir)
	}
	sort.Strings(queue)
	visited := map[string]bool{}
	for len(queue) > 0 {
		dir := queue[0]
		queue = queue[1:]
		if visited[dir] {
			continue
		}
		visited[dir] = true
		m, err := p.moduleForPackage(dir)
		if err != nil {
			return nil, fail("unsupported_configuration", err.Error())
		}
		if problem := m.validateSDKVersion(sdk); problem != nil {
			return nil, problem
		}
		entries, err := os.ReadDir(dir)
		if err != nil {
			return nil, fail("object_extraction_failed", err.Error())
		}
		for _, entry := range entries {
			if !strings.HasSuffix(entry.Name(), ".go") || ((!includeTests || !dirs[dir]) && strings.HasSuffix(entry.Name(), "_test.go")) {
				continue
			}
			filename := filepath.Join(dir, entry.Name())
			data, err := safeFile(p.Root, filename)
			if err != nil {
				return nil, fail("unsupported_configuration", err.Error())
			}
			rel, _ := filepath.Rel(p.Root, filename)
			files = append(files, filepath.ToSlash(rel))
			if _, problem := hasTargetConstraint(p.Root, filepath.ToSlash(rel)); problem != nil {
				return nil, problem
			}
			supported, problem := matchesSupportedTarget(p.Root, filepath.ToSlash(rel), sdk)
			if problem != nil {
				return nil, problem
			}
			if !supported {
				continue
			}
			parsed, err := parser.ParseFile(token.NewFileSet(), filename, data, parser.ImportsOnly)
			if err != nil {
				return nil, fail("object_extraction_failed", err.Error(), filepath.ToSlash(rel))
			}
			for _, imported := range parsed.Imports {
				name, err := strconv.Unquote(imported.Path.Value)
				if err != nil {
					return nil, fail("object_extraction_failed", err.Error(), filepath.ToSlash(rel))
				}
				dependency, local, err := p.importDirectory(name)
				if err != nil {
					return nil, fail("unsupported_configuration", err.Error(), filepath.ToSlash(rel))
				}
				if local {
					if err := within(p.Root, dependency); err != nil {
						return nil, fail("unsupported_configuration", err.Error(), filepath.ToSlash(rel))
					}
					queue = append(queue, dependency)
				}
			}
		}
	}
	sort.Strings(files)
	return files, nil
}

// hasTargetConstraint 解析 Go 布尔条件并校验其中所有标签有固定含义。
func hasTargetConstraint(root, rel string) (bool, *analysisError) {
	data, err := safeFile(root, filepath.Join(root, filepath.FromSlash(rel)))
	if err != nil {
		return false, fail("unsupported_configuration", err.Error(), rel)
	}
	base := strings.TrimSuffix(filepath.Base(rel), ".go")
	base = strings.TrimSuffix(base, "_test")
	pieces := strings.Split(base, "_")
	explicit := false
	if len(pieces) > 1 && knownTargetTag(pieces[len(pieces)-1]) {
		explicit = true
	}
	for _, line := range strings.Split(string(data), "\n") {
		line = strings.TrimSpace(line)
		if !strings.HasPrefix(line, "//go:build ") && !strings.HasPrefix(line, "// +build ") {
			if line != "" && !strings.HasPrefix(line, "//") {
				break
			}
			continue
		}
		expr, err := constraint.Parse(line)
		if err != nil {
			return false, fail("unsupported_configuration", err.Error(), rel)
		}
		tags := map[string]bool{}
		collectConstraintTags(expr, tags)
		for tag := range tags {
			if !supportedBuildTag(tag) {
				return false, fail("unsupported_configuration", "unsupported build tag: "+tag, rel)
			}
			if knownTargetTag(tag) || tag == "unix" || tag == "cgo" {
				explicit = true
			}
		}
	}
	return explicit, nil
}

// collectConstraintTags 递归读取布尔表达式中的原子标签。
func collectConstraintTags(expr constraint.Expr, tags map[string]bool) {
	switch e := expr.(type) {
	case *constraint.TagExpr:
		tags[e.Tag] = true
	case *constraint.NotExpr:
		collectConstraintTags(e.X, tags)
	case *constraint.AndExpr:
		collectConstraintTags(e.X, tags)
		collectConstraintTags(e.Y, tags)
	case *constraint.OrExpr:
		collectConstraintTags(e.X, tags)
		collectConstraintTags(e.Y, tags)
	}
}

// knownTargetTag 标识 Go 文件选择中的系统或架构条件。
func knownTargetTag(tag string) bool {
	switch tag {
	case "aix", "android", "darwin", "dragonfly", "freebsd", "hurd", "illumos", "ios", "js", "linux", "nacl", "netbsd", "openbsd", "plan9", "solaris", "wasip1", "windows", "386", "amd64", "amd64p32", "arm", "arm64", "loong64", "mips", "mips64", "mips64le", "mipsle", "ppc64", "ppc64le", "riscv64", "s390x", "wasm":
		return true
	}
	return false
}

// supportedBuildTag 接受已知目标及所有合法 release 标签；真假由所选 SDK 决定。
func supportedBuildTag(tag string) bool {
	if knownTargetTag(tag) || tag == "unix" || tag == "cgo" || tag == "gc" {
		return true
	}
	return strings.HasPrefix(tag, "go1.") && validGoVersion(strings.TrimPrefix(tag, "go")) && version.Lang(tag) == tag
}

// projectSourcePaths 返回实际读取并影响配置的仓内配置文件。
func (p *project) projectSourcePaths() []string {
	paths := []string{}
	if p.WorkSource != "" {
		paths = append(paths, p.WorkSource)
	}
	for _, m := range p.Modules {
		paths = append(paths, m.Source)
	}
	for _, m := range p.ContextModules {
		paths = append(paths, m.Source)
	}
	sort.Strings(paths)
	return paths
}

// rejectSpecialSources 拒绝不能由纯 Go parser/types 保证完整性的源形态。
func rejectSpecialSources(root string, selected []string, includeTests bool) *analysisError {
	for _, rel := range selected {
		if !strings.HasSuffix(rel, ".go") {
			return fail("invalid_input", "Go helper only accepts .go files", rel)
		}
		if strings.HasSuffix(rel, "_test.go") && !includeTests {
			continue
		}
		if strings.Contains(rel, "/vendor/") || strings.HasPrefix(rel, "vendor/") {
			return fail("unsupported_configuration", "vendor source selection is unsupported", rel)
		}
		data, err := safeFile(root, filepath.Join(root, filepath.FromSlash(rel)))
		if err != nil {
			return fail("invalid_input", err.Error(), rel)
		}
		if strings.Contains(string(data), "import \"C\"") || strings.Contains(string(data), "//go:embed") {
			return fail("unsupported_configuration", "cgo and embed directives are outside the verified static profile", rel)
		}
	}
	return nil
}
