# Backlog: GitHub

GitHub Issues are the implementation backlog. Product requirements and specifications are
versioned Markdown documents in [`docs/specs/`](../specs/README.md), not issue bodies. Use
the `gh` CLI for issue operations.

## Source of truth

- A PRD or specification is the source of truth for product scope, decisions, and
  cross-ticket acceptance criteria. Create and update it in `docs/specs/`.
- An issue is a small, independently executable implementation ticket. It must link to
  its governing specification and state only the ticket's scope, acceptance criteria,
  and blocking relationships.
- Do not duplicate a complete PRD or specification in an issue. Keep the document and
  its linked issues consistent when scope changes.

## Conventions

- **Create an issue**: `gh issue create --title "..." --body "..."`. Use a heredoc for multi-line bodies.
- **Read an issue**: `gh issue view <number> --comments`, filtering comments by `jq` and also fetching labels.
- **List issues**: `gh issue list --state open --json number,title,body,labels,comments --jq '[.[] | {number, title, body, labels: [.labels[].name], comments: [.comments[].body]}]'` with appropriate `--label` and `--state` filters.
- **Comment on an issue**: `gh issue comment <number> --body "..."`
- **Apply / remove labels**: `gh issue edit <number> --add-label "..."` / `--remove-label "..."`
- **Close**: `gh issue close <number> --comment "..."`

Infer the repository from `git remote -v`; `gh` does this automatically when run inside this clone.

## When a skill says "publish to the backlog"

Create a GitHub issue.

## When a skill produces a PRD or specification

Create or update its Markdown file under `docs/specs/`, then create an issue only when
there is work to track. The issue body links to the specific document and heading that
governs it.

## When a skill says "fetch the relevant ticket"

Run `gh issue view <number> --comments`.
