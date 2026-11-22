# Project Conventions

## Quick reference

| ID | Convention |
| --- | --- |
| C1 | Install the `figma-*` skills locally from upstream `skills-figquery`, verbatim and untracked; never commit them, because upstream grants no license. |
| C2 | Route Rust legs to `rust-engineer` or `rust-implementer`, UI legs to `ui-engineer`, and Rust or UI verification to `rust-verifier`. |

## Numbered conventions

Add stable project patterns with identifiers that are never renumbered.

### C1: Figma skills are installed locally, never tracked

- Source: `figma/mcp-server-guide` `skills-figquery/` (plugin 2.2.211, upstream `43a06ff`), copied to `.agents/skills/figma-*/` for Codex.
- Upstream declares no license, so the copies are gitignored and stay out of the public repository (untracked 2026-11-22, owner decision).
- Install or refresh in each clone or worktree that needs them: copy the directories from a fresh upstream clone, verbatim, and confirm with `diff -rq`.
- Use `skills-figquery`, not `skills/`: Figma's Claude and Cursor plugins both ship `skills-figquery`. Claude Code uses its Figma plugin instead.

### C2: Route subagent legs to the project roles

- Rust legs go to `rust-engineer`, or to `rust-implementer` for mechanical edits.
- egui and UI legs that need Figma detail go to `ui-engineer`; verification of Rust or UI claims goes to `rust-verifier`.
- Every other leg keeps its global role. The role table is in `CLAUDE.md` "Project subagents"; definitions are in `.claude/agents/`.
- When a global source role changes in agent-configs, update its project copy in the same change.
