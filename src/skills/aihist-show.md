---
name: aihist-show
description: Show the full turn-by-turn transcript of a specific AI session (Claude, Codex, or OpenCode). Use after aihist-search to read the actual conversation content, decisions, and code from a prior session.
argument-hint: <session_id>
---

# aihist-show

Retrieve the complete turn sequence for a session.

## Usage

```bash
# Show a session (get session_id from aihist search or aihist sessions)
aihist show claude:abc123def456

# JSON for structured access
aihist show opencode:some-uuid --json | jq '.[] | select(.role == "assistant") | .content'
```

## Getting session IDs

```bash
# List recent sessions
aihist sessions --limit 20

# Filter by tool
aihist sessions --tool claude

# Find by topic first
aihist search "the topic you remember" | head -5
```

## Output

Each turn shows: `role  tool_name(if any)  content_preview`

Roles: `user`, `assistant`, `tool_use` (outbound call), `tool_result` (inbound response).

## Notes

- Session IDs are prefixed with the tool name: `claude:`, `codex:`, `opencode:`
- Large sessions can have hundreds of turns; pipe through `less` or search with `--json | jq`
- Tool use turns show the tool name and input args; tool_result turns show the output
