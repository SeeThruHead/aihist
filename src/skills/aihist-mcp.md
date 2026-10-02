---
name: aihist-mcp
description: List all MCP (Model Context Protocol) tool calls in a session -- what MCPs were invoked, with what arguments, and what they returned. Use to audit what tools an agent called, debug unexpected behavior, or find examples of prior MCP usage patterns.
argument-hint: <session_id>
---

# aihist-mcp

Show every MCP tool call (and result) in a session. MCP calls are `tool_use` turns whose tool name contains a `/` (e.g. `mcp__github__create_issue`) or whose name matches a registered MCP server prefix.

## Usage

```bash
# Show MCP calls in a session
aihist mcp claude:abc123def456

# All tool calls (MCP and built-in)
aihist tools claude:abc123def456

# JSON for structured analysis
aihist mcp claude:abc123def456 --json | jq '.[].tool_name' | sort | uniq -c | sort -rn
```

## Finding sessions with MCP activity

```bash
# Search for sessions that used a particular MCP tool
aihist search "mcp__github" --json | jq '.[].session_id'

# Then inspect one
aihist mcp <session_id>
```

## Output

Each row: `seq  tool_name  content_preview`

`tool_use` rows show the arguments sent to the MCP server.
`tool_result` rows (indented) show the response.

## Notes

- Only indexes sessions that have already been ingested via `aihist index`
- Claude stores MCP calls as tool_use blocks in assistant turns
- OpenCode stores them in the `parts` array of message data
- Codex stores them as `function_call` / `function_call_output` response items
