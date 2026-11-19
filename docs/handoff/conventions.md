# Project Conventions

## Quick reference

| ID | Convention |
| --- | --- |
| C1 | Refresh vendored `figma-*` skills from upstream `skills-figquery` verbatim; never reformat or hand-edit them. |

## Numbered conventions

Add stable project patterns with identifiers that are never renumbered.

### C1: Vendored Figma skills stay byte-identical to upstream

- Source: `figma/mcp-server-guide` `skills-figquery/` (plugin 2.2.211, upstream `43a06ff`), copied to `.agents/skills/figma-*/`.
- Use `skills-figquery`, not `skills/`: Figma's Claude and Cursor plugins both ship `skills-figquery`.
- To refresh, replace the directories from a fresh upstream clone and confirm with `diff -rq`.
- The markdown-tooling exclusion in `.standards/config.toml` keeps Prettier and markdownlint off these files.
