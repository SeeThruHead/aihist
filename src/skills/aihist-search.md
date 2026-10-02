---
name: aihist-search
description: Full-text search across all AI conversation history (Claude, Codex, OpenCode). Returns session IDs, snippets, and scores ranked by relevance. Use whenever you need to find a prior conversation, a past decision, a tool call pattern, or code discussed in a previous session.
argument-hint: [query] [--tool claude|codex|opencode] [--since <unix-ms>] [--limit N]
---

# aihist-search

Search conversation history across all AI CLIs (Claude Code, Codex, OpenCode).

## Prereq

The local index must be current. Run `aihist index` if it may be stale (first use, or sessions exist that haven't been indexed). The index lives at `~/.local/share/aihist/aihist.db`.

## Usage

```bash
# Search all tools
aihist search "Effect Schema validator"

# Narrow to one tool
aihist search "bulk actions" --tool claude

# Narrow by recency (unix ms)
aihist search "temporal workflow" --since 1700000000000

# Machine-readable output for piping
aihist search "csrf fix" --json | jq '.[].session_id'
```

## Output columns (table mode)

`session_id  tool  date  score  snippet`

Score is BM25 rank (higher = more relevant). Snippet shows the matched text with `[...]` context markers.

## Follow-up commands

After finding a session ID:
- `aihist show <session_id>` -- full transcript of that session
- `aihist mcp <session_id>` -- MCP tool calls only (what MCPs were invoked and with what args)
- `aihist tools <session_id>` -- all tool calls including non-MCP

## What to search for

- Past architectural decisions, rejected options, rationale
- Specific code patterns, function names, error messages
- Prior MCP or tool invocations (search "mcp" + a topic)
- Sessions where a particular file or ticket was discussed
