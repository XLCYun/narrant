# Go native analyzer helper

This standalone module implements `narrant.native-analyzer/1` over stdin/stdout NDJSON. The release build pins its compiler independently of project targets. The helper's language capability comes from its actual compiler (`runtime.Version()` and `go/version`), rather than a product Go-version allowlist. Analysis never invokes the Go command, a project script, a dependency, or a shell.

Build and test with the compiler pinned by the support-pack build configuration:

```sh
cd native-helpers/go
GOTOOLCHAIN=local GOPROXY=off "$GO_COMPILER_ROOT/bin/go" test ./...
GOTOOLCHAIN=local GOPROXY=off "$GO_COMPILER_ROOT/bin/go" build -o bin/narrant-go .
```

For Windows x64, build `bin/narrant-go.exe` with the same pinned compiler. The executable takes no arguments. It emits one `ready`, reads one `analyze`, emits one `completed`, and exits without waiting for stdin EOF. A separate stdin reader accepts a matching `cancel` while analysis runs. stdout contains protocol JSON; fatal protocol diagnostics go to stderr.

The Host provides approved, locally installed `go-sdk` entries. Each entry records its actual stable version (for example `1.23.8`), an absolute SDK root containing the matching `VERSION` and complete `src/` tree, and metadata `{ "goos": "darwin|linux|windows", "goarch": "arm64|amd64", "goamd64": "v1" }`. `goamd64` is required only for amd64. The executable needs no installed Go runtime at launch; SDK source is target analysis data and is not bundled with the helper. The Host can supply several installed minor versions and keeps the highest stable patch of each minor.

Version selection follows the repository's static declarations:

- Without `go.work`, each selected independent module receives its own context and SDK selection. Selecting several modules does not make them an implicit workspace.
- A workspace uses its `go` declaration and its non-`default` `toolchain` suggestion. The effective minimum is the higher of those versions; module `toolchain` suggestions do not override the workspace. The workspace's `go` version must be at least every used module's `go` version.
- A standalone module applies the same rule to its own `go.mod`. `toolchain default` adds no version suggestion: this profile uses the declared `go` version and does not read or emulate the user's `GOTOOLCHAIN` settings.
- For each OS/architecture, selection uses the highest approved stable patch in the effective minor, provided it meets the exact minimum patch. It never borrows a newer minor when that target is absent. Duplicate SDK entries do not create duplicate configurations.
- Missing minor/patch targets return `target_unavailable`. A required language newer than the helper's actual compiler returns `unsupported_configuration`. SDK patches within the compiler's supported minor share its language capability.
- Each repository package is checked with its own module's `go` language version, including explicit local replacement modules. Standard-library packages use the selected SDK's version. Thus a newer workspace SDK does not upgrade older modules' language rules.

File selection uses the selected SDK's release tags, including correct handling of guards for future releases. The helper also statically reads that SDK's default experiment baseline, so the compiler used to build the helper cannot inject its own experiment tags into an older target. This reader accepts only boolean/string declarations, comparisons, and target switches preceding the SDK's baseline literal. Unknown shapes or customized default experiments fail explicitly. It does not execute SDK code. cgo and user-supplied custom build tags remain disabled.

The loader reads the repository's `go.mod`/`go.work`, workspace modules, and unversioned local directory `replace` targets inside the repository, plus approved SDK standard-library sources. It never consults the module cache, GOPATH, network, `go list`, or compiler export data. Third-party dependency source remains excluded even when present on disk. Missing third-party imports and APIs unavailable in the selected SDK can leave relationships unresolved while complete source declarations remain in the graph. A used local replacement must provide its own readable `go.mod` so its package language version is known.

Source that violates its package's effective Go language version returns `object_extraction_failed` with no graph, including violations in unselected files and imported repository package bodies. This uses the pinned compiler's public `types.Error.Msg` version diagnostics, without accessing private error codes. The selected Go 1.23 SDK also retains its disabled-by-default generic-alias experiment; newer SDKs retain their native compatibility rules. A supported per-file release guard can still raise the file's effective language version.

Without explicit platform constraints, the helper analyzes its native GOOS/GOARCH baseline. Otherwise it inspects the admitted package/import closure, including unselected platform files, and evaluates supported Boolean `//go:build` / legacy `+build` conditions against the catalog. Every selected non-test file must be active in an analyzed target. External test packages, vendor, embed, custom tags, implicit nested modules, and ambiguous configuration remain outside this static profile. Workspace `replace` wins over module `replace`; conflicting module replacements without an override fail.

The tests use actual SDK `VERSION` files; they never relabel one SDK source tree as another version. General graph tests use the test compiler's real SDK. Cross-minor regressions require a real Go 1.23 installation and a second installation at Go 1.24 or later that the helper compiler supports. Provide their roots using the platform's path-list separator:

```sh
NARRANT_GO_TEST_SDKS="/path/to/go1.23.8:/path/to/go1.27.1" \
  GOTOOLCHAIN=local GOPROXY=off "$GO_COMPILER_ROOT/bin/go" test -v ./...
```

The tests also inspect `runtime.GOROOT()` and `/usr/local/go` when present. If real cross-minor targets are absent, those cases explicitly skip; that is not evidence of cross-version support. They cover SDK patch minimums, missing/future targets, `toolchain`/workspace precedence, per-package language rules, independent modules, old-SDK API absence, release guards, and default experiment isolation. The previous Go 1.23.8 validation record remains [historical implementation evidence](../../docs/research/go-native-implementation.md); it does not establish acceptance for an untested SDK or host platform.
