---
status: accepted
---

# Use extensible Code Object kinds

Code Graphs classify Code Objects with a common code-semantic category and a
language-qualified native kind. Object data retains the code information
callers need, while native kinds preserve distinctions between language constructs.
This keeps language extensions local to their Adapters without expanding the shared
enumeration for every syntax variation or requiring consumers to reinterpret raw
parser tags.

The category catalog and payloads are being refined in the
[Code Object Model](../specs/0018a-code-object-kinds.md). The current six-category
proposal does not change the accepted two-level classification decision recorded here.
