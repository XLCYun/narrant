package main

import (
	"bufio"
	"fmt"
	"go/build"
	"go/build/constraint"
	"os"
	"path"
	"path/filepath"
	"runtime"
	"sort"
	"strings"
)

// module 描述仓内静态可解释的 Go module。
type module struct {
	Path      string
	Dir       string
	GoVersion string
	Source    string
	Replaces  map[string]string
}

// project 持有只读仓库模块索引和配置文件来源。
type project struct {
	Root        string
	Modules     []*module
	WorkSource  string
	WorkGo      string
	Replace     map[string]string
	WorkReplace map[string]string
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
			if len(p) != 2 {
				return nil, fmt.Errorf("invalid go directive")
			}
			m.GoVersion = p[1]
		case "toolchain":
			if len(p) != 2 || p[1] != "go1.23.8" {
				return nil, fmt.Errorf("unsupported toolchain %v", p)
			}
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
	if m.GoVersion != "1.23" && !strings.HasPrefix(m.GoVersion, "1.23.") {
		return nil, fmt.Errorf("Go version %s is outside verified 1.23 profile", m.GoVersion)
	}
	rel, _ := filepath.Rel(root, filename)
	m.Source = filepath.ToSlash(rel)
	return m, nil
}

// loadProject 静态发现仓内 module/workspace，不查询 go 命令或用户缓存。
func loadProject(root string, selected []string) (*project, *analysisError) {
	p := &project{Root: root, Replace: map[string]string{}, WorkReplace: map[string]string{}}
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
				if len(parts) != 2 {
					return nil, fail("unsupported_configuration", "invalid go.work go directive", "go.work")
				}
				p.WorkGo = parts[1]
			case "toolchain":
				if len(parts) != 2 || parts[1] != "go1.23.8" {
					return nil, fail("unsupported_configuration", "unsupported go.work toolchain", "go.work")
				}
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
		if (p.WorkGo != "1.23" && !strings.HasPrefix(p.WorkGo, "1.23.")) || len(useDirs) == 0 {
			return nil, fail("unsupported_configuration", "go.work requires Go 1.23 and at least one use", "go.work")
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

// standardTarget 从固定目录选择 Go 源码且关闭 cgo 与自定义 tag。
func standardTarget(sdk string, meta platformMetadata) build.Context {
	bc := build.Default
	bc.GOROOT = sdk
	bc.GOPATH = ""
	bc.GOOS = meta.GOOS
	bc.GOARCH = meta.GOARCH
	bc.CgoEnabled = false
	bc.BuildTags = nil
	bc.ToolTags = []string{"goexperiment.regabiwrappers", "goexperiment.regabiargs", "goexperiment.coverageredesign"}
	if meta.GOARCH == "amd64" {
		bc.ToolTags = append(bc.ToolTags, "amd64.v1")
	}
	return bc
}

// validatePlatform 核对 SDK 版本与受支持的目标组合。
func validatePlatform(p platform) error {
	if p.ID == "" || p.Kind != "go-sdk" || p.Version != "1.23.8" || p.Fingerprint == "" || !filepath.IsAbs(p.Root) {
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
	if !strings.HasPrefix(strings.TrimSpace(string(data)), "go1.23.8") {
		return fmt.Errorf("SDK VERSION is not go1.23.8")
	}
	if _, err := os.Stat(filepath.Join(p.Root, "src", "builtin", "builtin.go")); err != nil {
		return err
	}
	return nil
}

// discoverPlatforms 根据明确条件选择 catalog 候选；无条件时仅取宿主基线。
func discoverPlatforms(project *project, selected []string, includeTests bool, candidates []platform) ([]platform, *analysisError) {
	contextFiles, problem := projectContextFiles(project, selected, includeTests)
	if problem != nil {
		return nil, problem
	}
	explicit := []string{}
	for _, rel := range contextFiles {
		hasTarget, problem := hasTargetConstraint(project.Root, rel)
		if problem != nil {
			return nil, problem
		}
		if hasTarget {
			explicit = append(explicit, rel)
		}
	}
	chosen := []platform{}
	for _, candidate := range candidates {
		if len(explicit) == 0 {
			if candidate.Metadata.GOOS == runtime.GOOS && candidate.Metadata.GOARCH == runtime.GOARCH {
				chosen = append(chosen, candidate)
			}
			continue
		}
		buildContext := standardTarget(candidate.Root, candidate.Metadata)
		for _, rel := range explicit {
			matches, err := buildContext.MatchFile(filepath.Join(project.Root, filepath.Dir(rel)), filepath.Base(rel))
			if err != nil {
				return nil, fail("unsupported_configuration", err.Error(), rel)
			}
			if matches {
				chosen = append(chosen, candidate)
				break
			}
		}
	}
	if len(chosen) == 0 {
		return nil, fail("target_unavailable", "no approved Go SDK target satisfies discovered source constraints")
	}
	sort.Slice(chosen, func(i, j int) bool { return chosen[i].ID < chosen[j].ID })
	return chosen, nil
}

// projectContextFiles 搜集选中包中的允许源码，包括未选中的平台实现文件。
func projectContextFiles(p *project, selected []string, includeTests bool) ([]string, *analysisError) {
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
	for dir := range dirs {
		entries, err := os.ReadDir(dir)
		if err != nil {
			return nil, fail("object_extraction_failed", err.Error())
		}
		for _, entry := range entries {
			if !strings.HasSuffix(entry.Name(), ".go") || (!includeTests && strings.HasSuffix(entry.Name(), "_test.go")) {
				continue
			}
			filename := filepath.Join(dir, entry.Name())
			if _, err := safeFile(p.Root, filename); err != nil {
				return nil, fail("unsupported_configuration", err.Error())
			}
			rel, _ := filepath.Rel(p.Root, filename)
			files = append(files, filepath.ToSlash(rel))
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
	pieces := strings.Split(base, "_")
	explicit := false
	for _, piece := range pieces {
		if knownTargetTag(piece) {
			explicit = true
		}
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

// supportedBuildTag 限定静态可解释的目标、编译器与 Go 1.23 发布标签。
func supportedBuildTag(tag string) bool {
	if knownTargetTag(tag) || tag == "unix" || tag == "cgo" || tag == "gc" {
		return true
	}
	for minor := 1; minor <= 23; minor++ {
		if tag == fmt.Sprintf("go1.%d", minor) {
			return true
		}
	}
	return false
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
