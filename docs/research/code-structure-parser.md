# Code Structure View parser foundation

Task 1 for [specification 0018](../specs/0018-code-structure-changes.md) and
[ADR-0030](../adr/0030-encapsulate-code-structure-analysis.md). Investigation date:
2026-09-08. Starting repository revision: `ebfd8a7`.

## Dependency decision

Use `ast-grep-core`, `ast-grep-language`, and `ast-grep-outline` **0.45.1**.
Disable the language crate's default `builtin-parser` feature and enable only
`tree-sitter-javascript`, `tree-sitter-typescript`, and `tree-sitter-rust`.
The TypeScript grammar supplies both TS and TSX; JavaScript supplies JSX too.
Resolved versions in Cargo.lock: Tree-sitter **0.26.13**, JavaScript **0.25.0**,
TypeScript **0.23.2**, Rust **0.24.2**, tree-sitter-language **0.1.8**.

The chosen package provides reusable outline extraction and grammar C sources
packaged in ordinary crates. Narrant can supplement confirmed extraction gaps
while retaining one parse. This is an internal choice, not the public structure
analysis result or a promise of semantic call resolution.

## Candidate evidence

Both candidates were checked against official documentation and downloaded release
crate source, not inferred from language-support marketing:

| Candidate | Verified API / source | Integration consequence |
| --- | --- | --- |
| ast-grep-language 0.45.1 | `SupportLang`, `LanguageExt`, public `AstGrep::<StrDoc<SupportLang>>::try_new`; explicit grammar Cargo features; `src/parsers.rs` references grammar `LANGUAGE` constants | Four supported grammar modes can be statically compiled without runtime downloader or dynamic grammar discovery. Disabled languages panic if requested, so Narrant exposes only its four-language enum. |
| ast-grep-outline 0.45.1 | `parse_outline_rules`, `DEFAULT_OUTLINE_RULES`, `CombinedExtractors::try_from` / `extract`; `OutlineEntry`, `OutlineItem`, `OutlineMember` | Real reusable names, signatures, AST kinds, byte ranges, item/member grouping and import/export/public predicates. Its two-level model is evidence, not complete native hierarchy. |
| tree-sitter-language-pack 1.16.2 | `process(&str, &ProcessConfig) -> Result<ProcessResult, Error>`; `get_parser`; `StructureItem` includes native trait/impl kinds, nested children, spans/body spans/signatures | Stronger recursive structure shape, but field/type/constant coverage still needs adapters. No candidate supplies Narrant's project-wide call graph. |

In language-pack 1.16.2, `src/intel/intelligence.rs::structure_kind_at` handles
functions, arrows, methods, classes, structs, interfaces, enums, modules, traits
and impls. It does not include field declarations, type aliases or constants in
the structure matcher; some are separately categorized in symbols. This is
**source inspection, not a runtime coverage claim**. `StructureItem.decorators`
is explicitly reserved/unpopulated in that release.

Language-pack's default Cargo features enable dynamic loading and downloading.
Its `build.rs` accepts `TSLP_LANGUAGES` and a static link mode, but published
crate sources do not contain the selected grammar `parser.c` files. Build source
resolution can download a separate GitHub release archive and checksum. With
`TSLP_OFFLINE=1`, absent grammar sources are skipped. A prepared source bundle
can make that route offline; it is feasible, but adds packaging steps compared
with the selected grammar crates. No language-pack runtime comparison was run.

Authoritative references:

- [ast-grep language 0.45.1 API](https://docs.rs/ast-grep-language/0.45.1/ast_grep_language/)
- [ast-grep outline 0.45.1 API](https://docs.rs/ast-grep-outline/0.45.1/ast_grep_outline/)
- [ast-grep outline guide](https://ast-grep.github.io/guide/outline-code)
- [language-pack 1.16.2 published source](https://docs.rs/crate/tree-sitter-language-pack/1.16.2/source/)
- [language-pack static building guide](https://docs.tree-sitter-language-pack.xberg.io/guides/building/)

Downloaded official archives (SHA-256):

| Package | SHA-256 |
| --- | --- |
| ast-grep-core 0.45.1 | `dc04b01428d8ff8e2c22f888a69115af6e3921b5c20f99d85d3bc838cc2b6f18` |
| ast-grep-language 0.45.1 | `d9ebe4f04dceadc722936dbdf5819ce05bba78507ae81aa34781d94e4e7bd840` |
| ast-grep-outline 0.45.1 | `38cb150a6850fe4d28f5e116bd6966302db5349c4ff0db16f1969bacd530b96d` |
| tree-sitter-language-pack 1.16.2 | `d9966122c508f2a80493c23de4e95971720b1c26015c2153539ca60a433d2ebc` |

Archives were retrieved from `https://static.crates.io/crates/{name}/{name}-{version}.crate`.
The initial sandbox request could not resolve crates.io; escalated crates.io API
download returned HTTP 403. Official static downloads succeeded. The first
Cargo index request briefly timed out, then resolved and downloaded dependencies.

## Scope to keep behind the parser boundary

The parser foundation must retain the original text and a provider-independent
syntax arena (including unnamed tokens, named fields, parent links and recovered
nodes). Later extraction can recover syntax the outline skipped without opening
files again or parsing in the UI. Native AST kinds and outline categories are
different evidence: e.g. `trait_item` and `impl_item` must not become interface
and object merely because the provider uses those outline categories.

An outline entry's signature is provider text, sometimes only its first nonempty
line. It is not a canonical declaration signature, formatting-insensitive body,
documentation association or proof of unchanged content. Byte ranges are
half-open, source lines are zero-based, and byte columns must be distinguished
from provider character columns and Monaco UTF-16 columns.

Likewise, an empty outline does not prove absence. Syntax diagnostics are retained
independently of extracted entries, including unnamed zero-width MISSING tokens.
An error-free subtree does not establish trustworthy lexical ownership when an
enclosing scope is damaged. Snapshot side, unavailable content, cancellation and
resource budgets belong to the orchestration contract in subsequent tasks.

## Observed extraction coverage and Task 3 gaps

These observations are from the production module's real tests, using the pinned
dependencies. They describe the provider evidence, not final product coverage.

| Fixture | Observed outline | Required subsequent adaptation |
| --- | --- | --- |
| TS class / field / method / free function / top-level arrow | `Box`, `value`, `read`, `helper`, `arrow`; native class/field/method/function/variable-declarator kinds and exact source ranges | Derive canonical native declaration kinds and signatures from retained syntax. |
| TSX `function Card` and `export const Label = () => <span/>` | Card is Function; Label is **Constant**, matched on `export_statement` | Unwrap export syntax, recognize named arrow as function, retain true declaration and export ranges. |
| JS import alias, exported class/method and JSX arrow | Import source `'./runner'`, Box, read and arrow; import/export/public flags retained | Resolve the actual import bindings and calls in Task 4; provider import text is not a resolved target. |
| Rust struct, trait, trait impl and inherent impl | `struct_item` / Struct, `trait_item` / **Interface**, `impl_item` / **Object**; methods retain distinct parents and ranges | Native trait/impl classification, impl trait/type paths and multi-file type aggregation. Do not merge by provider name or category. |
| TS exported arrow / type Alias / outer(inner) / namespace N(nested) | Arrow = Constant, Alias = **Struct**; outer and N are listed; inner and namespace nested are omitted | Correct native type/arrow kinds; build recursive lexical hierarchy; supplement skipped nested functions and namespace contents. |
| Rust module with struct field, impl method, outer(inner), type Alias and FLAG | Only module, direct struct and direct outer function are listed | Supplement module-local impl/method, struct field, nested function, type alias and constant. All corresponding syntax remains available. |
| Rust invalid initializer plus missing semicolon | Separate ERROR and zero-width MISSING `;`, with good function retained | Carry file/side/coverage and damaged-scope uncertainty forward; never infer declaration absence from omitted evidence. |

The Rust unclosed-function-at-EOF probe recovered as ERROR, not MISSING. The
missing-semicolon fixture was verified to produce MISSING. Tests assert actual
grammar behavior rather than assuming a specific recovery node for all damage.

Provider signatures in the TS/JS one-line fixtures include bodies; Rust signatures
are bodyless in these fixtures. Native source ranges and children permit later
body/signature/documentation separation without reparsing. No call graph,
inheritance resolution, semantic type resolution, matching, formatting comparison,
or documentation association is implemented in this foundation.

## Owned API supplied to later tasks

`review::structure::parse_source(SourceLanguage, &str) -> Result<ParsedSource, ParseFailure>`
accepts in-memory text. The result owns source text, language, provider identity,
all syntax nodes, outline evidence, and independent syntax diagnostics.

- `SyntaxNode`: native kind, UTF-8 source range, parent/children indices, named
  field, named/extra/error/missing flags and subtree error flag. Index 0 is the
  file root. Traversal is iterative; no native recursion is added.
- `OutlineEvidence`: provider category/name/signature, original matched native
  kind, source range, syntax-node index, item/member role and parent, plus all
  supplied import/export/public flags. Member-only and item-only flags remain
  `None` where the provider does not supply them.
- `SyntaxDiagnostic`: Error or Missing, node index and source range. A missing
  token is never inserted into owned source text.
- `ParseFailure`: parser failure, rule compilation failure, or inconsistent
  provider evidence. Recoverable source diagnostics are returned with results.

Rules compile once per language per analysis thread. The entire third-party AST
and extractor handles stay in `parser.rs`; downstream code consumes owned data.
Snapshot identity and side are attached by the source/orchestration layer.

## Offline and build evidence

On macOS aarch64 with rustc 1.97.1, the **actual production module** was compiled
as a test binary against the Cargo-built, locked parser rlibs; no copied source,
alternate implementation or additional Cargo project was used. Eight tests passed
normally (1.17 s) and all eight passed in a new process with explicit network
denial (1.74 s). This is runtime network isolation, not merely Cargo `--offline`.

```sh
scripts/cargo-agent.sh build --manifest-path src-tauri/Cargo.toml -p ast-grep-outline -p ast-grep-language -j 2
```

Build passed in 1m 58s. Cargo's language fingerprint lists exactly
`tree-sitter-javascript`, `tree-sitter-rust`, `tree-sitter-typescript`, with no
`default` / `builtin-parser` feature. Each grammar crate includes its C parser
and a `cc::Build` script. Binary `nm` confirmed all four local text symbols:

```text
T _tree_sitter_javascript
T _tree_sitter_rust
T _tree_sitter_tsx
T _tree_sitter_typescript
```

Production module test compilation (paths identify the actual built artifacts):

```sh
lockf -k /Users/lizhanfa/projects/narrant/.git/narrant-cargo-agent.lock node scripts/rust-cache.mjs run sccache rustc --edition 2021 --test src-tauri/src/review/structure/mod.rs -L dependency=src-tauri/target/debug/deps --extern ast_grep_core=src-tauri/target/debug/deps/libast_grep_core-d7f8fdb8f4714f7b.rlib --extern ast_grep_language=src-tauri/target/debug/deps/libast_grep_language-8e79b193ab8b7a38.rlib --extern ast_grep_outline=src-tauri/target/debug/deps/libast_grep_outline-3d4652369f772d73.rlib -o /tmp/narrant-parser-tests
/tmp/narrant-parser-tests --nocapture --test-threads=1
/usr/bin/sandbox-exec -p '(version 1) (allow default) (deny network*)' /tmp/narrant-parser-tests --test-threads=1
```

Network control: curl reached the official index outside the profile (HTTP 200),
but failed to resolve it under the same deny-network profile (exit 6).
The unoptimized standalone test binary is 19,378,552 bytes; this is **not** the
incremental size of the packaged Narrant application. Windows packaging and
Narrant-scale time/peak-memory/cancellation have not been measured here.

The initial full-application Cargo test/check attempts were interrupted during
cold Tauri/native dependency compilation under observed machine memory pressure.
They did not produce application-level pass/fail results. The controller approved
the locked, sequential production-module validation above to unblock downstream
work. Full application `check --lib`, focused Cargo tests, and the once-per-Issue
full suite remain part of coordinated integration validation; the standalone
results do not replace them. No target sharing, incremental disabling, cache
clearing, concurrent Cargo, or manual quality-gate occurred.
