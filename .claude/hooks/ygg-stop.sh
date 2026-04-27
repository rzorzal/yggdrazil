#!/usr/bin/env bash
# Yggdrazil stop hook — signals session end when running inside a managed world.
grep -q 'YGGDRAZIL PROTOCOL' CLAUDE.md 2>/dev/null \
	&& "/usr/local/bin/ygg" hook --world "$(basename "$PWD")" 2>/dev/null
exit 0
