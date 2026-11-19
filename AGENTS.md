<!-- prettier-ignore-start -->

<!-- BEGIN project-standards:agent-handoff -->
<!-- markdownlint-disable MD025 -->
# Agent Handoff

Use the repo-local `agent-handoff` skill at session startup and closeout. Do not reread state already injected by SessionStart. Keep project knowledge inside this repository and store credential references only, never values.
<!-- markdownlint-enable MD025 -->
<!-- END project-standards:agent-handoff -->

<!-- prettier-ignore-end -->

<!-- prettier-ignore-start -->

<!-- BEGIN project-standards:github-workflow -->
<!-- markdownlint-disable MD025 -->
# GitHub Workflow

This repository's work belongs to the `L3DigitalNet` organization. Route every GitHub work-state action through the table below; every row is a `gh-workflow` subcommand unless it says raw `gh`. Load the `github-workflow` skill for triage, a schema audit, T0 or relationship judgment, or rare recovery.

| Action | Command |
| --- | --- |
| Create a typed issue | `new --type T --title S [--field Name=Value]` |
| Set fields or Issue Type | `set --issue N [--type T] [--field Name=Value]` |
| Close or reopen an issue | `close --issue N --as done\|dropped` / `reopen --issue N --workflow V` |
| Check or read an issue or PR | `check`/`receipt --issue N` or `--pr N`; `check --pr N --through PHASE` |
| Summary / schema audit | `summary` / `audit` |
| Open a draft PR | raw `gh pr create --draft --body-file PATH` |
| Ready, then merge | `ready --pr N` / `merge --pr N [--method M] [--auto]` |
| Close an open Final unmerged | `close --pr N --as OUTCOME --reason S` |
| Classify admission | `admission --branch B [--offline]` |

All ten accept `--output human|json`; the binary is at `.agents/skills/github-workflow/bin/gh-workflow` and its `.claude/` twin, and its refusals name valid values. These rules bind even when the skill was never loaded:

- An operator instruction is sufficient authority for the action it names. You author acceptance criteria and admit work to `Ready`; open state never implies it.
- Every commit on a governed branch carries one `Workflow-Admission` trailer: `T0` (trivial prose repair, no protected surface), `PR #N` (written by `merge`), `handoff` (touches only `docs/handoff/**`, `docs/STATUS.md`, `docs/TODO.md`), or `release`. A commit mixing handoff and other paths is not handoff: like all other work it starts as a draft PR declaring `Final: #N`, `Supporting: #N`, or `Standalone` under `## Governing work`.
- Keep terminal state paired: `Done` closes as completed, `Dropped` as not planned; reopen returns a nonterminal `Workflow` value.
- Never create shadow state labels, mutate organization schema, or bypass live enforcement.
- A related finding you can address this session needs no issue: fix it here when this repository owns it, file it against the owning upstream repository, ask the operator only when it needs its own session.

<!-- markdownlint-enable MD025 -->
<!-- END project-standards:github-workflow -->

<!-- prettier-ignore-end -->

<!-- prettier-ignore-start -->

<!-- BEGIN project-standards:markdown-tooling -->
<!-- markdownlint-disable MD025 -->
# Markdown and structured-text tooling

Prettier owns physical formatting and markdownlint owns Markdown structure. Do not add overlapping tools.

Enabled checks: format, lint.
Markdown scope: `**/*.md`.
Structured-config scope: `**/*.json`, `**/*.jsonc`, `**/*.yml`, `**/*.yaml`.
Lint additionally skips generated directories: `.pytest_cache/**`, `.ruff_cache/**`, `.venv/**`, `node_modules/**`.

Declared exclusions:
- `docs/specs/fuzzy_dedupe_spec.md` (both): Preserve the existing house-format product and architecture draft until it is independently revised.
- `.agents/skills/agent-handoff/**` (both): The Agent Handoff package locks these skill files.
- `.claude/skills/agent-handoff/**` (both): The Agent Handoff package locks these skill files.
- `.agents/skills/figma-*/**` (both): Vendored verbatim from figma/mcp-server-guide skills-figquery; refresh from upstream instead of reformatting.

Check formatting over exactly that scope, with Git as the corpus authority:

```bash
git ls-files -s -z -- ':(glob)**/*.md' ':(glob)**/*.json' ':(glob)**/*.jsonc' ':(glob)**/*.yml' ':(glob)**/*.yaml' ':(glob,exclude).agents/skills/agent-handoff/**' ':(glob,exclude).agents/skills/figma-*/**' ':(glob,exclude).claude/skills/agent-handoff/**' ':(glob,exclude)docs/specs/fuzzy_dedupe_spec.md' | sed -zn '/^120000 /!s/^[^\t]*\t//p' | xargs -0 -r npx prettier --check --
```

Without Git, bound the same scope by glob instead. Prettier's CLI has no negative pattern, so this form does not apply the declared format exclusions above; pass them through an `--ignore-path` file inside the repository:

```bash
npx prettier --check --no-error-on-unmatched-pattern -- '**/*.md' '**/*.json' '**/*.jsonc' '**/*.yml' '**/*.yaml'
```

Never check or write with a bare `.`: it reaches undeclared languages and Git-excluded scratch.

Lint Markdown structure over the same Git-tracked scope:

```bash
git ls-files -z -- ':(glob)**/*.md' ':(glob,exclude).pytest_cache/**' ':(glob,exclude).ruff_cache/**' ':(glob,exclude).venv/**' ':(glob,exclude)node_modules/**' ':(glob,exclude).agents/skills/agent-handoff/**' ':(glob,exclude).agents/skills/figma-*/**' ':(glob,exclude).claude/skills/agent-handoff/**' ':(glob,exclude)docs/specs/fuzzy_dedupe_spec.md' | sed -z 's|^|:|' | xargs -0 -r npx markdownlint-cli2 --no-globs
```

Never lint a bare recursive glob: it descends into any independent Git repository checked out below this one.

Run the enabled checks before claiming completion.
<!-- markdownlint-enable MD025 -->
<!-- END project-standards:markdown-tooling -->

<!-- prettier-ignore-end -->
