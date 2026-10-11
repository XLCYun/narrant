# Bounded pre-acceptance cleanup report

Status: DONE. Preparation only; Task7 retirement, integration and ordered platform/production acceptance remain with the controller.
Base: `fc63fd0cc43801097326ddeb403749fd0e6790d1`.

## Implementation and self-review

Moved the configDir fixture unchanged (byte equality checked against HEAD) to the neutral `structure_review/tests/config_dir_fixture.rs`, declared once under `#[cfg(test)]` in `structure_review/mod.rs`. All three native/legacy consumers now refer to that single module. All consumer business assertions remain intact. Fixture visibility remains `pub(super)` and no framework was introduced.

Collapsed the two nested Host input conditions using equivalent `&&`/parenthesized `||` expressions. Outer kind/profile gating and Python length short circuit still precede metadata/platform indexing; errors and admission policy are unchanged. Removed only the four unreachable cancel-mode lines from the descendant timer. Actual cancel branches remain unchanged.

Self-reviewed the full diff: no engine/graph/protocol/process-owner/factory/pack/catalog change, old chain retirement, lint allowance, expected-panic noise change, new unnecessary tests, or unrelated formatting. No concerns about correctness found.

## Environment and commands

Worktree: `/Users/lizhanfa/.codex/worktrees/migrate-existing-native-helpers/narrant`.
Every Cargo command used `scripts/cargo-agent.sh`, serialized, with:

```sh
export PATH=/Users/lizhanfa/.nvm/versions/node/v24.18.0/bin:$PATH
export NARRANT_TEST_NODE=/Users/lizhanfa/.nvm/versions/node/v24.18.0/bin/node
export NARRANT_TYPESCRIPT_PACK="$PWD/.superpowers/sdd/0025a-existing-native-helper-implementation-plan/task-4-artifacts-lifecycle-r4-verified/smoke-installed/typescript-javascript/1.1.0/aarch64-apple-darwin"
```

Used the existing candidate and fixed Node24; no downloads or helper rebuilds.

```sh
scripts/cargo-agent.sh fmt --manifest-path src-tauri/Cargo.toml
scripts/cargo-agent.sh clippy --manifest-path src-tauri/Cargo.toml --lib --tests --locked -- -D warnings
```

Both exit 0. Raw strict Clippy output:

```text
   Compiling narrant v1.2.14 (/Users/lizhanfa/.codex/worktrees/migrate-existing-native-helpers/narrant/src-tauri)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 18.13s
```

Each focused test command/output follows; all seven actual selections pass one test each:

```sh
scripts/cargo-agent.sh test --manifest-path src-tauri/Cargo.toml --lib --locked structure_review::native_analyzer::tests::input_rejects_unsafe_paths_platforms_and_tool_roots -- --exact
```

```text
   Compiling narrant v1.2.14 (/Users/lizhanfa/.codex/worktrees/migrate-existing-native-helpers/narrant/src-tauri)
    Finished `test` profile [unoptimized + debuginfo] target(s) in 19.37s
     Running unittests src/lib.rs (src-tauri/target/debug/deps/narrant_lib-d79c0579e6c743d1)

running 1 test
test structure_review::native_analyzer::tests::input_rejects_unsafe_paths_platforms_and_tool_roots ... ok

test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 946 filtered out; finished in 0.01s

```

```sh
scripts/cargo-agent.sh test --manifest-path src-tauri/Cargo.toml --lib --locked structure_review::native_analyzer::tests::clang_catalog_requires_exact_approved_additional_header_ids -- --exact
```

```text
    Finished `test` profile [unoptimized + debuginfo] target(s) in 1.83s
     Running unittests src/lib.rs (src-tauri/target/debug/deps/narrant_lib-d79c0579e6c743d1)

running 1 test
test structure_review::native_analyzer::tests::clang_catalog_requires_exact_approved_additional_header_ids ... ok

test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 946 filtered out; finished in 0.03s

```

```sh
scripts/cargo-agent.sh test --manifest-path src-tauri/Cargo.toml --lib --locked structure_review::native_analyzer::tests::migration::embedded_python_stdlib_is_strict_and_tied_to_the_helper -- --exact
```

```text
    Finished `test` profile [unoptimized + debuginfo] target(s) in 0.53s
     Running unittests src/lib.rs (src-tauri/target/debug/deps/narrant_lib-d79c0579e6c743d1)

running 1 test
test structure_review::native_analyzer::tests::migration::embedded_python_stdlib_is_strict_and_tied_to_the_helper ... ok

test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 946 filtered out; finished in 0.01s

```

```sh
scripts/cargo-agent.sh test --manifest-path src-tauri/Cargo.toml --lib --locked structure_review::native_analyzer::tests::subprocess_cooperative_and_forced_cancellation -- --exact
```

```text
    Finished `test` profile [unoptimized + debuginfo] target(s) in 0.65s
     Running unittests src/lib.rs (src-tauri/target/debug/deps/narrant_lib-d79c0579e6c743d1)

running 1 test
test structure_review::native_analyzer::tests::subprocess_cooperative_and_forced_cancellation ... ok

test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 946 filtered out; finished in 0.48s

```

```sh
scripts/cargo-agent.sh test --manifest-path src-tauri/Cargo.toml --lib --locked structure_review::native_analyzer::tests::subprocess_cancel_preserves_observed_backend_and_protocol_failures -- --exact
```

```text
    Finished `test` profile [unoptimized + debuginfo] target(s) in 0.47s
     Running unittests src/lib.rs (src-tauri/target/debug/deps/narrant_lib-d79c0579e6c743d1)

running 1 test
test structure_review::native_analyzer::tests::subprocess_cancel_preserves_observed_backend_and_protocol_failures ... ok

test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 946 filtered out; finished in 0.15s

```

```sh
scripts/cargo-agent.sh test --manifest-path src-tauri/Cargo.toml --lib --locked structure_review::native_analyzer::tests::subprocess_reaps_descendants_after_parent_exit -- --exact
```

```text
    Finished `test` profile [unoptimized + debuginfo] target(s) in 0.49s
     Running unittests src/lib.rs (src-tauri/target/debug/deps/narrant_lib-d79c0579e6c743d1)

running 1 test
test structure_review::native_analyzer::tests::subprocess_reaps_descendants_after_parent_exit ... ok

test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 946 filtered out; finished in 0.49s

```

```sh
scripts/cargo-agent.sh test --manifest-path src-tauri/Cargo.toml --lib --locked structure_review::native_languages::typescript_graph::graph_config_dir_templates_retain_local_call -- --exact --ignored
```

```text
    Finished `test` profile [unoptimized + debuginfo] target(s) in 0.46s
     Running unittests src/lib.rs (src-tauri/target/debug/deps/narrant_lib-d79c0579e6c743d1)

running 1 test
test structure_review::native_languages::typescript_graph::graph_config_dir_templates_retain_local_call ... ok

test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 946 filtered out; finished in 1.36s

```

Additional checks (all exit 0, no output):

```sh
node node_modules/eslint/bin/eslint.js --no-ignore --global process --global Buffer --global setInterval --global clearInterval --global setTimeout --max-warnings 0 src-tauri/src/structure_review/native_analyzer/tests/fake-helper.mjs
scripts/cargo-agent.sh fmt --manifest-path src-tauri/Cargo.toml -- --check
git diff --check
cmp src-tauri/src/structure_review/tests/config_dir_fixture.rs <(git show HEAD:src-tauri/src/structure_review/typescript_analyzer/tests/config_dir_fixture.rs)
```

Validation invocation corrections: the initial exact native graph selector incorrectly included `::tests::` and selected zero tests (exit 0); the runner explicitly rejected the zero-test output. After reading the module declaration, the corrected selector above ran one passing test. Initial scoped ESLint invocation without `--no-ignore` produced a repository ignore warning; retry with explicit Node globals initially omitted `setTimeout` and reported two no-undef errors at unchanged lines37/42. The final invocation above supplies all used Node globals and passes with zero warnings. These were invocation configuration issues, requiring no source changes or suppressions.

## Skips and limits

Per explicit dispatch/brief restriction, no full Host suite, helper semantic suite, old runtime rebuild, Windows execution, Task7 final acceptance, UI browser checks (no frontend change), subagents/reviewers, push, Actions, quality-gate, PR or merge. macOS ARM64 focused native configDir behavior was actually exercised; Windows-specific descendant observation branches were not executable on this macOS host. Legacy consumers were compiled by strict Clippy, with unchanged fixture data and assertions, as required. No expected-panic test/output-polish work was attempted. TDD was not requested; existing meaningful tests cover this equivalent cleanup.

## Files changed

- `src-tauri/src/structure_review/mod.rs`
- `src-tauri/src/structure_review/tests/config_dir_fixture.rs` (100% unchanged move from `typescript_analyzer/tests/config_dir_fixture.rs`)
- `src-tauri/src/structure_review/typescript_analyzer/mod.rs`
- `src-tauri/src/structure_review/typescript_analyzer/tests/graph_service.rs`
- `src-tauri/src/structure_review/typescript_analyzer/tests/real_service.rs`
- `src-tauri/src/structure_review/native_languages/tests/typescript_graph.rs`
- `src-tauri/src/structure_review/native_analyzer/input.rs`
- `src-tauri/src/structure_review/native_analyzer/tests/fake-helper.mjs`
- This report.
