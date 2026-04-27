# Yggdrazil Governance Rules

This project uses **Yggdrazil** multi-agent governance. These rules apply whenever
you are running inside a managed world (CLAUDE.md contains `YGGDRAZIL PROTOCOL ACTIVE`).

## Required Behaviour

1. **Before starting any task** — read `.ygg/shared_memory.json` to see what files
   other agents are currently modifying.
2. **After every file modification** — call `ygg hook` so other agents know what
   you are touching:
   ```
   ygg hook --world <WORLD_ID> --files <comma-separated-relative-paths>
   ```
3. **On conflict warnings** — if CLAUDE.md gains a `CONFLICT WARNING` block,
   stop editing that file and notify the human before continuing.

## Why

Each agent runs in an isolated git worktree. `shared_memory.json` is the only
shared state. Without it, agents will silently clobber each other's work.
