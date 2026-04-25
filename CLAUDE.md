# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project

`fazerei` is a single-binary Rust CLI to-do app backed by SQLite (bundled `rusqlite`). User-facing commands and behaviors are documented in `README.md` — read that for the full surface area before changing CLI semantics.

## Commands

```bash
cargo build                          # debug build
cargo build --release                # release build
cargo run -- <subcommand> [args]     # run from source
cargo test                           # all tests (unit + integration)
cargo test --test cli                # only the integration tests in tests/cli.rs
cargo test <name>                    # run a single test by name substring
cargo clippy --all-targets -- -D warnings   # lint
cargo fmt                            # format
cargo install --path .               # install the binary
```

Integration tests in `tests/cli.rs` shell out to the compiled binary via `assert_cmd` and isolate state by pointing `FAZEREI_DB` at a `tempfile::TempDir`. Use the same pattern when adding tests — never let a test touch the user's real DB.

## Architecture

Three Rust source files in a flat layout:

- `src/main.rs` — clap CLI definition, command dispatch, and **all business-logic orchestration**. Each subcommand has a `cmd_*` handler that parses inputs, validates, opens transactions, calls `db::*`, and prints. Validation (priority range, date parsing, recurrence shape) lives here, not in `db`.
- `src/db.rs` — thin SQLite layer. Exposes connection setup, schema migration, and CRUD/query functions. No clap, no printing. SQL strings are built here; parameters are always bound, never interpolated.
- `src/models.rs` — domain types (`Todo`, `Priority`, `Sort`, `Field`), tag normalization, and output renderers (`render_json`, `render_parsable`, `render_simple`, `TodoRow` for the pretty table).
- `tests/cli.rs` — black-box tests against the built binary.

### Key invariants to preserve

**Migrations are append-only and idempotent.** `db::MIGRATIONS` is an ordered slice tracked by `PRAGMA user_version`. To change schema, *add* a new migration function — never edit a shipped one. Each migration must be safe to re-run (use `IF NOT EXISTS`, `column_exists` guards). Existing user DBs auto-migrate on next open.

**Tags are stored as `,tag1,tag2,` (leading + trailing commas).** This lets `tags LIKE '%,work,%'` match exactly without false positives. `models::normalize_tags` produces this form; filter SQL in `db::list` depends on it. Don't change one without the other.

**Bulk mutations are all-or-nothing.** `done`, `undone`, `rm`, `snooze`, multi-field `edit`, and `import` open a `conn.transaction()` and only commit if every item succeeds. Tests assert this rollback behavior (`bulk_rm_rolls_back_on_missing_id`) — keep it.

**The undo journal is single-slot.** `undo_last` has `CHECK(id = 1)` — only one entry exists. Every mutating command writes a journal entry inside its transaction via `write_journal(...)` *before* commit, with a JSON `payload` capturing enough state to reverse the action (full row snapshots for `rm`/`prune`/`edit`, id lists + spawned-clone ids for `done`/`undone`, prev_due per id for `snooze`). `cmd_undo` parses this payload and reverses; pure additions like `add` and `import` are intentionally *not* journaled. If you add a new mutating command, write a journal entry — otherwise `undo` will silently revert something else.

**Dates: stored as `YYYY-MM-DD` strings, timestamps as RFC 3339 UTC (`...Z`).** Relative inputs (`0D`, `1W`, `-2M`, `1Y`) are resolved to absolute ISO dates in `main.rs` (`parse_relative` → `shift_today` → `resolve_date`) before reaching `db`. `db` never parses dates. JSON output keeps timestamps in UTC; the pretty table and `show` view convert to local via `format_ts_local`.

**Output modes on `list` are mutually exclusive.** `--count`, `--simple`, `--parsable`, `--json` are wired with clap `conflicts_with_all`. The default is the bordered `tabled` table. `--simple` must stay ANSI-free (status-bar consumers depend on this — see `list_simple_has_no_ansi_escape_bytes`).

**Recurrence spawns a clone on `done`.** When a recurring item is marked done, a new pending row is inserted with `due = next_occurrence(old_due, recur_spec)`. The spawned id is recorded in the `done` journal entry so `undo` deletes the clone in addition to flipping the parent back to pending.

**Shell completion is dynamic, not a static snapshot.** `clap_complete::engine::ArgValueCandidates` callbacks (`todo_id_candidates`, `pending_todo_id_candidates`, `done_todo_id_candidates`) open the DB at completion time and return real ids with help strings. They must stay fast and never panic — failures fall through to an empty candidate list.

### DB resolution order

`--db` flag → `FAZEREI_DB` env var → `directories::ProjectDirs` default (`~/.local/share/fazerei/fazerei.db` on Linux). The `db` arg is `global = true`, so it works after any subcommand. `--db` and `FAZEREI_DB` share the same clap arg, so one source of truth.

## Conventions

- Errors in command handlers exit via `fail(msg)` — a printed `error: <msg>` to stderr plus `exit(1)`. Don't propagate `Result` out of `cmd_*` for user-facing errors.
- New SQL: bind parameters with `params![...]` or `?` placeholders. Never `format!` user input into SQL.
- New CLI flags: keep short flags consistent with the existing scheme (`-p` priority, `-d` due, `-t` tags, `-n` notes, `-r` recur, `-D` done-only, `-R` reverse). Several short letters are reused across subcommands with different long names — check before reassigning.
- `0D` (today) is the intentional default for `add --due`; do not propose removing it.
