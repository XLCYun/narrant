# Go native analyzer helper

This standalone Go 1.23.8 module implements `narrant.native-analyzer/1` over stdin/stdout NDJSON. Build it ahead of analysis; the helper never invokes the Go command or project build scripts while analyzing a repository.

```sh
cd native-helpers/go
GOTOOLCHAIN=local GOPROXY=off /usr/local/go/bin/go test ./...
GOTOOLCHAIN=local GOPROXY=off /usr/local/go/bin/go build -o bin/narrant-go .
```

For Windows x64, build `bin/narrant-go.exe` with Go 1.23.8. The executable takes no arguments. It emits one `ready` message, reads one `analyze`, emits one `completed`, and exits without waiting for stdin EOF. A separate stdin reader accepts a matching `cancel` while analysis runs. stdout carries only protocol JSON; fatal protocol diagnostics go to stderr.

The Host provides an approved `go-sdk` catalog. Each entry uses `version: "1.23.8"`, a nonempty fingerprint, an absolute SDK root containing `VERSION` and the complete `src/` tree, and metadata `{ "goos": "darwin|linux|windows", "goarch": "arm64|amd64", "goamd64": "v1" }`. `goamd64` is required only for amd64. The helper's executable does not require a Go runtime at launch; the target SDK sources are analysis data. The helper inspects all allowed files in selected packages, including unselected platform files, and evaluates supported Boolean `//go:build` / legacy `+build` conditions against each catalog target. Without explicit platform constraints, the profile analyzes the helper's native GOOS/GOARCH baseline. Every selected non-test file must be active in at least one analyzed target.

The static loader reads `go.mod` and `go.work` from the fixed repository, admits only workspace modules and unversioned local directory `replace` targets inside the same repository, and recursively reads approved target SDK standard-library source. It never consults module cache, GOPATH, network, `go list`, compiler export data, or a shell. It excludes third-party dependency source even when present on disk. Missing third-party imports can leave call observations unresolved while complete source declarations remain in the graph.

The verified profile is intentionally finite: Go 1.23 module/workspace declarations, local module imports, cgo off, no custom build tags, no external test packages, and no vendor or embed processing. Workspace `replace` wins over module `replace`; conflicting module replacements without an override and implicit nested modules are rejected. Unknown necessary configuration returns an error. The current implementation was validated locally on Go 1.23.8 / macOS arm64; Windows native behavior remains a separate CI acceptance gate. See [implementation evidence](../../docs/research/go-native-implementation.md).
