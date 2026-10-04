package main

import (
	"bufio"
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"os"
	"reflect"
	"strings"
)

const protocol = "narrant.native-analyzer/1"

// request 是一次分析使用的完整协议输入。
type request struct {
	Protocol    string      `json:"protocol"`
	Type        string      `json:"type"`
	ID          string      `json:"id"`
	Input       input       `json:"input"`
	Environment environment `json:"environment"`
	Limits      limits      `json:"limits"`
}

// input 描述固定侧仓库和选中文件。
type input struct {
	RepositoryRoot string   `json:"repository_root"`
	Files          []string `json:"files"`
	IncludeTests   bool     `json:"include_tests"`
	Profile        string   `json:"profile"`
}

// environment 只包含受批准的平台声明和隔离 scratch 目录。
type environment struct {
	ScratchRoot string     `json:"scratch_root"`
	Platforms   []platform `json:"platforms"`
}

// platform 指定目标 Go SDK 与一个目标系统架构。
type platform struct {
	ID       string           `json:"id"`
	Kind     string           `json:"kind"`
	Version  string           `json:"version"`
	Root     string           `json:"root"`
	Metadata platformMetadata `json:"metadata"`
}

// platformMetadata 明确列出受支持的目标维度。
type platformMetadata struct {
	GOOS    string `json:"goos"`
	GOARCH  string `json:"goarch"`
	GOAMD64 string `json:"goamd64,omitempty"`
}

// limits 限制单次分析的时间和输出大小。
type limits struct {
	AnalysisTimeoutMS  int `json:"analysis_timeout_ms"`
	MaxResultBytes     int `json:"max_result_bytes"`
	MaxDiagnosticBytes int `json:"max_diagnostic_bytes"`
}

// node 是公共图中一个有单一声明父级的源码对象。
type node struct {
	Category   string         `json:"category"`
	ID         string         `json:"id"`
	Language   string         `json:"language"`
	NativeKind string         `json:"native_kind"`
	Name       *string        `json:"name"`
	Signature  *string        `json:"signature"`
	Code       *string        `json:"code"`
	Source     *string        `json:"source"`
	Contains   []string       `json:"contains"`
	References []reference    `json:"references"`
	Extensions map[string]any `json:"extensions"`
}

// reference 是一个已确认或保守未知的关系观察。
type reference struct {
	Semantic string `json:"semantic"`
	Target   target `json:"target"`
}

// target 用 kind 区分图内、图外和未知目标。
type target struct {
	Kind    string `json:"kind"`
	ID      string `json:"id,omitempty"`
	Symbol  string `json:"symbol,omitempty"`
	Name    string `json:"name,omitempty"`
	Scope   string `json:"-"`
	Display string `json:"-"`
}

// externalSymbol 描述图外但已确认身份的仓内或 SDK 符号。
type externalSymbol struct {
	Symbol string `json:"symbol"`
	Name   string `json:"name"`
	Scope  string `json:"scope"`
}

// graph 是 helper 已完成的图构造数据。
type graph struct {
	RootContains    []string         `json:"root_contains"`
	Nodes           []node           `json:"nodes"`
	ExternalSymbols []externalSymbol `json:"external_symbols"`
}

// configurationEvidence 记录实际分析的配置及来源。
type configurationEvidence struct {
	ID          string   `json:"id"`
	SourcePaths []string `json:"source_paths"`
	PlatformIDs []string `json:"platform_ids"`
}

// fileEvidence 记录输入文件在配置中的覆盖结果。
type fileEvidence struct {
	Path             string   `json:"path"`
	Disposition      string   `json:"disposition"`
	ConfigurationIDs []string `json:"configuration_ids"`
}

// originEvidence 记录图对象被哪些选中源码支持。
type originEvidence struct {
	NodeID        string   `json:"node_id"`
	SelectedPaths []string `json:"selected_paths"`
}

// evidence 汇总完整性可交叉检验的证据。
type evidence struct {
	Configurations []configurationEvidence `json:"configurations"`
	Files          []fileEvidence          `json:"files"`
	Origins        []originEvidence        `json:"origins"`
}

// analysisError 是协议允许的原子失败。
type analysisError struct {
	Code    string   `json:"code"`
	Message string   `json:"message"`
	Paths   []string `json:"paths"`
}

// completed 是分析的唯一终结消息。
type completed struct {
	Protocol string         `json:"protocol"`
	Type     string         `json:"type"`
	ID       string         `json:"id"`
	Outcome  string         `json:"outcome"`
	Graph    *graph         `json:"graph,omitempty"`
	Evidence *evidence      `json:"evidence,omitempty"`
	Error    *analysisError `json:"error,omitempty"`
}

// fail 创建带明确路径的原子分析错误。
func fail(code, message string, paths ...string) *analysisError {
	return &analysisError{Code: code, Message: message, Paths: append([]string{}, paths...)}
}

// strictJSON 拒绝未知字段和递归位置上的重复 JSON 键。
func strictJSON(data []byte, out any) error {
	decoder := json.NewDecoder(bytes.NewReader(data))
	if err := uniqueValue(decoder); err != nil {
		return err
	}
	decoder = json.NewDecoder(bytes.NewReader(data))
	decoder.DisallowUnknownFields()
	if err := decoder.Decode(out); err != nil {
		return err
	}
	if decoder.Decode(new(any)) != io.EOF {
		return errors.New("trailing JSON value")
	}
	return nil
}

// uniqueValue 递归检查对象键唯一，同时保持标准解码器对数字及转义的判断。
func uniqueValue(decoder *json.Decoder) error {
	tok, err := decoder.Token()
	if err != nil {
		return err
	}
	delim, ok := tok.(json.Delim)
	if !ok {
		return nil
	}
	if delim == '{' {
		seen := map[string]bool{}
		for decoder.More() {
			keyTok, err := decoder.Token()
			if err != nil {
				return err
			}
			key, ok := keyTok.(string)
			if !ok {
				return errors.New("invalid JSON key")
			}
			if seen[key] {
				return fmt.Errorf("duplicate JSON key %q", key)
			}
			seen[key] = true
			if err := uniqueValue(decoder); err != nil {
				return err
			}
		}
	} else if delim == '[' {
		for decoder.More() {
			if err := uniqueValue(decoder); err != nil {
				return err
			}
		}
	} else {
		return errors.New("unexpected JSON delimiter")
	}
	_, err = decoder.Token()
	return err
}

// requiredFields 使缺失字段与零值字段得到不同的协议结果。
func requiredFields(data []byte, fields ...string) error {
	var raw map[string]json.RawMessage
	if err := json.Unmarshal(data, &raw); err != nil {
		return err
	}
	for _, field := range fields {
		if _, ok := raw[field]; !ok {
			return fmt.Errorf("missing %s", field)
		}
	}
	return nil
}

// readLines 独立于分析任务读取取消信号，EOF 会取消正在进行的任务。
func readLines(reader io.Reader, lines chan<- []byte, gone chan<- struct{}) {
	defer close(lines)
	defer close(gone)
	buf := bufio.NewReader(reader)
	for {
		line, err := buf.ReadBytes('\n')
		if err != nil {
			return
		}
		if len(line) > 4*1024*1024 {
			return
		}
		lines <- bytes.TrimSuffix(line, []byte{'\n'})
	}
}

// runProtocol 严格执行 ready、一次 analyze、一次 completed 的生命周期。
func runProtocol(in io.Reader, out io.Writer) error {
	encoder := json.NewEncoder(out)
	if err := encoder.Encode(map[string]any{"protocol": protocol, "type": "ready", "helper_id": "narrant-go", "helper_version": "0.1.0", "projection_revision": "go-source-v1", "languages": []string{"go"}, "profiles": []string{"go-static-v1"}}); err != nil {
		return err
	}
	lines, gone := make(chan []byte, 2), make(chan struct{})
	go readLines(in, lines, gone)
	first, ok := <-lines
	if !ok {
		return errors.New("stdin closed before analyze")
	}
	var req request
	if err := strictJSON(first, &req); err != nil {
		return fmt.Errorf("invalid analyze: %w", err)
	}
	if err := requiredFields(first, "protocol", "type", "id", "input", "environment", "limits"); err != nil {
		return err
	}
	if req.Protocol != protocol || req.Type != "analyze" || strings.TrimSpace(req.ID) == "" {
		return errors.New("invalid analyze envelope")
	}
	if err := checkRequiredNested(first); err != nil {
		return err
	}
	ctx, cancel := context.WithCancel(context.Background())
	defer cancel()
	result := make(chan completed, 1)
	go func() { result <- analyze(ctx, req) }()
	for {
		select {
		case message, ok := <-lines:
			if !ok {
				cancel()
				<-result
				return errors.New("stdin closed during analyze")
			}
			var control struct {
				Protocol string `json:"protocol"`
				Type     string `json:"type"`
				ID       string `json:"id"`
			}
			if err := strictJSON(message, &control); err != nil {
				cancel()
				<-result
				return err
			}
			if err := requiredFields(message, "protocol", "type", "id"); err != nil {
				cancel()
				<-result
				return err
			}
			if control.Protocol != protocol || control.Type != "cancel" || control.ID != req.ID {
				cancel()
				<-result
				return errors.New("invalid cancel")
			}
			cancel()
		case response := <-result:
			if ctx.Err() != nil && response.Outcome == "ok" {
				response = completed{Protocol: protocol, Type: "completed", ID: req.ID, Outcome: "cancelled"}
			}
			return encoder.Encode(response)
		case <-gone:
			cancel()
			<-result
			return errors.New("stdin closed during analyze")
		}
	}
}

// checkRequiredNested 检查协议对象每一级必需字段的存在性。
func checkRequiredNested(data []byte) error {
	var root map[string]json.RawMessage
	if err := json.Unmarshal(data, &root); err != nil {
		return err
	}
	for field, names := range map[string][]string{
		"input":       {"repository_root", "files", "include_tests", "profile"},
		"environment": {"scratch_root", "platforms"},
		"limits":      {"analysis_timeout_ms", "max_result_bytes", "max_diagnostic_bytes"},
	} {
		if err := requiredFields(root[field], names...); err != nil {
			return err
		}
	}
	var env map[string]json.RawMessage
	_ = json.Unmarshal(root["environment"], &env)
	var platforms []json.RawMessage
	_ = json.Unmarshal(env["platforms"], &platforms)
	for _, p := range platforms {
		if err := requiredFields(p, "id", "kind", "version", "root", "metadata"); err != nil {
			return err
		}
	}
	return nil
}

// stringPtr 将值型字符串用于可空的图字段。
func stringPtr(value string) *string { return &value }

// sameNodeContent 比较跨配置同身份对象的稳定自身内容。
func sameNodeContent(a, b node) bool {
	return a.Category == b.Category && a.Language == b.Language && a.NativeKind == b.NativeKind && reflect.DeepEqual(a.Name, b.Name) && reflect.DeepEqual(a.Signature, b.Signature) && reflect.DeepEqual(a.Code, b.Code) && reflect.DeepEqual(a.Source, b.Source) && reflect.DeepEqual(a.Extensions, b.Extensions)
}

// main 只通过 stdout 输出协议消息，异常写入 stderr 后退出。
func main() {
	if err := runProtocol(os.Stdin, os.Stdout); err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(1)
	}
}
