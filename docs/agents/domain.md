# Domain Docs

How engineering skills should consume this repository's domain documentation when exploring the codebase.

## Layout

This is a single-context repository. Domain documentation lives at the repository root:

```
/
├── GLOSSARY.md
├── docs/specs/
│   └── 0001-example-feature.md
├── docs/adr/
│   ├── 0001-example-decision.md
│   └── 0002-example-decision.md
└── src/
```

## Before exploring, read these

- **`GLOSSARY.md`** at the repository root
- **`docs/specs/`** — read the specification linked from the current issue; if work is
  started without an issue, read the specification named by the requester
- **`docs/adr/`** — read ADRs that touch the area you are about to work in

If `GLOSSARY.md` or `docs/adr/` does not exist, proceed silently. Do not flag its absence
or suggest creating it upfront; `/grill-with-docs` creates these documents when terms or
decisions are actually resolved. If an issue lacks a specification link, ask for the
governing specification before implementing it.

## Use the glossary's vocabulary

When output names a domain concept—in an issue title, refactor proposal, hypothesis, or test name—use the term defined in `GLOSSARY.md`. Do not drift to synonyms the glossary explicitly avoids.

If a needed concept is not in the glossary, reconsider whether the project already has a preferred term, or note the gap for `/grill-with-docs`.

## Flag ADR conflicts

If output contradicts an existing ADR, surface the conflict explicitly rather than silently overriding it.
