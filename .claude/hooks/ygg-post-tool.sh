#!/usr/bin/env bash
# Yggdrazil PostToolUse hook — only fires inside managed worlds.
grep -q 'YGGDRAZIL PROTOCOL' CLAUDE.md 2>/dev/null || exit 0
world_id=$(grep -o 'World: `[^`]*`' CLAUDE.md 2>/dev/null | sed 's/World: `//;s/`//')
file=$(python3 -c "import sys,json; d=json.load(sys.stdin); print(d.get('tool_input',{}).get('file_path',''))" 2>/dev/null)
[ -n "$file" ] && [ -n "$world_id" ] && "/usr/local/bin/ygg" hook --world "$world_id" --files "$file" 2>/dev/null
exit 0
