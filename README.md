# fazerei

A simple CLI to-do app backed by SQLite.

## Installation

### From Crates.io

```bash
cargo install fazerei
```

### From Source

```bash
git clone <repository-url>
cd fazerei
cargo build --release
# Binary will be at target/release/fazerei
```

## Quick Start

```bash
# Add a to-do due tomorrow with high priority
fazerei add "Review pull request" -p 1 -d 1D

# List pending items
fazerei list

# Mark it done
fazerei done 1

# List all (including completed)
fazerei list --all
```

## Commands

### `fazerei add <content>`

Add a new to-do item.

```bash
fazerei add "Write documentation"           # Basic
fazerei add "Fix bug" -p 1 -d 2026-04-15     # High priority, specific date
fazerei add "Mail package" -d -1D          # Due yesterday
fazerei add "Call mom" -d 2W -n "Sunday call"  # Due in 2 weeks, with notes
```

| Option | Short | Description |
|--------|-------|-------------|
| `--priority` | `-p` | Priority 1 (highest) to 5 (lowest), default: 3 |
| `--due` | `-d` | Due date: YYYY-MM-DD or relative (0D, 1W, 2M, 1Y), default: today (0D) |
| `--notes` | `-n` | Optional notes for the item |

### `fazerei list`

List to-do items. By default, shows only pending items.

```bash
fazerei list                    # Pending items only
fazerei list --all             # Show all (pending + done)
fazerei list --done            # Show only completed
fazerei list -p 1              # Filter by priority
fazerei list --days 7          # Due within next 7 days (including overdue)
fazerei list --weeks 2         # Due within next 2 weeks
fazerei list --months 1         # Due within next month
```

### `fazerei show <id>`

Show full details of a to-do item.

```bash
fazerei show 1
```

Output:
```
ID:        1
Status:    Pending
Priority:  1 (highest)
Content:   Review pull request
Due:       2026-04-09
Notes:     Check the tests too
Created:   2026-04-08 14:30:00
Updated:   2026-04-08 14:30:00
```

### `fazerei edit <id>`

Edit an existing to-do item. Only specified fields are updated.

```bash
fazerei edit 1 --content "New content"      # Change content
fazerei edit 1 --priority 5                 # Lower priority
fazerei edit 1 --due 2026-05-01             # Set new due date
fazerei edit 1 --due none                   # Clear due date
fazerei edit 1 --notes "Added info"         # Update notes
fazerei edit 1 --notes none                 # Clear notes
fazerei edit 1 -c "Do X" -p 2 -d 1W         # Multiple updates
```

### `fazerei done <id>`

Mark a to-do item as done.

```bash
fazerei done 1
```

### `fazerei undone <id>`

Revert a to-do item to pending (mark as not done).

```bash
fazerei undone 1
```

### `fazerei rm <id>`

Delete a to-do item permanently.

```bash
fazerei rm 1
```

## Due Date Formats

### Relative Shorthand

| Format | Meaning |
|--------|---------|
| `0D` | Today |
| `1D` | Tomorrow |
| `-1D` | Yesterday |
| `1W` | 1 week from today |
| `2W` | 2 weeks from today |
| `1M` | 1 month from today |
| `2M` | 2 months from today |
| `1Y` | 1 year from today |
| `-2M` | 2 months ago |

### Absolute Date

Use ISO 8601 format: `YYYY-MM-DD`

```bash
fazerei add "Event" -d 2026-12-25
```

## Database

By default, the database is stored at:
- Linux/macOS: `~/.local/share/fazerei/fazerei.db`
- Windows: `C:\Users\<user>\AppData\Local\fazerei\fazerei.db`

### Custom Database Location

**Via CLI flag:**
```bash
fazerei --db /path/to/custom.db add "Task"
```

**Via environment variable:**
```bash
export FAZEREI_DB=/path/to/custom.db
fazerei add "Task"
```

## Priority Levels

| Level | Label |
|-------|-------|
| 1 | Highest |
| 2 | High |
| 3 | Medium (default) |
| 4 | Low |
| 5 | Lowest |

## Examples

### Daily Workflow

```bash
# Add today's tasks
fazerei add "Morning standup meeting" -p 1 -d 0D
fazerei add "Review design doc" -p 2 -d 1D
fazerei add "Update documentation" -p 3 -d 1W

# See what needs doing
fazerei list

# Mark standup done after meeting
fazerei done 1

# See all completed today
fazerei list --done
```

### Weekly Planning

```bash
# Add weekly tasks with relative dates
fazerei add "Submit weekly report" -p 1 -d 1W
fazerei add "Team sync" -p 2 -d 3D
fazerei add "Code review" -p 2 -d 2D
fazerei add "Update roadmap" -p 3 -d 1W

# Focus on what's due soon
fazerei list --days 3

# Focus on high priority
fazerei list -p 1
```

### Project Tracking

```bash
# Add project tasks
fazerei add "Project kickoff" -p 1 -d 0D -n "Invite all stakeholders"
fazerei add "Define requirements" -p 1 -d 1W
fazerei add "Setup CI/CD" -p 2 -d 2W
fazerei add "Write tests" -p 2 -d 3W
fazerei add "Deploy MVP" -p 1 -d 1M

# View full project details
fazerei show 5

# Lower priority of a task after reprioritization
fazerei edit 4 --priority 3

# Clear due date for low-priority task
fazerei edit 4 --due none
```

### Working with Others' Data

```bash
# Use a shared database
fazerei --db /shared/todos.db list

# Use environment variable for persistent custom location
export FAZEREI_DB=/shared/todos.db
fazerei add "Shared task" -p 2
```

## License

MIT
