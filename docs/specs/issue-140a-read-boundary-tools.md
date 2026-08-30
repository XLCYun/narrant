# Issue 140A: Repository Tool Contract and Read Boundary

## Objective

Create the reusable `babysit::repository_tools` public seam and implement the Read tool.

## Public seam

- `RepositoryTools::new(workspace_root)` canonicalizes the Review Workspace once.
- A schema-bearing registration view exposes exactly `read`, `glob`, and `grep`.
- The execution API accepts typed Read input plus `CancellationSignal` and returns a typed,
  serializable response containing workspace-relative identity, content, continuation, and
  explicit truncation.
- Stable tool errors never contain the absolute workspace path.

## Required behavior

- Resolve relative, absolute, and parent-component inputs only when their canonical target is
  contained by the canonical workspace root using component-aware containment.
- Support regular text files and directories; reject missing targets and binary/non-text files
  with stable redacted errors.
- Permit symlinks whose canonical target remains inside and reject symlinks escaping outside.
- Read no more than 64 KiB per call and support byte `offset`/`limit` continuation.
- Observe cancellation before and during potentially long work.

## TDD acceptance seam

Tests live under `src-tauri/src/babysit/repository_tools/tests/` and invoke only the public
registration/execution seam against real temporary directory trees. Cover relative/absolute/
parent paths, files/directories, missing paths, symlinks, binary input, continuation, budget,
cancellation, and redacted errors. Run only focused tests via `scripts/cargo-agent.sh`.

