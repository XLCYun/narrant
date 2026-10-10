package main

import (
	"errors"
	"fmt"
	"go/ast"
	"go/types"
	"go/version"
	"path/filepath"
	"regexp"
	"runtime"
	"sort"
	"strings"
)

// stableGoVersion 只承认正式发布的数字版本，不把 rc、devel 或自定义工具链当成稳定 SDK。
var stableGoVersion = regexp.MustCompile(`^1\.(0|[1-9][0-9]*)(\.(0|[1-9][0-9]*))?$`)

// languageVersionMessage 对应固定 Go 编译器公开 Msg 的版本诊断格式；错误码尚未公开，不能读取私有字段。
var languageVersionMessage = regexp.MustCompile(`(?:^|[ :])requires go1\.[0-9]+ or later$|^(?:file|package) requires newer Go version go1\.[0-9]+ \(application built with go1\.[0-9]+\)$`)

// validGoVersion 同时校验声明的数字格式及 Go 自身的版本语法。
func validGoVersion(value string) bool {
	return stableGoVersion.MatchString(value) && version.IsValid("go"+value)
}

// validSDKVersion 区分语言版本与真实稳定发布；Go 1.21 起首个发布也必须有 .0。
func validSDKVersion(value string) bool {
	return validGoVersion(value) && (strings.Count(value, ".") == 2 || version.Compare("go"+value, "go1.21") < 0)
}

// validToolchain 允许稳定官方工具链建议和明确禁用自动切换的 default。
func validToolchain(value string) bool {
	return value == "default" || (strings.HasPrefix(value, "go") && validSDKVersion(strings.TrimPrefix(value, "go")))
}

// compilerLanguage 从 helper 自身真实编译器推导语言上限，不维护产品 minor 白名单。
func compilerLanguage() string {
	return version.Lang(runtime.Version())
}

// requireCompilerLanguage 拒绝当前 parser/types 尚不能解释的语言版本。
func requireCompilerLanguage(required, source string) *analysisError {
	capability := compilerLanguage()
	if capability == "" || version.Compare(version.Lang("go"+required), capability) > 0 {
		return fail("unsupported_configuration", fmt.Sprintf("Go %s requires a newer analyzer compiler than %s", required, runtime.Version()), source)
	}
	return nil
}

// validateSDKVersion 统一检查包语言是否同时落在 helper 和实际 SDK 的能力范围内。
func (m *module) validateSDKVersion(sdk platform) *analysisError {
	if problem := requireCompilerLanguage(m.GoVersion, m.Source); problem != nil {
		return problem
	}
	if version.Compare("go"+m.GoVersion, "go"+sdk.Version) > 0 {
		return fail("unsupported_configuration", "package requires Go "+m.GoVersion+" newer than selected SDK "+sdk.Version, m.Source)
	}
	return nil
}

// languageVersionError 只提升公开类型错误中的版本诊断；普通缺失导入和不可用 API 仍保留未知关系。
func languageVersionError(err error) *types.Error {
	var diagnostic types.Error
	if !errors.As(err, &diagnostic) || !languageVersionMessage.MatchString(diagnostic.Msg) {
		return nil
	}
	return &diagnostic
}

// rejectDisabledAliasTypeParams 补回 Go 1.23 的实验门禁；新版 types 已删除此检查，不能让旧 SDK 默认开启泛型别名。
func rejectDisabledAliasTypeParams(sdk platform, files []*fileUnit) *analysisError {
	if version.Lang("go"+sdk.Version) != "go1.23" || containsString(sdk.ToolTags, "goexperiment.aliastypeparams") {
		return nil
	}
	for _, file := range files {
		found := false
		ast.Inspect(file.AST, func(node ast.Node) bool {
			declaration, ok := node.(*ast.TypeSpec)
			if ok && declaration.Assign.IsValid() && declaration.TypeParams.NumFields() > 0 {
				found = true
			}
			return !found
		})
		if found {
			return fail("object_extraction_failed", "generic type alias requires GOEXPERIMENT=aliastypeparams in the selected Go 1.23 SDK", file.Rel)
		}
	}
	return nil
}

// requiredSDKVersion 将生效的 go 与 toolchain 建议合并成最低标准库版本。
func (p *project) requiredSDKVersion() (string, string) {
	goVersion, toolchain, source := p.Modules[0].GoVersion, p.Modules[0].Toolchain, p.Modules[0].Source
	if p.WorkSource != "" {
		goVersion, toolchain, source = p.WorkGo, p.WorkToolchain, p.WorkSource
	}
	if toolchain != "" && toolchain != "default" && version.Compare(toolchain, "go"+goVersion) > 0 {
		goVersion = strings.TrimPrefix(toolchain, "go")
	}
	return goVersion, source
}

// selectSDKVersions 每个目标维度只保留声明 minor 内满足最低 patch 的最高稳定 SDK。
func selectSDKVersions(p *project, candidates []platform) ([]platform, *analysisError) {
	required, source := p.requiredSDKVersion()
	if problem := requireCompilerLanguage(required, source); problem != nil {
		return nil, problem
	}
	language := version.Lang("go" + required)
	best := map[platformMetadata]platform{}
	for _, candidate := range candidates {
		if version.Lang("go"+candidate.Version) != language || version.Compare("go"+candidate.Version, "go"+required) < 0 {
			continue
		}
		previous, found := best[candidate.Metadata]
		comparison := version.Compare("go"+candidate.Version, "go"+previous.Version)
		if !found || comparison > 0 || (comparison == 0 && candidate.ID < previous.ID) {
			best[candidate.Metadata] = candidate
		}
	}
	if len(best) == 0 {
		return nil, fail("target_unavailable", fmt.Sprintf("no approved %s SDK satisfies minimum Go %s", language, required), source)
	}
	result := make([]platform, 0, len(best))
	for _, candidate := range best {
		tags, err := sdkExperimentTags(candidate)
		if err != nil {
			return nil, fail("unsupported_configuration", fmt.Sprintf("Go %s SDK default build tags: %s", candidate.Version, err))
		}
		candidate.ToolTags = tags
		result = append(result, candidate)
	}
	sort.Slice(result, func(i, j int) bool { return result[i].ID < result[j].ID })
	return result, nil
}

// projectSelection 将独立 module 的选中文件与其静态上下文保持在同一个分析单元中。
type projectSelection struct {
	Project *project
	Files   []string
}

// analysisConfiguration 绑定一次分析使用的项目、文件和唯一 SDK，防止跨 module 混合上下文。
type analysisConfiguration struct {
	Project *project
	Files   []string
	SDK     platform
	ID      string
}

// loadProjectSelections 在 workspace 内共用上下文，在无 workspace 时隔离每个独立 module。
func loadProjectSelections(root string, selected []string) ([]projectSelection, *analysisError) {
	first, problem := loadProject(root, selected[:1])
	if problem != nil {
		return nil, problem
	}
	if first.WorkSource != "" {
		return []projectSelection{{Project: first, Files: selected}}, nil
	}
	groups := map[string]*projectSelection{}
	for _, rel := range selected {
		p, problem := loadProject(root, []string{rel})
		if problem != nil {
			return nil, problem
		}
		key := filepath.Clean(p.Modules[0].Dir)
		if groups[key] == nil {
			groups[key] = &projectSelection{Project: p}
		}
		groups[key].Files = append(groups[key].Files, rel)
	}
	keys := make([]string, 0, len(groups))
	for key := range groups {
		keys = append(keys, key)
	}
	sort.Strings(keys)
	result := make([]projectSelection, 0, len(keys))
	for _, key := range keys {
		result = append(result, *groups[key])
	}
	return result, nil
}
