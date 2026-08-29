---
name: gloss
description: Capture review-relevant reasoning for source edits with Gloss. Use in a Gloss-configured repository when edits involve important design decisions, constraints, non-obvious choices, local rules or idioms, or when Gloss metadata, lint, repair, hooks, or CI validation need maintenance. Skip annotations that only narrate obvious or mechanical changes.
metadata:
  managed-by: gloss
---

# Gloss

Gloss stores review context for source edits in sibling
`.gloss/<file>.gloss` files. Its purpose is to preserve the reasoning a
reviewer cannot recover from the diff alone: why the code takes this shape,
which constraints mattered, and what failure or tradeoff the choice addresses.
Enumerate the concrete decision factors—constraints, alternatives, rules, and
expected consequences—without turning the gloss into an unfiltered transcript.

Use the `gloss` CLI for all mutation and validation. Do not hand-edit generated
headers, ranges, UUIDs, or provenance.

## The annotation standard

Every gloss record must state **why** the edit exists. A record that only says
what changed is useless; do not create it.

Add a gloss when a hunk contains review-relevant reasoning such as:

1. An important design or architectural decision: boundaries, abstractions,
   ownership, data flow, API shape, persistence, or failure handling.
2. A choice between plausible alternatives, including the tradeoff or failure
   mode that ruled out the obvious alternative.
3. An esoteric, counterintuitive, or easy-to-"simplify" choice whose purpose
   would otherwise be lost: ordering, concurrency, compatibility, workarounds,
   unusual constants, or deliberately indirect code.
4. A constraint or invariant that drove the implementation: security, privacy,
   performance, reliability, migration, backwards compatibility, or an external
   system's behavior.
5. A project or domain rule, idiom, convention, colloquialism, or local term
   that materially influenced the decision. Define obscure shorthand and connect
   it to the code choice; do not create a glossary entry unrelated to the edit.
6. A non-local consequence, deliberate omission, or intentionally narrow scope
   that a reviewer might otherwise mistake for a bug or incomplete work.
7. A bug fix whose important context is the root cause and why this layer is the
   right place to fix it.

Use this gate before adding a record:

> Would a reviewer reasonably ask "why this approach?", and is the answer
> missing from the diff and nearby code?

If not, skip the record. Glosses are not coverage quotas.

Do not add gloss records for:

- formatting, import sorting, generated metadata, routine renames, or other
  mechanical edits with no meaningful decision;
- statements that merely narrate the diff, such as "added a check", "moved the
  helper", "updated tests", or "renamed the variable";
- generic justification such as "cleaner", "more maintainable", "best
  practice", or "future-proof" without the concrete constraint or consequence;
- command history, implementation play-by-play, tool output, or unfiltered
  internal monologue;
- obvious test updates that only mirror changed behavior;
- speculative rationale not actually used to make the edit; or
- duplicate explanations scattered across every hunk of one logical decision.

Prefer one precise gloss per logical decision. Write a compact rationale that
names the choice, the reason, and the consequence when useful:

```text
<choice> because <constraint or evidence>; this avoids/enables <consequence>.
```

Good:

```text
Keep parsing separate from validation because diagnostic callers must inspect malformed input without triggering policy checks; combining them would make inspection enforce runtime policy.
```

```text
Use relative skill adapters because project and user installs must remain relocatable; copied skill bodies would drift between harnesses.
```

Bad:

```text
Moved validation into another function.
```

```text
Added symlinks for the skills.
```

## While editing

1. Run `gloss status` to see changed hunks and current explanation coverage.
2. Inspect each logical edit using the review-value gate above. For decisions
   that pass it, add the rationale before finishing:

   ```text
   gloss add <file> <start>:<end> "<why this edit exists>"
   ```

3. Before adding each record, verify that it contains an explicit reason and
   would help a reviewer evaluate the decision. If it only restates the hunk,
   do not add it.
4. Explanations are optional. Every touched file still needs a fresh gloss
   header, including files whose changes are self-explanatory. A header-only
   gloss is correct when no hunk carries useful decision context.
5. Run `gloss lint --fix` after source edits. Review the generated metadata and
   resolve any `stale_gloss` or `ambiguous_repair` error instead of guessing.

## Before committing

1. Stage each source file and its sibling gloss together.
2. Run `gloss lint --staged`. Do not treat a working-tree lint as proof that the
   staged snapshot is valid.
3. If Git history was rewritten, run `gloss repair`, review the result, and lint
   again.

Never silently retarget a stale explanation to unrelated code. A missing gloss
record is safer than an incorrect, invented, or low-value one.
