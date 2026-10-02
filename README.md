# aihist

A unified CLI for querying conversation history across all local AI harnesses: Claude Code, Codex, and OpenCode.

Indexes sessions into a local SQLite database with FTS5 full-text search. No proxy, no daemon, no network. The binary reads the harnesses' own storage formats directly.

## Storage locations indexed

| Tool | Format | Default path |
|------|--------|--------------|
| Claude Code | JSONL per session | `~/.claude/projects/**/*.jsonl` |
| Codex | JSONL per session | `~/.codex/sessions/**/*.jsonl` |
| OpenCode | SQLite | `~/.local/share/opencode/opencode.db` |

Pi is not indexed (its run-history only contains hashed task summaries, not conversation content).

## Installation

### From source (requires Rust 1.70+)

```bash
git clone https://github.com/SeeThruHead/aihist
cd aihist
cargo build --release
cp target/release/aihist ~/.local/bin/aihist
```

### Install agent skills

```bash
aihist install-skills
```

This writes three skills to `~/.agents/skills/`:

- `aihist-search` -- full-text search across all sessions
- `aihist-show` -- read a session transcript
- `aihist-mcp` -- inspect MCP tool calls in a session

After running this, any harness that reads `~/.agents/skills/` (Claude Code, Pi, OpenCode, Codex via AGENTS.md) can invoke these skills.

## Usage

### Index first

```bash
aihist index
# Indexed: 149  skipped: 0  errors: 0

# Incremental (only files modified since a unix timestamp in ms)
aihist index --since 1700000000000

# Verbose
aihist index --verbose
```

### Search

```bash
# Full-text search (BM25 ranking, snippet extraction)
aihist search "Effect Schema validator"

# Narrow to one tool
aihist search "bulk actions" --tool claude

# Top N results
aihist search "temporal workflow" --limit 5

# JSON output for piping
aihist search "csrf" --json | jq '.[].session_id'
```

### List sessions

```bash
aihist sessions
aihist sessions --tool opencode --limit 10
aihist sessions --since 1700000000000
```

### Show a session transcript

```bash
aihist show claude:abc123def456
aihist show opencode:some-uuid --json
```

### Inspect tool calls

```bash
# All tool calls in a session
aihist tools claude:abc123def456

# MCP calls only
aihist mcp claude:abc123def456

# Frequency of MCP tools used
aihist mcp claude:abc123def456 --json | jq '.[].tool_name' | sort | uniq -c | sort -rn
```

### Token usage stats

```bash
# Across all sessions
aihist stats

# For one session
aihist stats claude:abc123def456
```

## Options

All subcommands accept:

- `--json` -- output as JSON instead of tabular text
- `--db <PATH>` -- use a non-default database path (default: `~/.local/share/aihist/aihist.db`)

## Performance

Cold search (first run after boot): ~400ms. Warm search (page cache hot): ~30ms. The database uses `PRAGMA mmap_size=512MiB` to let the OS keep the index in page cache across invocations without holding RAM after the process exits.

## Configuration

No config file. All paths are derived from `$HOME`. To use a different database:

```bash
aihist --db /path/to/custom.db index
aihist --db /path/to/custom.db search "query"
```

To point adapters at non-default locations, set these environment variables:

| Variable | Adapter | Default |
|----------|---------|---------|
| `CLAUDE_DIR` | Claude | `$HOME/.claude` |
| `CODEX_DIR` | Codex | `$HOME/.codex` |
| `OPENCODE_DB` | OpenCode | `$HOME/.local/share/opencode/opencode.db` |

## Development

```bash
cargo test        # 49 tests
cargo build       # debug
cargo build --release
```

Tests use in-memory SQLite and fixture JSONL files under `tests/fixtures/`. No network, no filesystem side effects.

## License

MIT
