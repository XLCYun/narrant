package main

import (
	"bufio"
	"fmt"
	"go/build"
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
	Root       string
	Modules    []*module
	WorkSource string
	WorkGo     string
	Replace    map[string]string
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
	p := &project{Root: root, Replace: map[string]string{}}
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
				p.Replace[parts[1]] = dir
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
			p.Replace[old] = dir
		}
	}
	return p, nil
}

// moduleForFile 查找包含文件的最近模块。
func (p *project) moduleForFile(filename string) *module {
	var best *module
	for _, m := range p.Modules {
		if err := within(m.Dir, filename); err == nil && (best == nil || len(m.Dir) > len(best.Dir)) {
			best = m
		}
	}
	return best
}

// importDirectory 将导入路径只映射到仓内 module 或批准的本地 replace。
func (p *project) importDirectory(importPath string) (string, bool) {
	var best *module
	for _, m := range p.Modules {
		if importPath == m.Path || strings.HasPrefix(importPath, m.Path+"/") {
			if best == nil || len(m.Path) > len(best.Path) {
				best = m
			}
		}
	}
	if best != nil {
		suffix := strings.TrimPrefix(importPath, best.Path)
		return filepath.Join(best.Dir, filepath.FromSlash(suffix)), true
	}
	for old, dir := range p.Replace {
		if importPath == old || strings.HasPrefix(importPath, old+"/") {
			return filepath.Join(dir, filepath.FromSlash(strings.TrimPrefix(importPath, old))), true
		}
	}
	return "", false
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

// explicitTargets 由选中文件名及 build constraints 发现有限必要目标。
func explicitTargets(root string, selected []string) (map[string]bool, *analysisError) {
	targets := map[string]bool{}
	knownOS := map[string]bool{"darwin": true, "linux": true, "windows": true}
	knownArch := map[string]bool{"arm64": true, "amd64": true}
	for _, rel := range selected {
		parts := strings.Split(strings.TrimSuffix(path.Base(rel), ".go"), "_")
		for _, part := range parts {
			if knownOS[part] || knownArch[part] {
				targets[part] = true
			}
		}
		data, err := safeFile(root, filepath.Join(root, filepath.FromSlash(rel)))
		if err != nil {
			return nil, fail("invalid_input", err.Error(), rel)
		}
		for _, line := range strings.Split(string(data), "\n") {
			line = strings.TrimSpace(line)
			if strings.HasPrefix(line, "// +build ") {
				return nil, fail("unsupported_configuration", "legacy +build condition requires a separately verified profile", rel)
			}
			if !strings.HasPrefix(line, "//go:build ") && !strings.HasPrefix(line, "// +build ") {
				if line != "" && !strings.HasPrefix(line, "//") {
					break
				}
				continue
			}
			constraint := strings.TrimSpace(strings.TrimPrefix(strings.TrimPrefix(line, "//go:build"), "// +build"))
			if strings.Contains(constraint, "!") {
				return nil, fail("unsupported_configuration", "negated build conditions require a separately verified profile", rel)
			}
			for _, word := range strings.FieldsFunc(constraint, func(r rune) bool {
				return !((r >= 'a' && r <= 'z') || (r >= 'A' && r <= 'Z') || (r >= '0' && r <= '9') || r == '_' || r == '.')
			}) {
				if word == "unix" {
					targets["darwin"] = true
					targets["linux"] = true
					continue
				}
				if word == "go1.20" || word == "go1.21" || word == "go1.22" || word == "go1.23" {
					continue
				}
				if word == "" || word == "cgo" {
					if word == "cgo" {
						return nil, fail("unsupported_configuration", "cgo build constraint is unsupported", rel)
					}
					continue
				}
				if knownOS[word] || knownArch[word] {
					targets[word] = true
					continue
				}
				return nil, fail("unsupported_configuration", "custom or unsupported build tag: "+word, rel)
			}
		}
	}
	return targets, nil
}

// discoverPlatforms 根据明确条件选择 catalog 候选；无条件时仅取宿主基线。
func discoverPlatforms(root string, selected []string, candidates []platform) ([]platform, *analysisError) {
	need, problem := explicitTargets(root, selected)
	if problem != nil {
		return nil, problem
	}
	knownOS := map[string]bool{"darwin": true, "linux": true, "windows": true}
	knownArch := map[string]bool{"amd64": true, "arm64": true}
	needOS, needArch := map[string]bool{}, map[string]bool{}
	for item := range need {
		if knownOS[item] {
			needOS[item] = true
		}
		if knownArch[item] {
			needArch[item] = true
		}
	}
	var chosen []platform
	for _, p := range candidates {
		if len(need) > 0 && (len(needOS) == 0 || needOS[p.Metadata.GOOS]) && (len(needArch) == 0 || needArch[p.Metadata.GOARCH]) {
			chosen = append(chosen, p)
		}
	}
	if len(need) == 0 {
		for _, p := range candidates {
			if p.Metadata.GOOS == runtime.GOOS && p.Metadata.GOARCH == runtime.GOARCH {
				chosen = append(chosen, p)
			}
		}
	}
	if len(chosen) == 0 {
		return nil, fail("target_unavailable", "no catalog SDK matches statically discovered Go target")
	}
	for item := range need {
		found := false
		for _, p := range chosen {
			if p.Metadata.GOOS == item || p.Metadata.GOARCH == item {
				found = true
			}
		}
		if !found {
			return nil, fail("target_unavailable", "missing target for "+item)
		}
	}
	sort.Slice(chosen, func(i, j int) bool { return chosen[i].ID < chosen[j].ID })
	return chosen, nil
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
