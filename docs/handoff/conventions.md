# Project Conventions

## Quick reference

| ID | Convention |
| --- | --- |
| C1 | Install the `figma-*` skills locally from upstream `skills-figquery`, verbatim and untracked; never commit them, because upstream grants no license. |

## Numbered conventions

Add stable project patterns with identifiers that are never renumbered.

### C1: Figma skills are installed locally, never tracked

- Source: `figma/mcp-server-guide` `skills-figquery/` (plugin 2.2.211, upstream `43a06ff`), copied to `.agents/skills/figma-*/` for Codex.
- Upstream declares no license, so the copies are gitignored and stay out of the public repository (untracked 2026-11-22, owner decision).
- Install or refresh in each clone or worktree that needs them: copy the directories from a fresh upstream clone, verbatim, and confirm with `diff -rq`.
- Use `skills-figquery`, not `skills/`: Figma's Claude and Cursor plugins both ship `skills-figquery`. Claude Code uses its Figma plugin instead.
