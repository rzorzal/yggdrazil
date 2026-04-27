#!/usr/bin/env bash
# Yggdrazil stop hook — signals session end when running inside a managed world.
# Fires on every Claude Code session stop; guard ensures it only acts in ygg worlds.
grep -q 'YGGDRAZIL PROTOCOL' CLAUDE.md 2>/dev/null \
    && ygg hook --world "$(basename "$PWD")" 2>/dev/null
exit 0
