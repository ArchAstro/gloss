# Gloss

Gloss is a Rust CLI for attaching durable, agent-facing explanations to source
edit hunks. The committed `.gloss` record says why an edit exists; disposable
state under `.git/annotations/` maps its stable UUID to Git history.

## Install

```fish
cargo install --path .
gloss init
```

`gloss init` is the idempotent one-shot setup. It installs:

1. Reinstall-safe `pre-commit`, `post-commit`, and `post-rewrite` hooks.
2. `.github/workflows/gloss.yml` for pull-request validation.
3. The GitHub Linguist generated-file rule in `.gitattributes`.
4. Header-only gloss metadata for the repository files it creates.

If `.github/workflows/gloss.yml` already exists without Gloss's ownership
marker, setup stops instead of overwriting it.

```gitattributes
**/.annotations/*.gloss linguist-generated=true
```

Commit `.gitattributes`; GitHub will exclude glosses from language statistics
and collapse them as generated files in pull-request diffs.

## Use

```fish
gloss status
gloss lint --fix
git add -A
gloss lint --staged
gloss add src/foo.ex 42:58 \
  "Separate parsing from validation so malformed input remains inspectable."
```

Metadata comes from flags or environment variables:

```fish
set -x GLOSS_USER calvin
set -x GLOSS_AGENT codex
set -x GLOSS_SESSION sess_123
```

Every added, modified, or renamed source file must have a sibling gloss whose
`updated` timestamp changed in the same diff. A gloss may contain only its
header; explanations remain optional.

Maintenance commands have deliberately different authority:

1. `gloss lint [path...]` checks working-tree coverage and never writes.
2. `gloss lint --staged` reads the Git index and is installed as `pre-commit`.
3. `gloss lint --base origin/main` validates a committed CI/PR diff. Set
   `GLOSS_BASE` instead when that is more convenient.
4. `gloss lint --fix` creates missing header-only glosses, updates timestamps,
   maintains ranges/lifecycle, then lints again. It never stages files.
5. `gloss update [path...]` performs deterministic header/range maintenance.
6. `gloss repair` rebuilds UUID provenance and refuses ambiguous range changes.

Every command accepts `--json`. Failures use stable codes such as
`gloss_outside_hunk`, `stale_gloss`, and `outdated_header`.

## File mapping

```text
src/foo.ex
src/.annotations/foo.ex.gloss
```

Gloss files are normal committed files. Everything in `.git/annotations/` is
derived and can be deleted and rebuilt with `gloss repair`.
