mod db;
mod models;

use std::path::PathBuf;
use std::process;

use clap::{CommandFactory, Parser, Subcommand, ValueHint};
use clap_complete::engine::{ArgValueCandidates, CompletionCandidate};
use clap_complete::CompleteEnv;
use clap_complete::Shell;
use tabled::{settings::Style, Table};

use models::{
    format_ts_local, normalize_tags, render_json, render_json_value, render_parsable,
    render_simple, Field, Priority, Sort, TodoRow,
};

// ---------------------------------------------------------------------------
// CLI definition
// ---------------------------------------------------------------------------

#[derive(Parser)]
#[command(name = "fazerei", version, about = "A simple CLI to-do app")]
struct Cli {
    /// Path to the SQLite database file.
    /// Defaults to ~/.local/share/fazerei/fazerei.db
    /// Can also be set via FAZEREI_DB env var.
    #[arg(long, global = true, env = "FAZEREI_DB", value_hint = ValueHint::FilePath)]
    db: Option<PathBuf>,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Add a new to-do item
    Add {
        /// The to-do content / description (omitted when --stdin is used)
        #[arg(required_unless_present = "stdin")]
        content: Option<String>,

        /// Priority 1 (highest) to 5 (lowest)
        #[arg(short, long, default_value_t = 3)]
        priority: u8,

        /// Due date: YYYY-MM-DD or relative (0D, 1W, 2M, 1Y, -1D). Defaults to today.
        #[arg(short, long, default_value = "0D", allow_hyphen_values = true)]
        due: String,

        /// Optional notes
        #[arg(short, long)]
        notes: Option<String>,

        /// Tags (comma-separated or repeated -t). Normalized to lowercase.
        #[arg(short, long, value_delimiter = ',')]
        tags: Option<Vec<String>>,

        /// Recurrence spec (e.g. 1D, 1W, 2M, 1Y). When the item is marked
        /// done, a new instance is auto-created with due shifted by this.
        #[arg(short = 'r', long)]
        recur: Option<String>,

        /// Read one to-do per line from stdin (content-only). Shared flags
        /// (priority/due/notes/tags/recur) apply to every item.
        #[arg(long, conflicts_with = "content")]
        stdin: bool,
    },

    /// List to-do items (pending by default)
    List {
        /// Show all items (pending and done)
        #[arg(short, long)]
        all: bool,

        /// Show only completed items
        #[arg(short = 'D', long)]
        done: bool,

        /// Filter by priority level (1-5)
        #[arg(long)]
        priority: Option<u8>,

        /// Show items due on or before this date (YYYY-MM-DD or relative e.g. 1d, 3w, 4m)
        #[arg(short = 'd', long = "due", allow_hyphen_values = true,
              conflicts_with_all = ["overdue", "today", "week"])]
        due: Option<String>,

        /// Shortcut: pending items past their due date
        #[arg(long, conflicts_with_all = ["due", "today", "week", "done", "all"])]
        overdue: bool,

        /// Shortcut: items due today
        #[arg(long, conflicts_with_all = ["due", "overdue", "week"])]
        today: bool,

        /// Shortcut: items due within the next 7 days
        #[arg(long, conflicts_with_all = ["due", "overdue", "today"])]
        week: bool,

        /// Include done items from the past timeframe (e.g., 30d, 2w, 1m)
        #[arg(short = 'p', long, allow_hyphen_values = true)]
        past: Option<String>,

        /// Filter by tag (repeat or comma-separate for OR semantics)
        #[arg(short = 't', long = "tag", value_delimiter = ',')]
        tag: Vec<String>,

        /// Case-insensitive substring search across content and notes
        #[arg(long)]
        search: Option<String>,

        /// Include done items with no due date
        #[arg(long)]
        include_nodate: bool,

        /// Sort order
        #[arg(long, value_enum, default_value_t = Sort::Due)]
        sort: Sort,

        /// Reverse the sort order
        #[arg(short = 'R', long)]
        reverse: bool,

        /// Output only the count of matching items
        #[arg(short = 'c', long = "count", conflicts_with_all = ["simple", "parsable", "json"])]
        count: bool,

        /// Bare-bones display format: "[x] YYYY-MM-DD Content". Great for
        /// status bars and hover menus. Invariant — not affected by
        /// --full-date / --priority-text.
        #[arg(short, long, conflicts_with_all = ["count", "parsable", "json"])]
        simple: bool,

        /// Machine-parsable output. Default fields: id,status,due,content.
        /// Override with --fields. Single-space separated, `-` for nulls,
        /// content / notes always come last.
        #[arg(long, conflicts_with_all = ["count", "simple", "json"])]
        parsable: bool,

        /// Comma-separated list of fields for --parsable
        /// (id,status,priority,due,updated,created,tags,content,notes)
        #[arg(long, value_enum, value_delimiter = ',', requires = "parsable")]
        fields: Option<Vec<Field>>,

        /// JSON array output (one object per item).
        #[arg(long, conflicts_with_all = ["count", "simple", "parsable"])]
        json: bool,

        /// Show day alongside date in the pretty view (e.g., Tuesday, December 31, 2015)
        #[arg(long)]
        full_date: bool,

        /// Show priority as text (e.g., "1 (highest)") in the pretty view
        #[arg(long)]
        priority_text: bool,
    },

    /// Show full details of one or more to-do items
    Show {
        /// Database IDs of the to-do items. Run `fazerei list` to see IDs.
        #[arg(add = ArgValueCandidates::new(todo_id_candidates), required = true)]
        ids: Vec<i64>,
    },

    /// Edit an existing to-do item (only specified fields are updated)
    Edit {
        /// Database ID of the to-do item. Run `fazerei list` to see IDs.
        #[arg(add = ArgValueCandidates::new(todo_id_candidates))]
        id: i64,

        /// New content
        #[arg(short, long)]
        content: Option<String>,

        /// New priority (1-5)
        #[arg(short, long)]
        priority: Option<u8>,

        /// Due date: YYYY-MM-DD, relative (0D, 1W, 2M, -1D), or "none" to clear
        #[arg(short, long, allow_hyphen_values = true)]
        due: Option<String>,

        /// New notes, or "none" to clear
        #[arg(short, long)]
        notes: Option<String>,

        /// Replace tags (comma-separated or repeated -t). Use "none" to clear.
        #[arg(short, long, value_delimiter = ',')]
        tags: Option<Vec<String>>,

        /// Replace recurrence spec (e.g. 1D, 1W). Use "none" to clear.
        #[arg(short = 'r', long)]
        recur: Option<String>,
    },

    /// Mark one or more to-do items as done
    Done {
        /// Database IDs of the to-do items. Run `fazerei list` to see IDs.
        #[arg(add = ArgValueCandidates::new(pending_todo_id_candidates), required = true)]
        ids: Vec<i64>,
    },

    /// Revert one or more to-do items to pending
    Undone {
        /// Database IDs of the to-do items. Run `fazerei list` to see IDs.
        #[arg(add = ArgValueCandidates::new(done_todo_id_candidates), required = true)]
        ids: Vec<i64>,
    },

    /// Delete one or more to-do items permanently
    Rm {
        /// Database IDs of the to-do items. Run `fazerei list` to see IDs.
        #[arg(add = ArgValueCandidates::new(todo_id_candidates), required = true)]
        ids: Vec<i64>,
    },

    /// Shift the due date of one or more items by a relative duration
    Snooze {
        /// Database IDs of the to-do items. Run `fazerei list` to see IDs.
        #[arg(add = ArgValueCandidates::new(pending_todo_id_candidates), required = true)]
        ids: Vec<i64>,

        /// Duration to shift by (e.g. 1D, 1W, 2M, -3D). Forward by default.
        #[arg(short = 'b', long, allow_hyphen_values = true)]
        by: String,
    },

    /// Shortcut: list items due today (alias for `list --today`)
    Today,

    /// Show the single highest-priority pending item
    Next,

    /// Show summary statistics
    Stats,

    /// Delete completed items older than a relative cutoff
    Prune {
        /// Delete only done items (required for safety)
        #[arg(long, required = true)]
        done: bool,

        /// Relative age cutoff (e.g. 30d, 2w, 1m). Done items whose
        /// updated_at is strictly older than this will be removed.
        #[arg(long = "older-than", value_name = "REL")]
        older_than: String,

        /// Preview what would be deleted without removing anything
        #[arg(short = 'n', long)]
        dry_run: bool,
    },

    /// Reverse the most recent mutation (rm, prune, edit, done, undone, snooze)
    Undo,

    /// Dump all to-dos to stdout as a JSON array
    Export,

    /// Import to-dos from a JSON array file (appends; assigns new ids)
    Import {
        /// Path to a JSON file produced by `fazerei export`.
        #[arg(value_hint = ValueHint::FilePath)]
        path: PathBuf,
    },

    /// Install shell tab completions
    InstallCompletion {
        /// Shell to generate completions for
        #[arg(value_enum)]
        shell: Shell,

        /// Custom output path (optional)
        #[arg(short, long, value_hint = ValueHint::FilePath)]
        output: Option<std::path::PathBuf>,
    },
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn default_db_path() -> PathBuf {
    if let Some(proj_dirs) = directories::ProjectDirs::from("", "", "fazerei") {
        proj_dirs.data_dir().join("fazerei.db")
    } else {
        PathBuf::from("fazerei.db")
    }
}

fn resolve_db_path(cli_path: Option<PathBuf>) -> PathBuf {
    cli_path.unwrap_or_else(default_db_path)
}

fn complete_todo_id_impl(filter_done: Option<bool>) -> Vec<CompletionCandidate> {
    let db_path = std::env::var("FAZEREI_DB")
        .map(PathBuf::from)
        .unwrap_or_else(|_| default_db_path());
    let Ok(conn) = db::open(&db_path) else {
        return vec![];
    };
    let sql = match filter_done {
        Some(true) => "SELECT id, content, notes, done, due_date, created_at FROM todos WHERE done = 1 ORDER BY due_date ASC NULLS LAST, priority ASC, created_at DESC",
        Some(false) => "SELECT id, content, notes, done, due_date, created_at FROM todos WHERE done = 0 ORDER BY due_date ASC NULLS LAST, priority ASC, created_at DESC",
        None => "SELECT id, content, notes, done, due_date, created_at FROM todos ORDER BY due_date ASC NULLS LAST, priority ASC, created_at DESC",
    };
    let Ok(mut stmt) = conn.prepare(sql) else {
        return vec![];
    };
    let Ok(rows) = stmt.query_map([], |row| {
        Ok((
            row.get::<_, i64>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, Option<String>>(2)?,
            row.get::<_, bool>(3)?,
            row.get::<_, Option<String>>(4)?,
            row.get::<_, String>(5)?,
        ))
    }) else {
        return vec![];
    };
    rows.filter_map(|r| r.ok())
        .enumerate()
        .map(|(i, (id, content, notes, done, due_date, created_at))| {
            let date_str = due_date.as_deref().unwrap_or("no date");
            let status = if done { "[x]" } else { "[ ]" };
            let content_short = models::truncate(&content, 40);
            let notes_short = notes
                .as_deref()
                .map(|n| models::truncate(n, 30))
                .unwrap_or_else(|| "(no notes)".into());
            let help =
                format!("{date_str} - {status} - {content_short} - {notes_short} - {created_at}");
            CompletionCandidate::new(id.to_string())
                .help(Some(help.into()))
                .display_order(Some(i))
        })
        .collect()
}

fn todo_id_candidates() -> Vec<CompletionCandidate> {
    complete_todo_id_impl(None)
}

fn pending_todo_id_candidates() -> Vec<CompletionCandidate> {
    complete_todo_id_impl(Some(false))
}

fn done_todo_id_candidates() -> Vec<CompletionCandidate> {
    complete_todo_id_impl(Some(true))
}

/// Parse a relative date shorthand (e.g. "0D", "1W", "-2M", "1Y") into its
/// numeric value and unit character.  Returns `None` if the string doesn't
/// match the pattern `^-?\d+[DWMY]$` (case-insensitive).
fn parse_relative(s: &str) -> Option<(i64, char)> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }
    let upper = s.to_uppercase();
    let unit = upper.chars().last()?;
    if !matches!(unit, 'D' | 'W' | 'M' | 'Y') {
        return None;
    }
    let num_part = &upper[..upper.len() - 1];
    if num_part.is_empty() {
        return None;
    }
    let n: i64 = num_part.parse().ok()?;
    Some((n, unit))
}

/// Apply a relative shift of `n` of `unit` to today and return ISO 8601.
fn shift_today(n: i64, unit: char, original: &str) -> Result<String, String> {
    let today = chrono::Local::now().date_naive();
    let target = match unit {
        'D' => today
            .checked_add_signed(chrono::Duration::days(n))
            .ok_or_else(|| format!("date overflow for '{original}'"))?,
        'W' => today
            .checked_add_signed(chrono::Duration::weeks(n))
            .ok_or_else(|| format!("date overflow for '{original}'"))?,
        'M' => {
            if n >= 0 {
                today
                    .checked_add_months(chrono::Months::new(n as u32))
                    .ok_or_else(|| format!("date overflow for '{original}'"))?
            } else {
                today
                    .checked_sub_months(chrono::Months::new(n.unsigned_abs() as u32))
                    .ok_or_else(|| format!("date overflow for '{original}'"))?
            }
        }
        'Y' => {
            let months = n
                .checked_mul(12)
                .ok_or_else(|| format!("date overflow for '{original}'"))?;
            if months >= 0 {
                today
                    .checked_add_months(chrono::Months::new(months as u32))
                    .ok_or_else(|| format!("date overflow for '{original}'"))?
            } else {
                today
                    .checked_sub_months(chrono::Months::new(months.unsigned_abs() as u32))
                    .ok_or_else(|| format!("date overflow for '{original}'"))?
            }
        }
        _ => return Err(format!("invalid unit '{unit}' in '{original}'")),
    };
    Ok(target.format("%Y-%m-%d").to_string())
}

/// Resolve a date string to ISO 8601. Accepts relative (0D, 1W, -2M) or
/// absolute (YYYY-MM-DD). `sign` multiplies relative N (use -1 for --past,
/// which treats positive N as "N ago").
fn resolve_date_signed(s: &str, sign: i64) -> Result<String, String> {
    if let Some((n, unit)) = parse_relative(s) {
        return shift_today(n * sign, unit, s);
    }
    chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d")
        .map(|d| d.format("%Y-%m-%d").to_string())
        .map_err(|_| {
            format!("invalid date '{s}' — expected YYYY-MM-DD or relative (e.g. 0D, 1W, 2M)")
        })
}

/// Resolve a forward-direction date (relative or absolute).
fn resolve_date(s: &str) -> Result<String, String> {
    resolve_date_signed(s, 1)
}

fn fail(msg: &str) -> ! {
    eprintln!("error: {msg}");
    process::exit(1);
}

/// Validate a recurrence spec: must be a positive relative duration
/// (e.g. 1D, 1W, 2M, 1Y). Returns the normalized (uppercased) form.
fn validate_recurrence(s: &str) -> Result<String, String> {
    let (n, unit) = parse_relative(s).ok_or_else(|| {
        format!("invalid recurrence '{s}' — expected e.g. 1D, 1W, 2M, 1Y")
    })?;
    if n <= 0 {
        return Err(format!(
            "recurrence must be positive, got '{s}' ({n}{unit})"
        ));
    }
    Ok(format!("{n}{unit}"))
}

/// Write an undo-journal entry, panicking if it fails. Intended to run
/// inside an existing transaction.
fn write_journal(conn: &rusqlite::Connection, action: &str, payload: &str, summary: &str) {
    db::write_journal(conn, action, payload, summary)
        .unwrap_or_else(|e| fail(&format!("failed to write undo journal: {e}")));
}

/// Given a due date and a recurrence spec, compute the next occurrence date.
fn next_occurrence(due: &str, rec: &str) -> Option<String> {
    let base = chrono::NaiveDate::parse_from_str(due, "%Y-%m-%d").ok()?;
    let (n, unit) = parse_relative(rec)?;
    let target = match unit {
        'D' => base.checked_add_signed(chrono::Duration::days(n))?,
        'W' => base.checked_add_signed(chrono::Duration::weeks(n))?,
        'M' => base.checked_add_months(chrono::Months::new(n as u32))?,
        'Y' => base.checked_add_months(chrono::Months::new((n * 12) as u32))?,
        _ => return None,
    };
    Some(target.format("%Y-%m-%d").to_string())
}

// ---------------------------------------------------------------------------
// Command handlers
// ---------------------------------------------------------------------------

#[allow(clippy::too_many_arguments)]
fn cmd_add(
    conn: &mut rusqlite::Connection,
    content: Option<String>,
    priority: u8,
    due: String,
    notes: Option<String>,
    tags: Option<Vec<String>>,
    recur: Option<String>,
    stdin: bool,
) {
    if let Err(e) = Priority::new(priority) {
        fail(&e);
    }
    let resolved = match resolve_date(&due) {
        Ok(d) => d,
        Err(e) => fail(&e),
    };
    let tags_stored = tags.and_then(normalize_tags);
    let recurrence = match recur.as_deref() {
        Some(s) => Some(validate_recurrence(s).unwrap_or_else(|e| fail(&e))),
        None => None,
    };

    if stdin {
        use std::io::BufRead;
        let stdin = std::io::stdin();
        let lines: Vec<String> = stdin
            .lock()
            .lines()
            .filter_map(|l| l.ok())
            .map(|l| l.trim().to_string())
            .filter(|l| !l.is_empty())
            .collect();
        if lines.is_empty() {
            eprintln!("No input lines read from stdin.");
            return;
        }
        let tx = conn
            .transaction()
            .unwrap_or_else(|e| fail(&format!("failed to start transaction: {e}")));
        let mut added = Vec::with_capacity(lines.len());
        for line in lines {
            let id = db::add(
                &tx,
                &line,
                priority,
                Some(&resolved),
                notes.as_deref(),
                tags_stored.as_deref(),
                recurrence.as_deref(),
            )
            .unwrap_or_else(|e| fail(&format!("failed to add to-do: {e}")));
            added.push(id);
        }
        tx.commit()
            .unwrap_or_else(|e| fail(&format!("failed to commit: {e}")));
        for id in added {
            println!("Added to-do #{id}");
        }
        return;
    }

    let content = content.unwrap_or_else(|| fail("content required"));
    match db::add(
        conn,
        &content,
        priority,
        Some(&resolved),
        notes.as_deref(),
        tags_stored.as_deref(),
        recurrence.as_deref(),
    ) {
        Ok(id) => println!("Added to-do #{id}"),
        Err(e) => fail(&format!("failed to add to-do: {e}")),
    }
}

#[allow(clippy::too_many_arguments)]
fn cmd_list(
    conn: &rusqlite::Connection,
    all: bool,
    done: bool,
    priority: Option<u8>,
    due: Option<String>,
    overdue: bool,
    today_flag: bool,
    week: bool,
    past: Option<String>,
    tag: Vec<String>,
    search: Option<String>,
    include_nodate: bool,
    sort: Sort,
    reverse: bool,
    count: bool,
    simple: bool,
    parsable: bool,
    fields: Option<Vec<Field>>,
    json: bool,
    full_date: bool,
    priority_text: bool,
) {
    if let Some(p) = priority {
        if let Err(e) = Priority::new(p) {
            fail(&e);
        }
    }

    // Status scope. --overdue forces pending-only (clap already excludes --done/--all).
    let (show_pending, show_done) = if overdue {
        (true, false)
    } else {
        (!done, done || all)
    };

    // Date range: shortcuts override --due.
    let today = chrono::Local::now().date_naive();
    let (due_before, due_from) = if overdue {
        let yesterday = today - chrono::Duration::days(1);
        (Some(yesterday.format("%Y-%m-%d").to_string()), None)
    } else if today_flag {
        let d = today.format("%Y-%m-%d").to_string();
        (Some(d.clone()), Some(d))
    } else if week {
        let week_out = today + chrono::Duration::days(7);
        (
            Some(week_out.format("%Y-%m-%d").to_string()),
            Some(today.format("%Y-%m-%d").to_string()),
        )
    } else {
        let due_before = due
            .as_deref()
            .map(resolve_date)
            .transpose()
            .unwrap_or_else(|e| fail(&e));
        (due_before, None)
    };

    let done_since = past
        .as_deref()
        .map(|s| resolve_date_signed(s, -1))
        .transpose()
        .unwrap_or_else(|e| fail(&e));

    // Normalize tag filter to lowercase, dedupe.
    let tags_any: Vec<String> = {
        let mut v: Vec<String> = tag
            .into_iter()
            .map(|s| s.trim().to_lowercase())
            .filter(|s| !s.is_empty())
            .collect();
        v.sort();
        v.dedup();
        v
    };

    let mut todos = db::list(
        conn,
        show_pending,
        show_done,
        priority,
        due_before.as_deref(),
        due_from.as_deref(),
        done_since.as_deref(),
        include_nodate,
        &tags_any,
        search.as_deref(),
        sort,
    )
    .unwrap_or_else(|e| fail(&format!("failed to list to-dos: {e}")));

    if reverse {
        todos.reverse();
    }

    if count {
        println!("{}", todos.len());
        return;
    }
    if json {
        let objs: Vec<String> = todos.iter().map(render_json).collect();
        println!("[{}]", objs.join(","));
        return;
    }
    if todos.is_empty() {
        if !parsable && !simple {
            println!("No to-do items found.");
        }
        return;
    }
    if parsable {
        let effective = fields.unwrap_or_else(Field::defaults);
        for t in &todos {
            println!("{}", render_parsable(t, &effective));
        }
        return;
    }
    if simple {
        for t in &todos {
            println!("{}", render_simple(t));
        }
        return;
    }
    let rows: Vec<TodoRow> = todos
        .iter()
        .map(|t| TodoRow::new(t, full_date, priority_text))
        .collect();
    let table = Table::new(rows).with(Style::rounded()).to_string();
    println!("{table}");
}

fn cmd_show_single(conn: &rusqlite::Connection, id: i64) -> Result<(), String> {
    match db::get(conn, id) {
        Ok(Some(t)) => {
            println!("ID:        {}", t.id);
            println!("Status:    {}", if t.done { "Done" } else { "Pending" });
            println!("Priority:  {}", t.priority.label());
            println!("Content:   {}", t.content);
            println!("Due:       {}", t.due_date.as_deref().unwrap_or("—"));
            let tags_v = t.tags_vec();
            let tags_str = if tags_v.is_empty() {
                "—".to_string()
            } else {
                tags_v.join(", ")
            };
            println!("Tags:      {tags_str}");
            println!("Recur:     {}", t.recurrence.as_deref().unwrap_or("—"));
            println!("Notes:     {}", t.notes.as_deref().unwrap_or("—"));
            println!("Created:   {}", format_ts_local(&t.created_at));
            println!("Updated:   {}", format_ts_local(&t.updated_at));
            Ok(())
        }
        Ok(None) => Err(format!("to-do #{id} not found")),
        Err(e) => Err(format!("failed to fetch to-do: {e}")),
    }
}

fn cmd_show_multi(conn: &rusqlite::Connection, ids: Vec<i64>) {
    let mut errors = Vec::new();
    for (i, id) in ids.iter().enumerate() {
        if i > 0 {
            println!();
        }
        match cmd_show_single(conn, *id) {
            Ok(()) => {}
            Err(e) => errors.push((*id, e)),
        }
    }
    if !errors.is_empty() {
        eprintln!("Errors occurred:");
        for (id, err) in errors {
            eprintln!("  #{id}: {err}");
        }
        process::exit(1);
    }
}

#[allow(clippy::too_many_arguments)]
fn cmd_edit(
    conn: &mut rusqlite::Connection,
    id: i64,
    content: Option<String>,
    priority: Option<u8>,
    due: Option<String>,
    notes: Option<String>,
    tags: Option<Vec<String>>,
    recur: Option<String>,
) {
    if content.is_none()
        && priority.is_none()
        && due.is_none()
        && notes.is_none()
        && tags.is_none()
        && recur.is_none()
    {
        eprintln!("Nothing to update. Specify at least one field to change.");
        eprintln!("Run `fazerei edit --help` for usage.");
        process::exit(1);
    }

    // Pre-validate: the row exists and the values are valid. This happens
    // outside the transaction so we fail fast without an open tx.
    let before = match db::get(conn, id) {
        Ok(Some(t)) => t,
        Ok(None) => fail(&format!("to-do #{id} not found")),
        Err(e) => fail(&format!("failed to fetch to-do: {e}")),
    };
    if let Some(p) = priority {
        if let Err(e) = Priority::new(p) {
            fail(&e);
        }
    }
    let resolved_due = due.as_deref().map(|d| {
        if d.eq_ignore_ascii_case("none") {
            Ok::<Option<String>, String>(None)
        } else {
            resolve_date(d).map(Some)
        }
    });
    let resolved_due = match resolved_due {
        Some(Ok(v)) => Some(v),
        Some(Err(e)) => fail(&e),
        None => None,
    };
    let resolved_recur = match recur.as_deref() {
        Some(s) if s.eq_ignore_ascii_case("none") => Some(None),
        Some(s) => Some(Some(
            validate_recurrence(s).unwrap_or_else(|e| fail(&e)),
        )),
        None => None,
    };

    let tx = conn
        .transaction()
        .unwrap_or_else(|e| fail(&format!("failed to start transaction: {e}")));

    if let Some(c) = content.as_ref() {
        db::update_content(&tx, id, c)
            .unwrap_or_else(|e| fail(&format!("failed to update content: {e}")));
    }
    if let Some(p) = priority {
        db::update_priority(&tx, id, p)
            .unwrap_or_else(|e| fail(&format!("failed to update priority: {e}")));
    }
    if let Some(due_val) = resolved_due.as_ref() {
        db::update_due_date(&tx, id, due_val.as_deref())
            .unwrap_or_else(|e| fail(&format!("failed to update due date: {e}")));
    }
    if let Some(n) = notes.as_ref() {
        let notes_val = if n.eq_ignore_ascii_case("none") {
            None
        } else {
            Some(n.as_str())
        };
        db::update_notes(&tx, id, notes_val)
            .unwrap_or_else(|e| fail(&format!("failed to update notes: {e}")));
    }
    if let Some(t) = tags.as_ref() {
        // A single literal "none" entry clears all tags.
        let is_clear = t.len() == 1 && t[0].eq_ignore_ascii_case("none");
        let stored = if is_clear {
            None
        } else {
            normalize_tags(t.iter().cloned())
        };
        db::update_tags(&tx, id, stored.as_deref())
            .unwrap_or_else(|e| fail(&format!("failed to update tags: {e}")));
    }
    if let Some(rec_opt) = resolved_recur.as_ref() {
        db::update_recurrence(&tx, id, rec_opt.as_deref())
            .unwrap_or_else(|e| fail(&format!("failed to update recurrence: {e}")));
    }

    write_journal(
        &tx,
        "edit",
        &serde_json::json!({"before": render_json_value(&before)}).to_string(),
        &format!("edit #{id}"),
    );

    tx.commit()
        .unwrap_or_else(|e| fail(&format!("failed to commit: {e}")));
    println!("Updated to-do #{id}");
}

fn cmd_done_multi(conn: &mut rusqlite::Connection, ids: Vec<i64>, done_state: bool) {
    let tx = conn
        .transaction()
        .unwrap_or_else(|e| fail(&format!("failed to start transaction: {e}")));

    let mut errors = Vec::new();
    let mut succeeded = Vec::new();
    let mut spawned = Vec::new();

    for id in &ids {
        match db::get(&tx, *id) {
            Ok(None) => errors.push((*id, "not found".to_string())),
            Err(e) => errors.push((*id, format!("failed to fetch: {e}"))),
            Ok(Some(_)) => {
                let res = if done_state {
                    db::complete_with_recurrence(&tx, *id, |due, rec| next_occurrence(due, rec))
                        .map(|opt_new_id| {
                            if let Some(new_id) = opt_new_id {
                                spawned.push((*id, new_id));
                            }
                        })
                } else {
                    db::set_done(&tx, *id, false).map(|_| ())
                };
                match res {
                    Ok(()) => succeeded.push(*id),
                    Err(e) => errors.push((*id, format!("failed: {e}"))),
                }
            }
        }
    }

    if !errors.is_empty() {
        drop(tx);
        eprintln!("No changes were committed. Errors:");
        for (id, err) in errors {
            eprintln!("  #{id}: {err}");
        }
        process::exit(1);
    }

    let action = if done_state { "done" } else { "undone" };
    write_journal(
        &tx,
        action,
        &serde_json::json!({"ids": &succeeded, "spawned": spawned.iter().map(|(_, new)| *new).collect::<Vec<_>>()}).to_string(),
        &format!("{action} {}", succeeded.len()),
    );

    tx.commit()
        .unwrap_or_else(|e| fail(&format!("failed to commit: {e}")));

    let verb = if done_state { "done" } else { "pending" };
    for id in &succeeded {
        println!("Marked to-do #{id} as {verb}");
    }
    for (src, new) in &spawned {
        println!("Recurrence: spawned #{new} from #{src}");
    }
}

fn cmd_rm_multi(conn: &mut rusqlite::Connection, ids: Vec<i64>) {
    let tx = conn
        .transaction()
        .unwrap_or_else(|e| fail(&format!("failed to start transaction: {e}")));

    let mut errors = Vec::new();
    let mut captured: Vec<serde_json::Value> = Vec::new();

    for id in &ids {
        match db::get(&tx, *id) {
            Ok(None) => errors.push((*id, "not found".to_string())),
            Err(e) => errors.push((*id, format!("failed to fetch: {e}"))),
            Ok(Some(t)) => {
                captured.push(render_json_value(&t));
                if let Err(e) = db::delete(&tx, *id) {
                    errors.push((*id, format!("failed: {e}")));
                }
            }
        }
    }

    if !errors.is_empty() {
        drop(tx);
        eprintln!("No changes were committed. Errors:");
        for (id, err) in errors {
            eprintln!("  #{id}: {err}");
        }
        process::exit(1);
    }

    write_journal(
        &tx,
        "rm",
        &serde_json::json!({"rows": captured}).to_string(),
        &format!("rm {}", ids.len()),
    );

    tx.commit()
        .unwrap_or_else(|e| fail(&format!("failed to commit: {e}")));

    for id in ids {
        println!("Deleted to-do #{id}");
    }
}

fn cmd_snooze(conn: &mut rusqlite::Connection, ids: Vec<i64>, by: String) {
    let (n, unit) = parse_relative(&by).unwrap_or_else(|| {
        fail(&format!(
            "invalid --by '{by}' — expected relative format (e.g. 1D, 1W, 2M, -3D)"
        ))
    });

    let tx = conn
        .transaction()
        .unwrap_or_else(|e| fail(&format!("failed to start transaction: {e}")));

    let mut errors = Vec::new();
    let mut updates: Vec<(i64, String, String)> = Vec::new();

    for id in &ids {
        match db::get(&tx, *id) {
            Ok(None) => errors.push((*id, "not found".to_string())),
            Err(e) => errors.push((*id, format!("failed to fetch: {e}"))),
            Ok(Some(t)) => {
                let base = t
                    .due_date
                    .as_deref()
                    .and_then(|d| chrono::NaiveDate::parse_from_str(d, "%Y-%m-%d").ok())
                    .unwrap_or_else(|| chrono::Local::now().date_naive());
                let shifted = match unit {
                    'D' => base.checked_add_signed(chrono::Duration::days(n)),
                    'W' => base.checked_add_signed(chrono::Duration::weeks(n)),
                    'M' => {
                        if n >= 0 {
                            base.checked_add_months(chrono::Months::new(n as u32))
                        } else {
                            base.checked_sub_months(chrono::Months::new(n.unsigned_abs() as u32))
                        }
                    }
                    'Y' => {
                        let months = match n.checked_mul(12) {
                            Some(m) => m,
                            None => {
                                errors.push((*id, format!("date overflow for '{by}'")));
                                continue;
                            }
                        };
                        if months >= 0 {
                            base.checked_add_months(chrono::Months::new(months as u32))
                        } else {
                            base.checked_sub_months(
                                chrono::Months::new(months.unsigned_abs() as u32),
                            )
                        }
                    }
                    _ => unreachable!("parse_relative guarantees unit"),
                };
                let Some(new_date) = shifted else {
                    errors.push((*id, format!("date overflow for '{by}'")));
                    continue;
                };
                let old = t.due_date.clone().unwrap_or_else(|| "—".into());
                let new_str = new_date.format("%Y-%m-%d").to_string();
                if let Err(e) = db::update_due_date(&tx, *id, Some(&new_str)) {
                    errors.push((*id, format!("failed: {e}")));
                } else {
                    updates.push((*id, old, new_str));
                }
            }
        }
    }

    if !errors.is_empty() {
        drop(tx);
        eprintln!("No changes were committed. Errors:");
        for (id, err) in errors {
            eprintln!("  #{id}: {err}");
        }
        process::exit(1);
    }

    let journal_payload = serde_json::json!({
        "changes": updates.iter().map(|(id, old, _)| {
            let prev = if old == "—" { serde_json::Value::Null } else { serde_json::Value::String(old.clone()) };
            serde_json::json!({"id": id, "prev_due": prev})
        }).collect::<Vec<_>>()
    })
    .to_string();
    write_journal(
        &tx,
        "snooze",
        &journal_payload,
        &format!("snooze {} by {by}", updates.len()),
    );

    tx.commit()
        .unwrap_or_else(|e| fail(&format!("failed to commit: {e}")));

    for (id, old, new) in updates {
        println!("Snoozed #{id}: {old} -> {new}");
    }
}

fn cmd_next(conn: &rusqlite::Connection) {
    let todos = db::list(
        conn,
        true,
        false,
        None,
        None,
        None,
        None,
        false,
        &[],
        None,
        Sort::Priority,
    )
    .unwrap_or_else(|e| fail(&format!("failed: {e}")));
    let Some(t) = todos.first() else {
        println!("No pending items.");
        return;
    };
    println!("ID:        {}", t.id);
    println!("Priority:  {}", t.priority.label());
    println!("Content:   {}", t.content);
    println!("Due:       {}", t.due_date.as_deref().unwrap_or("—"));
    let tags = t.tags_vec();
    if !tags.is_empty() {
        println!("Tags:      {}", tags.join(", "));
    }
    if let Some(n) = t.notes.as_deref() {
        println!("Notes:     {n}");
    }
}

fn cmd_stats(conn: &rusqlite::Connection) {
    let (pending, done) = db::count_by_status(conn)
        .unwrap_or_else(|e| fail(&format!("failed: {e}")));
    let by_pri = db::count_pending_by_priority(conn)
        .unwrap_or_else(|e| fail(&format!("failed: {e}")));

    let today = chrono::Local::now().date_naive();
    let today_s = today.format("%Y-%m-%d").to_string();
    let week_s = (today + chrono::Duration::days(7))
        .format("%Y-%m-%d")
        .to_string();
    let week_ago_s = (today - chrono::Duration::days(7))
        .format("%Y-%m-%d")
        .to_string();

    let overdue = db::count_overdue(conn, &today_s)
        .unwrap_or_else(|e| fail(&format!("failed: {e}")));
    let due_today = db::count_due_range(conn, &today_s, &today_s)
        .unwrap_or_else(|e| fail(&format!("failed: {e}")));
    let due_week = db::count_due_range(conn, &today_s, &week_s)
        .unwrap_or_else(|e| fail(&format!("failed: {e}")));
    let completed_today = db::count_completed_since(conn, &today_s)
        .unwrap_or_else(|e| fail(&format!("failed: {e}")));
    let completed_week = db::count_completed_since(conn, &week_ago_s)
        .unwrap_or_else(|e| fail(&format!("failed: {e}")));

    println!("Total:      {}  ({pending} pending, {done} done)", pending + done);
    println!("Overdue:    {overdue}");
    println!("Due today:  {due_today}");
    println!("Due week:   {due_week}");
    println!("By priority (pending):");
    let labels = ["1 (highest)", "2 (high)", "3 (medium)", "4 (low)", "5 (lowest)"];
    for (i, count) in by_pri.iter().enumerate() {
        if *count > 0 {
            println!("  {:<12} {count}", labels[i]);
        }
    }
    println!("Completed today:     {completed_today}");
    println!("Completed this week: {completed_week}");
}

fn cmd_prune(conn: &mut rusqlite::Connection, older_than: String, dry_run: bool) {
    let cutoff = resolve_date_signed(&older_than, -1)
        .unwrap_or_else(|e| fail(&format!("invalid --older-than: {e}")));

    if dry_run {
        let n = db::count_done_older_than(conn, &cutoff)
            .unwrap_or_else(|e| fail(&format!("failed: {e}")));
        println!("Would delete {n} done item(s) older than {cutoff}.");
        return;
    }

    let tx = conn
        .transaction()
        .unwrap_or_else(|e| fail(&format!("failed to start transaction: {e}")));

    // Capture rows before we delete them so undo can restore them.
    let to_delete = db::list_done_older_than(&tx, &cutoff)
        .unwrap_or_else(|e| fail(&format!("failed: {e}")));
    let captured: Vec<serde_json::Value> = to_delete.iter().map(render_json_value).collect();

    let removed = db::delete_done_older_than(&tx, &cutoff)
        .unwrap_or_else(|e| fail(&format!("failed: {e}")));

    if removed > 0 {
        write_journal(
            &tx,
            "prune",
            &serde_json::json!({"rows": captured}).to_string(),
            &format!("prune {removed}"),
        );
    }

    tx.commit()
        .unwrap_or_else(|e| fail(&format!("failed to commit: {e}")));
    println!("Deleted {removed} done item(s) older than {cutoff}.");
}

fn cmd_undo(conn: &mut rusqlite::Connection) {
    let entry = db::read_journal(conn)
        .unwrap_or_else(|e| fail(&format!("failed to read journal: {e}")));
    let Some((action, payload, summary)) = entry else {
        println!("Nothing to undo.");
        return;
    };
    let parsed: serde_json::Value = serde_json::from_str(&payload)
        .unwrap_or_else(|e| fail(&format!("corrupt journal payload: {e}")));

    let tx = conn
        .transaction()
        .unwrap_or_else(|e| fail(&format!("failed to start transaction: {e}")));

    match action.as_str() {
        "rm" | "prune" => {
            let rows = parsed["rows"].as_array().cloned().unwrap_or_default();
            for row in &rows {
                restore_row(&tx, row);
            }
            db::clear_journal(&tx)
                .unwrap_or_else(|e| fail(&format!("failed to clear journal: {e}")));
            tx.commit()
                .unwrap_or_else(|e| fail(&format!("failed to commit: {e}")));
            println!("Undone: restored {} item(s) from '{summary}'", rows.len());
        }
        "edit" | "snooze" => {
            if action == "edit" {
                let before = &parsed["before"];
                restore_row(&tx, before);
            } else {
                // snooze
                let empty = Vec::new();
                let changes = parsed["changes"].as_array().unwrap_or(&empty);
                for ch in changes {
                    let id = ch["id"].as_i64().unwrap_or(0);
                    let prev = ch["prev_due"].as_str();
                    db::update_due_date(&tx, id, prev)
                        .unwrap_or_else(|e| fail(&format!("failed to restore due: {e}")));
                }
            }
            db::clear_journal(&tx)
                .unwrap_or_else(|e| fail(&format!("failed to clear journal: {e}")));
            tx.commit()
                .unwrap_or_else(|e| fail(&format!("failed to commit: {e}")));
            println!("Undone: reversed '{summary}'");
        }
        "done" | "undone" => {
            let empty = Vec::new();
            let ids = parsed["ids"].as_array().unwrap_or(&empty);
            let spawned = parsed["spawned"].as_array().unwrap_or(&empty);
            // Flip state back.
            let target = action == "undone"; // undoing 'undone' means set done=true
            for idv in ids {
                if let Some(id) = idv.as_i64() {
                    db::set_done(&tx, id, target)
                        .unwrap_or_else(|e| fail(&format!("failed: {e}")));
                }
            }
            // Remove any recurrence clones that were spawned by the original 'done'.
            for idv in spawned {
                if let Some(id) = idv.as_i64() {
                    db::delete(&tx, id).ok();
                }
            }
            db::clear_journal(&tx)
                .unwrap_or_else(|e| fail(&format!("failed to clear journal: {e}")));
            tx.commit()
                .unwrap_or_else(|e| fail(&format!("failed to commit: {e}")));
            println!("Undone: reversed '{summary}'");
        }
        other => fail(&format!("unknown journal action: {other}")),
    }
}

/// Restore a row from a JSON object (as produced by `render_json_value`).
fn restore_row(conn: &rusqlite::Connection, v: &serde_json::Value) {
    let id = v["id"].as_i64().unwrap_or_else(|| fail("restore: missing id"));
    let content = v["content"].as_str().unwrap_or("");
    let priority = v["priority"].as_u64().unwrap_or(3) as u8;
    let done = v["status"].as_str() == Some("done");
    let due = v["due"].as_str();
    let notes = v["notes"].as_str();
    let tags_vec: Vec<String> = v["tags"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|x| x.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();
    let tags_stored = if tags_vec.is_empty() {
        None
    } else {
        Some(format!(",{},", tags_vec.join(",")))
    };
    let recurrence = v["recurrence"].as_str();
    let created = v["created"].as_str().unwrap_or("");
    let updated = v["updated"].as_str().unwrap_or("");
    db::upsert_row(
        conn,
        id,
        content,
        priority,
        done,
        due,
        notes,
        tags_stored.as_deref(),
        recurrence,
        created,
        updated,
    )
    .unwrap_or_else(|e| fail(&format!("failed to restore row #{id}: {e}")));
}

fn cmd_export(conn: &rusqlite::Connection) {
    let todos = db::list(
        conn,
        true,
        true,
        None,
        None,
        None,
        None,
        true,
        &[],
        None,
        Sort::Created,
    )
    .unwrap_or_else(|e| fail(&format!("failed: {e}")));
    let objs: Vec<String> = todos.iter().map(render_json).collect();
    println!("[{}]", objs.join(","));
}

fn cmd_import(conn: &mut rusqlite::Connection, path: PathBuf) {
    let raw = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| fail(&format!("failed to read {}: {e}", path.display())));
    let parsed: serde_json::Value =
        serde_json::from_str(&raw).unwrap_or_else(|e| fail(&format!("invalid JSON: {e}")));
    let arr = parsed
        .as_array()
        .unwrap_or_else(|| fail("expected a JSON array at the top level"));

    let tx = conn
        .transaction()
        .unwrap_or_else(|e| fail(&format!("failed to start transaction: {e}")));

    let mut count = 0;
    for v in arr {
        let content = v["content"].as_str().unwrap_or_else(|| fail("missing content"));
        let priority = v["priority"].as_u64().unwrap_or(3) as u8;
        let done = v["status"].as_str() == Some("done");
        let due = v["due"].as_str();
        let notes = v["notes"].as_str();
        let tags_vec: Vec<String> = v["tags"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|x| x.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default();
        let tags_stored = if tags_vec.is_empty() {
            None
        } else {
            Some(format!(",{},", tags_vec.join(",")))
        };
        let recurrence = v["recurrence"].as_str();
        let now_str = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
        let created = v["created"].as_str().unwrap_or(&now_str);
        let updated = v["updated"].as_str().unwrap_or(&now_str);
        db::insert_full(
            &tx,
            content,
            priority,
            done,
            due,
            notes,
            tags_stored.as_deref(),
            recurrence,
            created,
            updated,
        )
        .unwrap_or_else(|e| fail(&format!("insert failed: {e}")));
        count += 1;
    }
    tx.commit()
        .unwrap_or_else(|e| fail(&format!("failed to commit: {e}")));
    println!("Imported {count} item(s).");
}

fn cmd_install_completion(shell: Shell, output: Option<std::path::PathBuf>) {
    let bin_name = "fazerei";

    let completion_line = match shell {
        Shell::Zsh => format!("source <(COMPLETE=zsh {})", bin_name),
        Shell::Bash => format!("source <(COMPLETE=bash {})", bin_name),
        Shell::Fish => format!("COMPLETE=fish {} | source", bin_name),
        Shell::PowerShell => format!(
            "$env:COMPLETE = \"powershell\"; {} | Out-String | Invoke-Expression",
            bin_name
        ),
        Shell::Elvish => format!("eval (COMPLETE=elvish {} | slurp)", bin_name),
        _ => fail("unsupported shell"),
    };

    let nosort_line = "zstyle ':completion:*:*:fazerei:*:*' sort false";

    if let Some(path) = output {
        let file_content = match shell {
            Shell::Fish => completion_line.clone(),
            Shell::Zsh => format!(
                "# fazerei completion\n{}\n{}\n",
                completion_line, nosort_line
            ),
            _ => format!("# fazerei completion\n{}\n", completion_line),
        };

        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .unwrap_or_else(|e| fail(&format!("failed to create directory: {}", e)));
        }

        std::fs::write(&path, &file_content)
            .unwrap_or_else(|e| fail(&format!("failed to write file: {}", e)));

        println!("Completion script written to: {}", path.display());
    } else {
        match shell {
            Shell::Zsh => {
                println!("Add the following lines to ~/.zshrc AFTER compinit:\n");
                println!("  {}", completion_line);
                println!("  {}\n", nosort_line);
                println!(
                    "For example:\n\n  \
                     autoload -U compinit\n  \
                     compinit\n  \
                     {}\n  \
                     {}\n",
                    completion_line, nosort_line
                );
            }
            Shell::Bash => {
                println!("Add the following line to ~/.bashrc AFTER any bash-completion setup:\n");
                println!("  {}\n", completion_line);
            }
            Shell::Fish => {
                println!("Add the following line to ~/.config/fish/config.fish:\n");
                println!("  {}\n", completion_line);
            }
            Shell::PowerShell => {
                println!("Add the following line to your PowerShell profile:\n");
                println!("  {}\n", completion_line);
            }
            Shell::Elvish => {
                println!("Add the following line to ~/.elvish/rc.elv:\n");
                println!("  {}\n", completion_line);
            }
            _ => fail("unsupported shell"),
        }
        println!("Then restart your shell or source the config file.");
    }
}

// ---------------------------------------------------------------------------
// Main
// ---------------------------------------------------------------------------

/// Restore the default SIGPIPE handler so that closing the reader side of a
/// pipe (e.g. `fazerei list | head -1`) terminates us cleanly instead of
/// triggering Rust's default "Broken pipe" panic from println!.
#[cfg(unix)]
fn reset_sigpipe() {
    // SAFETY: signal(2) with SIG_DFL is safe; no allocations or unwind.
    unsafe {
        // SIGPIPE = 13, SIG_DFL = 0 on all supported unix platforms. Avoid a
        // libc dep by hardcoding the constants.
        let _ = libc_signal(13, 0);
    }
}

#[cfg(not(unix))]
fn reset_sigpipe() {}

#[cfg(unix)]
extern "C" {
    #[link_name = "signal"]
    fn libc_signal(signum: i32, handler: usize) -> usize;
}

fn main() {
    reset_sigpipe();
    CompleteEnv::with_factory(Cli::command).complete();
    let cli = Cli::parse();
    let db_path = resolve_db_path(cli.db);

    let mut conn = db::open(&db_path).unwrap_or_else(|e| {
        fail(&format!(
            "failed to open database at {}: {e}",
            db_path.display()
        ))
    });

    match cli.command {
        Commands::Add {
            content,
            priority,
            due,
            notes,
            tags,
            recur,
            stdin,
        } => {
            cmd_add(&mut conn, content, priority, due, notes, tags, recur, stdin);
        }
        Commands::List {
            all,
            done,
            priority,
            due,
            overdue,
            today,
            week,
            past,
            tag,
            search,
            include_nodate,
            sort,
            reverse,
            count,
            simple,
            parsable,
            fields,
            json,
            full_date,
            priority_text,
        } => {
            cmd_list(
                &conn,
                all,
                done,
                priority,
                due,
                overdue,
                today,
                week,
                past,
                tag,
                search,
                include_nodate,
                sort,
                reverse,
                count,
                simple,
                parsable,
                fields,
                json,
                full_date,
                priority_text,
            );
        }
        Commands::Show { ids } => {
            cmd_show_multi(&conn, ids);
        }
        Commands::Edit {
            id,
            content,
            priority,
            due,
            notes,
            tags,
            recur,
        } => {
            cmd_edit(&mut conn, id, content, priority, due, notes, tags, recur);
        }
        Commands::Done { ids } => {
            cmd_done_multi(&mut conn, ids, true);
        }
        Commands::Undone { ids } => {
            cmd_done_multi(&mut conn, ids, false);
        }
        Commands::Rm { ids } => {
            cmd_rm_multi(&mut conn, ids);
        }
        Commands::Snooze { ids, by } => {
            cmd_snooze(&mut conn, ids, by);
        }
        Commands::Today => {
            cmd_list(
                &conn,
                false, false, None, None, false, true, false, None,
                Vec::new(), None, false, Sort::Due, false, false, false, false, None, false, false, false,
            );
        }
        Commands::Next => {
            cmd_next(&conn);
        }
        Commands::Stats => {
            cmd_stats(&conn);
        }
        Commands::Prune {
            done: _,
            older_than,
            dry_run,
        } => {
            cmd_prune(&mut conn, older_than, dry_run);
        }
        Commands::Undo => {
            cmd_undo(&mut conn);
        }
        Commands::Export => {
            cmd_export(&conn);
        }
        Commands::Import { path } => {
            cmd_import(&mut conn, path);
        }
        Commands::InstallCompletion { shell, output } => {
            cmd_install_completion(shell, output);
        }
    }
}
