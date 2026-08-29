# Gloss

Gloss is a Rust CLI for attaching durable, agent-facing explanations to source
edit hunks. The committed `.gloss` record says why an edit exists; disposable
state under `.git/annotations/` maps its stable UUID to Git history.

## Install

With Homebrew:

```fish
brew install archastro/tools/gloss
```

Or directly from source:

```fish
cargo install --path .
gloss init
```

`gloss init` is the idempotent one-shot setup. It installs:

1. Reinstall-safe `pre-commit`, `post-commit`, and `post-rewrite` hooks.
2. `.github/workflows/gloss.yml` for pull-request validation.
3. The GitHub Linguist generated-file rule in `.gitattributes`.
4. Project-local editor exclusions that do not affect Git tracking.
5. Header-only gloss metadata for the repository files it creates.
6. One canonical Gloss agent skill, with adapters for every supported harness
   found on `PATH`.

Agent skills install at project scope by default. Use `--user` for a home-level
installation, or `--project` to state the default explicitly:

```fish
gloss init                # project skills
gloss init --project      # project skills, explicit
gloss init --user         # user skills; repository setup stays project-local
```

Setup writes the skill once to `.skills/gloss`, then synthesizes lightweight
symlink adapters for all detected Claude, Codex, Cursor, Grok Build, and Rovo
harnesses. Their project adapter destinations are:

1. Claude: `.claude/skills/gloss`
2. Codex: `.codex/skills/gloss`
3. Cursor: `.cursor/plugins/local/archagents/skills/gloss`
4. Grok Build: `.grok/skills/gloss`
5. Rovo: `.rovodev/skills/archagent-gloss`

At user scope, the canonical skill is `~/.skills/gloss` and the same adapter
paths are rooted in the user's home directory. Setup does not overwrite an
existing skill unless it contains Gloss's ownership marker. Re-run `gloss init`
to refresh managed skills and adapters safely.

If `.github/workflows/gloss.yml` already exists without Gloss's ownership
marker, setup stops instead of overwriting it.

### Editor integration

`gloss init` keeps committed glosses out of the normal editing surface without
putting them in `.gitignore` or changing user/global editor settings:

1. VS Code, Cursor, and Windsurf: `.vscode/settings.json` file, search, and
   watcher exclusions.
2. Zed: `.zed/settings.json` file-scan exclusions, including Zed's defaults
   when setup creates the setting.
3. Sublime Text: `file_exclude_patterns` and `index_exclude_patterns` are
   merged into existing root-level `*.sublime-project` files. Setup does not
   create a project file when the repository has none.
4. Helix, Neovim, Vim, and Emacs search/picker integrations: a managed rule in
   `.ignore`, the portable ignore file used by ripgrep and related tools.

Stock Neovim/Emacs tree views and JetBrains project views do not share a safe,
declarative, portable project exclusion format. Setup deliberately avoids
executable `.nvim.lua`/`.dir-locals.el` files and generated JetBrains workspace
state. Configure those views locally if needed; their common ripgrep-based
search integrations still honor `.ignore`.

Existing settings and ignore rules are preserved. If a setting has an
incompatible type, an explicit conflicting value, invalid JSON, or an edited
Gloss ownership block, setup reports the conflict rather than guessing.

```gitattributes
**/.gloss/*.gloss linguist-generated=true
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
src/.gloss/foo.ex.gloss
```

Gloss files are normal committed files. Everything in `.git/annotations/` is
derived and can be deleted and rebuilt with `gloss repair`.
