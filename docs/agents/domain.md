# Domain Docs

How engineering skills should consume this repository's domain documentation when exploring the codebase.

## Layout

This is a single-context repository. Domain documentation lives at the repository root:

```
/
├── CONTEXT.md
├── docs/adr/
│   ├── 0001-example-decision.md
│   └── 0002-example-decision.md
└── src/
```

## Before exploring, read these

- **`CONTEXT.md`** at the repository root
- **`docs/adr/`** — read ADRs that touch the area you are about to work in

If either location does not exist, proceed silently. Do not flag its absence or suggest creating it upfront; `/grill-with-docs` creates these documents when terms or decisions are actually resolved.

## Use the glossary's vocabulary

When output names a domain concept—in an issue title, refactor proposal, hypothesis, or test name—use the term defined in `CONTEXT.md`. Do not drift to synonyms the glossary explicitly avoids.

If a needed concept is not in the glossary, reconsider whether the project already has a preferred term, or note the gap for `/grill-with-docs`.

## Flag ADR conflicts

If output contradicts an existing ADR, surface the conflict explicitly rather than silently overriding it.
