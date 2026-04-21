# Changelog

All notable changes to **fazerei** are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project follows [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

A large iteration on the CLI surface, storage layer, and test coverage. The
pretty-print, machine-parseable, and display outputs were all reworked in a
single pass; tagging, search, recurrence, undo, and export/import were
introduced; and the query layer got an opt-in `--sort`, proper indexes, and
parameterized date filters.

### Added

- **Tags.** `-t/--tags` on `add` and `edit` (comma-separated or repeated `-t`, normalized to lowercase, deduped). `--tag` on `list` is repeatable / comma-delim with OR semantics. Tags appear in the pretty table, `show`, `--parsable` (new `tags` field), and `--json`. `edit -t none` clears.
- **Recurrence.** `-r/--recur` on `add` and `edit` (positive relative specs: `1D`, `1W`, `2M`, `1Y`). When a recurring item is marked `done`, a clone is auto-spawned with `due + recurrence`. `edit -r none` clears. `Recur:` line in `show`; `recurrence` available as a parsable field and JSON key.
- **Text search.** `--search <q>` on `list` — case-insensitive substring match across `content` and `notes`, parameterized.
- **Shortcut date filters.** `--overdue`, `--today`, `--week` on `list`. Mutually exclusive with each other and with `--due`. `--overdue` implies pending-only.
- **Sort control.** `--sort {due|updated|priority|created}` (default `due` preserves the previous ordering). `--reverse` / `-R` flips whichever order is active.
- **New output modes on `list`.**
  - `--parsable` — invariant, space-separated, content/notes always last so whitespace-splitting keeps the tail intact. `-` fills null fields. Field set configurable via `--fields` (default `id,status,due,content`).
  - `--json` — compact JSON array; one object per todo. Pipe through `jq` to format.
  - `--simple` redesigned: bare-bones `[x] YYYY-MM-DD Content`, invariant, no ANSI. Suitable for status bars / hover menus.
  - `--count`, `--simple`, `--parsable`, `--json` are mutually exclusive at the clap level.
- **New subcommands.**
  - `snooze <id>... --by <dur>` — shift due date(s) by a relative offset. Falls back to `today + dur` when no prior due date. Transactional.
  - `today` — shortcut for `list --today`.
  - `next` — show the single highest-priority pending item.
  - `stats` — one-screen summary (totals, overdue, due-today/week, pending-by-priority, completions-today/week).
  - `prune --done --older-than <rel>` with `--dry-run` / `-n` — delete stale done items.
  - `undo` — one-level reverse of the most recent mutation (`rm`, `prune`, `edit`, `done`, `undone`, `snooze`). On undo of a recurrence-spawn, the spawned clone is also removed.
  - `export` — full JSON dump (all statuses, regardless of due).
  - `import <file>` — append rows from a JSON file, preserving source `created` / `updated` timestamps; new ids assigned.
- **`add --stdin`.** One todo per input line; shared flags (`--priority`, `--due`, `--tags`, `--notes`, `--recur`) apply to every item. Whole batch runs in one transaction.
- **`Updated` column** in the default pretty table.
- **Indexes** on `(done, due_date, priority)` and `updated_at`.
- **Undo journal.** Single-row `undo_last` table (overwritten on each mutation) captures enough data to reverse the previous action.
- **Migration framework.** `PRAGMA user_version` tracks schema version; migrations are numbered, idempotent functions in an ordered slice.
- **Test suite.** 60 tests total — 42 unit (models + db) + 18 CLI integration in `tests/cli.rs` (spawning the built binary against scratch DBs).

### Changed

- **`list` default table shape** now includes `Tags` and `Updated` columns.
- **Timestamps** stored as RFC 3339 UTC (`YYYY-MM-DDTHH:MM:SSZ`) for new rows. Pretty and `show` views render local time; `--json` / `--parsable` / `--simple` keep the raw UTC form for machine consumption. Legacy naive timestamps are interpreted as UTC.
- **`list --due`** now accepts absolute `YYYY-MM-DD` dates in addition to relative (`1d`, `3w`, `4m`). Previously only relative was accepted.
- **`edit <ID>`** is now required at the clap level (was `Option<i64>` with a manual unwrap).
- **Bulk `done` / `undone` / `rm`** and multi-field `edit` each run in a single transaction. If any operation fails, the whole batch is rolled back — nothing partially commits.
- **`--simple`** is now format-invariant: `--full-date` and `--priority-text` no longer affect it. Previously they would smash the priority label into the content column.
- **`db::list`** SQL now parameterizes every date filter (removed `format!`-interpolated values in the WHERE clause).
- **`cmd_list`'s date math** consolidated into a shared `resolve_date_signed(sign)` helper; removed ~90 lines of duplicated arithmetic.
- **ANSI OVERDUE coloring** is gated on `stdout().is_terminal()`. Piping no longer leaks escape bytes.
- **`row_to_todo` priority fallback** on corrupt rows emits a warning and uses the default priority rather than panicking.
- **`db::open`** propagates `create_dir_all` failures instead of swallowing them (returns `Box<dyn Error>`).
- **Completion candidate help strings** truncated (40 chars content / 30 chars notes) to avoid overflowing shell display.

### Fixed

- **Short-flag collision between `--simple` and `--search`** (both were `-s`). Clap's debug assertion caught it in debug builds but release builds silently stripped the check. `--search` is now long-only; `--simple` keeps `-s`. Running `cargo test` (debug profile) now surfaces any future short-flag conflict at test time.
- **`BrokenPipe` no longer panics.** `reset_sigpipe()` is called at the start of `main()` on Unix, restoring the default SIGPIPE handler so `fazerei list | head -1` exits cleanly.
- **`cmd_prune` capture for undo** initially missed rows hidden by `db::list`'s `due_date >= today` filter; replaced with a dedicated `db::list_done_older_than` query that matches the delete set exactly.

### Removed

- **Tracked `fazerei.db` artifact** (0-byte file) removed from the repository.
- **`Priority::value()`** briefly removed as unused during the first cleanup pass, then restored when JSON serialization needed it.

### Internal

- Added `serde_json = "1"` dependency for JSON output and undo-journal payloads.
- Added `assert_cmd`, `predicates`, `tempfile` as dev-dependencies for CLI integration tests.
- `.gitignore` now ignores `*.db`.

## [0.1.0] — 2026-04-10

Initial version.

- SQLite-backed CLI to-do tool with `add`, `list`, `show`, `edit`, `done`, `undone`, `rm` commands.
- Pretty table output via `tabled`; optional `--simple` output.
- Relative (`0D`, `1W`, `-2M`, `1Y`) and absolute (`YYYY-MM-DD`) date parsing.
- Configurable database location via `--db` or `FAZEREI_DB`.
- Shell completions via `install-completion <shell>` with dynamic to-do id suggestions.
- Bulk ids accepted for `show`, `done`, `undone`, `rm`.
- Priority 1–5 with text labels; optional `--priority-text` and `--full-date` flags for display.
