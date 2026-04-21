# fazerei

A minimalist CLI to-do app backed by SQLite. Sane flags, fast, scriptable.

## Installation

```bash
git clone <repository-url>
cd fazerei
cargo install --path .
```

## Quick Start

```bash
fazerei add "Review pull request" -p 1 -d 1D -t work,urgent
fazerei list
fazerei done 1
fazerei list --all
```

## Commands

### `fazerei add <content>`

Add a new to-do.

| Option | Short | Description |
|--------|-------|-------------|
| `--priority` | `-p` | Priority 1 (highest) to 5 (lowest), default: 3 |
| `--due` | `-d` | Due date: YYYY-MM-DD or relative (0D, 1W, 2M, 1Y), default: today (0D) |
| `--notes` | `-n` | Free-form notes |
| `--tags` | `-t` | Tags — comma-separated or repeated `-t`. Normalized to lowercase. |
| `--recur` | `-r` | Recurrence spec (1D, 1W, 2M, 1Y). Creates a new instance when the item is marked done. |
| `--stdin` |  | Read one to-do per line from stdin. Shared flags apply to every item. |

```bash
fazerei add "Fix bug" -p 1 -d 2026-04-15
fazerei add "Mail package" -d -1D
fazerei add "Call mom" -d 2W -n "Sunday call"
fazerei add "Review PR" -t work,urgent
fazerei add "Grocery run" -t home -t errand

# Bulk from stdin — every line becomes a todo with the shared flags
printf 'write tests\nupdate docs\nopen PR\n' | fazerei add --stdin -p 2 -t release
```

### `fazerei list`

List to-do items. Pending only by default. See **Filtering**, **Sorting**, and **Output formats** below.

```bash
fazerei list                        # pending
fazerei list --all                  # pending + done
fazerei list --done                 # done only
fazerei list --overdue              # pending, past due
fazerei list --today                # due today
fazerei list --week                 # due within 7 days
fazerei list --tag work             # filter by tag
fazerei list --search "migration"   # text match on content + notes
fazerei list --sort updated         # most-recently changed first
fazerei list --json | jq '.'        # machine-readable
fazerei list --parsable             # machine-friendly, space-separated
fazerei list --simple               # bare-bones, hover-menu-friendly
fazerei list --count                # just the number
```

### `fazerei show <id>...`

Show full details for one or more items. Accepts multiple IDs.

```bash
fazerei show 1
fazerei show 1 3 7
```

Output:

```
ID:        1
Status:    Pending
Priority:  1 (highest)
Content:   Review pull request
Due:       2026-04-22
Tags:      work, urgent
Notes:     —
Created:   2026-04-20 23:48:10
Updated:   2026-04-20 23:48:10
```

### `fazerei edit <id>`

Update specific fields. Unspecified fields are left alone.

| Option | Short | Description |
|--------|-------|-------------|
| `--content` | `-c` | Replace content |
| `--priority` | `-p` | Replace priority (1–5) |
| `--due` | `-d` | Replace due date; `none` clears |
| `--notes` | `-n` | Replace notes; `none` clears |
| `--tags` | `-t` | Replace tags; `none` clears |
| `--recur` | `-r` | Replace recurrence spec; `none` clears |

```bash
fazerei edit 1 -c "New content"
fazerei edit 1 -p 5
fazerei edit 1 -d 2026-05-01
fazerei edit 1 -d none
fazerei edit 1 -t "reviewed,work"
fazerei edit 1 -t none
fazerei edit 1 -c "Do X" -p 2 -d 1W -t new
```

Multi-field edits run in a single transaction — if any field fails, nothing changes.

### `fazerei done <id>...` / `fazerei undone <id>...` / `fazerei rm <id>...`

Bulk-safe: run in a single transaction. If any id fails (e.g., not found), **nothing** is committed.

```bash
fazerei done 1 2 3
fazerei undone 4
fazerei rm 5 6
```

### `fazerei snooze <id>... --by <duration>`

Shift the due date of one or more items by a relative duration. Items without a due date get `today + duration`. All shifts run in a single transaction — if any id fails, nothing is committed.

```bash
fazerei snooze 3 --by 1W            # push #3 out by a week
fazerei snooze 1 4 -b 3D            # push two items by 3 days
fazerei snooze 7 --by -2D           # pull #7 in by 2 days
```

### `fazerei today`

Alias for `fazerei list --today`. Shows items due today in the pretty table.

### `fazerei next`

Show the single highest-priority pending item (soonest due date breaks ties). Handy for "what should I work on right now?"

```
ID:        3
Priority:  1 (highest)
Content:   Review PR
Due:       2026-04-22
Tags:      work, urgent
```

### `fazerei stats`

One-screen summary — totals, overdue, due-today, due-this-week, a pending-by-priority breakdown, and recent completions.

```
Total:      12  (8 pending, 4 done)
Overdue:    3
Due today:  2
Due week:   5
By priority (pending):
  1 (highest)  2
  2 (high)     3
  3 (medium)   3
Completed today:     1
Completed this week: 4
```

### `fazerei prune --done --older-than <rel>`

Delete completed items whose `updated_at` is strictly older than the cutoff. `--done` is required (no accidental mass delete of pending work). `--dry-run` / `-n` previews without deleting.

```bash
fazerei prune --done --older-than 30d --dry-run
fazerei prune --done --older-than 90d
```

### `fazerei undo`

Reverse the most recent mutation (`rm`, `prune`, `edit`, `done`, `undone`, `snooze`). One-level undo — only the last mutation is journaled. If there's nothing to undo, prints `Nothing to undo.` and exits cleanly.

```bash
fazerei rm 3 4 5
fazerei undo          # restores the three deleted items with their original ids and timestamps
```

Undo of `done` on a recurring task also removes the spawned clone.

### `fazerei export` / `fazerei import <file>`

Dump all to-dos to stdout as a JSON array, or import from a JSON file. Import appends new rows (fresh ids assigned) and preserves timestamps from the source.

```bash
fazerei export > backup.json
fazerei import backup.json
```

The JSON shape is the same as `fazerei list --json`. Export includes every item regardless of status / due date.

### `fazerei install-completion <shell>`

Install dynamic shell completions. Sourcing the activation line registers a hook that delegates completion back to `fazerei` at runtime, so TAB surfaces live data — real to-do IDs pulled from the current database, filtered by the subcommand — instead of a static snapshot.

Two modes:

```bash
fazerei install-completion <shell>            # prints instructions
fazerei install-completion <shell> -o <path>  # writes a sourceable file
```

Supported shells: `bash`, `zsh`, `fish`, `powershell`, `elvish`.

**Activation lines** (what the subcommand prints — copy these by hand if you prefer):

| Shell | Line |
|-------|------|
| bash | `source <(COMPLETE=bash fazerei)` |
| zsh | `source <(COMPLETE=zsh fazerei)` |
| fish | `COMPLETE=fish fazerei \| source` |
| powershell | `$env:COMPLETE = "powershell"; fazerei \| Out-String \| Invoke-Expression` |
| elvish | `eval (COMPLETE=elvish fazerei \| slurp)` |

**Zsh** also needs one extra line to stop zsh from alphabetically re-sorting candidates (so the order we set — nearest due date first — survives):

```zsh
zstyle ':completion:*:*:fazerei:*:*' sort false
```

The subcommand emits this automatically for zsh in both stdout and `-o` modes.

**What completes:**

- Subcommands, flags, and `<shell>` values.
- `show` / `edit` / `rm` — any to-do ID.
- `done` / `snooze` — pending IDs only.
- `undone` — done IDs only.

Each ID candidate carries a help string (shown inline in shells that support it): `<due> - <status> - <content…> - <notes…> - <created>`.

**Examples**

Bash — add to `~/.bashrc` after any bash-completion setup:

```bash
source <(COMPLETE=bash fazerei)
```

Zsh — add to `~/.zshrc` after `compinit`:

```zsh
autoload -U compinit
compinit
source <(COMPLETE=zsh fazerei)
zstyle ':completion:*:*:fazerei:*:*' sort false
```

Write to a file instead (bash-completion drop-in):

```bash
fazerei install-completion bash -o ~/.local/share/bash-completion/completions/fazerei
```

Fish auto-sources files in its completion dir:

```bash
fazerei install-completion fish -o ~/.config/fish/completions/fazerei.fish
```

Then restart the shell (or source the config file).

## Due date formats

Accepted anywhere a date is taken (`add -d`, `edit -d`, `list -d`, `list -p`):

| Form | Meaning |
|------|---------|
| `0D` | Today |
| `1D` | Tomorrow |
| `-1D` | Yesterday |
| `1W` / `2W` | +1 / +2 weeks |
| `1M` / `-2M` | +1 month / –2 months |
| `1Y` | +1 year |
| `YYYY-MM-DD` | Absolute ISO 8601 |

## Tags

- Stored as a normalized lowercase list. `-t "Work, urgent"` becomes `work,urgent`.
- Duplicates are folded. Whitespace is trimmed.
- Filter with `--tag work` (OR semantics across multiple `--tag` or a comma list).
- `-t none` on `edit` clears all tags.

## Filtering

| Flag | Meaning |
|------|---------|
| `--all` | Include done items |
| `--done` / `-D` | Only done items |
| `--overdue` | Pending items past their due date |
| `--today` | Items due today |
| `--week` | Items due within the next 7 days |
| `--due <date>` / `-d` | Due on or before this date (absolute or relative) |
| `--past <rel>` / `-p` | Include done items updated within this timeframe (e.g. `30d`) |
| `--priority <n>` | Filter by priority 1–5 |
| `--tag <name>` / `-t` | Match tag (repeatable / comma-list, OR) |
| `--search <q>` | Case-insensitive match on content + notes |
| `--include-nodate` | Include done items with no due date |

Shortcuts (`--overdue`, `--today`, `--week`) are mutually exclusive with `--due` and with each other.

## Sorting

| `--sort` value | Order |
|----------------|-------|
| `due` *(default)* | done last, nearest due first, higher priority first, newer first |
| `updated` | most recently changed first |
| `priority` | higher priority first |
| `created` | newest first |

Add `--reverse` / `-R` to flip whichever order you picked.

## Output formats

Mutually exclusive: `--count`, `--simple`, `--parsable`, `--json`.

### Pretty (default)

A bordered table with `# | status | priority | content | tags | due | updated | created`. Overdue pending rows are bold red when stdout is a terminal.

- `--full-date` — render dates like "Wednesday, April 22, 2026"
- `--priority-text` — render priority as e.g. "1 (highest)"

### `--simple`

Bare-bones, invariant. Great for status bars and hover menus.

```
[ ] 2026-04-22 Review PR
[x] 2026-04-18 Reply to email
```

Format: `{status} {due|-} {content}`. Single-space separated. Not affected by `--full-date` / `--priority-text`.

### `--parsable`

Machine-friendly. Space-separated, content always last so splitting on N–1 whitespace chunks keeps the tail intact. `-` fills null fields.

```bash
fazerei list --parsable
# 42 [ ] 2026-04-22 Review PR

fazerei list --parsable --fields id,tags,content
# 42 work,urgent Review PR
```

Fields: `id, status, priority, due, updated, created, tags, recurrence, content, notes`. Default: `id,status,due,content`. `--fields` requires `--parsable`.

### `--json`

Flat JSON array of objects. Compact; pipe through `jq` to pretty-print.

```bash
fazerei list --json | jq '.[] | {id, content, tags, due}'
```

Object keys: `id, status, priority, content, notes, due, tags, recurrence, created, updated`.

### `--count`

Print only the number of matching items.

```bash
fazerei list --overdue --count
# 3
```

## Database

Default location:

- Linux/macOS: `~/.local/share/fazerei/fazerei.db`
- Windows: `C:\Users\<user>\AppData\Roaming\fazerei\fazerei.db`

Override with a flag or env var:

```bash
fazerei --db /path/to/custom.db list
FAZEREI_DB=/path/to/custom.db fazerei list
```

Schema is migrated in place: new columns are added automatically on first open of existing DBs. Migrations are tracked via SQLite's `PRAGMA user_version`.

Timestamps (`created_at`, `updated_at`) are stored as RFC 3339 UTC (`YYYY-MM-DDTHH:MM:SSZ`) and rendered in local time for the pretty table and `show` view. `--json` output keeps them in UTC so external tools get an unambiguous format. Legacy naive timestamps from older DBs are interpreted as UTC.

## Priority levels

| Level | Label |
|-------|-------|
| 1 | Highest |
| 2 | High |
| 3 | Medium (default) |
| 4 | Low |
| 5 | Lowest |

## Examples

**Daily focus**

```bash
fazerei list --today
fazerei list --overdue
fazerei list --priority 1
```

**Pipe IDs forward**

```bash
fazerei list --overdue --parsable --fields id | xargs fazerei done
```

**JSON + jq**

```bash
fazerei list --all --json | jq '[.[] | select(.tags | index("work"))] | length'
```

**Shared database**

```bash
export FAZEREI_DB=/shared/todos.db
fazerei add "Team task" -t work
```

## Changelog

See [CHANGELOG.md](CHANGELOG.md).

## License

MIT
