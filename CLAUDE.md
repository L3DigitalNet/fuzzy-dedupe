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

Check formatting over exactly that scope, with Git as the corpus authority:

```bash
git ls-files -s -z -- ':(glob)**/*.md' ':(glob)**/*.json' ':(glob)**/*.jsonc' ':(glob)**/*.yml' ':(glob)**/*.yaml' ':(glob,exclude).agents/skills/agent-handoff/**' ':(glob,exclude).claude/skills/agent-handoff/**' ':(glob,exclude)docs/specs/fuzzy_dedupe_spec.md' | sed -zn '/^120000 /!s/^[^\t]*\t//p' | xargs -0 -r npx prettier --check --
```

Without Git, bound the same scope by glob instead. Prettier's CLI has no negative pattern, so this form does not apply the declared format exclusions above; pass them through an `--ignore-path` file inside the repository:

```bash
npx prettier --check --no-error-on-unmatched-pattern -- '**/*.md' '**/*.json' '**/*.jsonc' '**/*.yml' '**/*.yaml'
```

Never check or write with a bare `.`: it reaches undeclared languages and Git-excluded scratch.

Lint Markdown structure over the same Git-tracked scope:

```bash
git ls-files -z -- ':(glob)**/*.md' ':(glob,exclude).pytest_cache/**' ':(glob,exclude).ruff_cache/**' ':(glob,exclude).venv/**' ':(glob,exclude)node_modules/**' ':(glob,exclude).agents/skills/agent-handoff/**' ':(glob,exclude).claude/skills/agent-handoff/**' ':(glob,exclude)docs/specs/fuzzy_dedupe_spec.md' | sed -z 's|^|:|' | xargs -0 -r npx markdownlint-cli2 --no-globs
```

Never lint a bare recursive glob: it descends into any independent Git repository checked out below this one.

Run the enabled checks before claiming completion.
<!-- markdownlint-enable MD025 -->
<!-- END project-standards:markdown-tooling -->

<!-- prettier-ignore-end -->
