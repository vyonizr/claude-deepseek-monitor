# Domain Docs

How the engineering skills should consume this repo's domain documentation when exploring the codebase.

## Before exploring, read these

- **`CONTEXT.md`** at the repo root — domain glossary, file map, pacing/DeepSeek logic explainer, and the Decisions table (this repo embeds its ADRs there rather than under `docs/adr/`).

## File structure

Single-context repo:

```
/
├── CONTEXT.md          ← domain glossary + embedded ADR table
└── src-tauri/
```

## Use the glossary's vocabulary

When your output names a domain concept (in an issue title, a refactor proposal, a hypothesis, a test name), use the term as defined in `CONTEXT.md`. Don't drift to synonyms the glossary explicitly avoids.

If the concept you need isn't in the glossary yet, that's a signal — either you're inventing language the project doesn't use (reconsider) or there's a real gap (note it for `/domain-modeling`).

## Flag ADR conflicts

If your output contradicts an existing decision in `CONTEXT.md`'s Decisions table, surface it explicitly rather than silently overriding:

> _Contradicts ADR-004 (±1pp pacing threshold) — but worth reopening because…_
