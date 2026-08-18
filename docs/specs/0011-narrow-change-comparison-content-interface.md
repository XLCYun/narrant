---
status: accepted
---

# Narrow Change Comparison Content Interface

## Problem Statement

The former `change_content/local` interface exposed parallel cancellable and
non-cancellable entry points, made Review lifecycle coordinate Branch fetch followed by
comparison creation, and contained Pull Request-specific fetch refs and error
classification. That public interface exposed orchestration that belongs behind the
Branch Selection, Pull Request Selection, and cancellable-operation seams.

Narrant needs a smaller Change Comparison content interface that owns only immutable
comparison construction and content reads pinned to a saved Snapshot.

## Scope

- Introduce a Branch Selection resolver that accepts repository path, stable Branch
  Selection, and `OperationContext`, then performs remote fetch, ref resolution, and
  immutable comparison construction behind one interface.
- Introduce a Branch Ref catalog that returns canonical refs plus explicit current and
  default refs through one cancellable interface.
- Replace `LocalOpenedReview` with a Snapshot content reader restored from an immutable
  Change Comparison;
  the reader exposes one cancellable File Change read operation.
- Keep one crate-private exact-comparison kernel shared by Branch and Pull Request
  resolution.
- Move Pull Request fetch refs and fetch/exact-revision error classification into the
  Pull Request Selection resolver.
- Require `OperationContext` at every public local Git I/O interface. Callers without an
  external operation use `OperationContext::background()`; a convergence phase derives
  `without_cancellation()` from its current context.
- Remove the old fetch-then-open protocol and cancellable/non-cancellable wrapper pairs.

## Non-goals

- Do not move Snapshot staging, Workspace convergence, activation, rollback, or runtime
  reader ownership out of Review lifecycle.
- Do not move process execution, timeout, output limits, or process-tree termination out
  of Core Git process.
- Do not change Branch or Pull Request comparison semantics.
- Do not introduce a Git trait or a second Git implementation.
- Do not rename persisted Snapshot or File Change fields in this change.

## Interfaces

```rust
BranchSelectionResolver::resolve(operation, repository_path, selection)
    -> ResolvedBranchSelection

BranchRefCatalog::list(operation, repository_path)
    -> BranchRefs

SnapshotContentReader::restore(repository_path, comparison)
SnapshotContentReader::read(operation, path)
    -> FileChangeContent
```

The shared comparison builder is crate-private. It accepts grouped `ResolvedRevisions`
containing display refs and already acquired exact base/head revisions, then verifies
commits, computes the merge base, and parses the raw diff into a `ChangeComparison`.

## Ownership

- Branch Selection resolution owns canonical ref validation, selected-remote fetch,
  ref-to-commit resolution, and current three-dot comparison construction.
- Pull Request Selection resolution owns GitHub observation, matching-remotes fallback,
  `refs/pull/<number>/head` fetch, and fetch-versus-exact-revision diagnostics.
- Change Comparison content owns comparison construction and Snapshot-pinned object reads.
- Review lifecycle calls one resolver per source and continues to own all persistence,
  Workspace, activation, rollback, and runtime state transitions.
- Core Git process remains unaware of Selections, Pull Requests, Snapshots, and File
  Changes.

## Testing Strategy

- Test Branch Selection resolution through `resolve` with real temporary repositories and
  remotes, including remote movement, invalid refs, cancellation, and missing ancestry.
- Test Branch Ref catalog through `list` with real repositories, including detached HEAD
  and explicit remote default selection.
- Test Snapshot reader through `restore` and `read`, covering text, binary, symlink,
  submodule, rename, and stale-path behavior.
- Keep Pull Request special-ref and fallback behavior at the existing Pull Request resolver
  seam; remove direct tests of its former local helpers.
- Delete tests whose only subject is a removed convenience wrapper.

## Acceptance Criteria

1. Review lifecycle contains no Branch `fetch` followed by comparison-open protocol.
2. `review/content/comparison` exposes no Pull Request number, `refs/pull/*`, matching-remote
   fallback, or Pull Request-specific error type.
3. No public local I/O operation has cancellable and non-cancellable variants.
4. `LocalOpenedReview`, `open`, `open_with_cancellation`, `open_revisions`,
   `read_file_change_with_cancellation`, `list_refs*`, `fetch_selected_remote_refs`,
   `fetch_pull_request_objects`, and `resolve_pull_request_objects` are removed.
5. Branch and Pull Request resolution share one exact-comparison implementation.
6. Runtime File Change reads remain pinned to the Snapshot supplied to the reader.
7. Existing three-dot comparison, fetch, File Change, and Workspace behavior is preserved.
8. Rust formatting, Clippy, Rust tests, frontend tests, lint, typechecking, and production
   build pass.

## Related Decisions

- [ADR-0006: Use Core Git for repository operations](../adr/0006-use-core-git-for-repository-operations.md)
- [ADR-0016: Resolve Pull Request Selections behind one seam](../adr/0016-resolve-pull-request-selections-behind-one-seam.md)
- [ADR-0017: Separate operation cancellation from Core Git execution](../adr/0017-separate-operation-cancellation-from-core-git-execution.md)
- [ADR-0018: Put source resolution outside local change content](../adr/0018-put-source-resolution-outside-local-change-content.md)
