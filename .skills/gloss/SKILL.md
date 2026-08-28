---
name: gloss
description: Maintain Gloss intent annotations while editing source files. Use when a repository has Gloss configured, when source edits need explanations, or when gloss lint, stale annotations, pre-commit checks, or CI metadata validation are involved.
metadata:
  managed-by: gloss
---

# Gloss

Gloss stores agent-facing explanations for source edits in sibling
`.annotations/<file>.gloss` files. Use the `gloss` CLI for all mutation and
validation; do not hand-edit generated headers, ranges, UUIDs, or provenance.

## While editing

1. Run `gloss status` to see changed hunks and current explanation coverage.
2. For a non-obvious logical edit, add intent before finishing:

   ```text
   gloss add <file> <start>:<end> "<why this edit exists>"
   ```

   Explain intent or constraints. Do not merely narrate the diff.
3. Explanations are optional. Every touched file still needs a fresh gloss
   header, including files whose changes are self-explanatory.
4. Run `gloss lint --fix` after source edits. Review the generated metadata and
   resolve any `stale_gloss` or `ambiguous_repair` error instead of guessing.

## Before committing

1. Stage each source file and its sibling gloss together.
2. Run `gloss lint --staged`. Do not treat a working-tree lint as proof that the
   staged snapshot is valid.
3. If Git history was rewritten, run `gloss repair`, review the result, and lint
   again.

Never silently retarget a stale explanation to unrelated code. A missing
explanation is safer than an incorrect one.
