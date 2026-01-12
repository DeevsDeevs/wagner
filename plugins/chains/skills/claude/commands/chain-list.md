Lists all available chains in this project.

## Steps

1. Check if chains directory exists:
```bash
ls -d .claude/chains/*/ 2>/dev/null
```

2. For each chain, get:
```bash
# Chain name (directory name)
# Link count
ls .claude/chains/[chain-name]/*.md 2>/dev/null | wc -l

# Most recent link (by timestamp in filename)
ls .claude/chains/[chain-name]/*.md 2>/dev/null | sort -r | head -1
```

3. Show the user:
   - Chain name
   - Number of links
   - Most recent link filename
   - How to load: `/chain-load [chain-name]`

If no chains exist, suggest using `/chain-link [name]` to start one.
