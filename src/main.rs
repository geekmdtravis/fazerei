mod db;
mod models;

use std::path::PathBuf;
use std::process;

use clap::{CommandFactory, Parser, Subcommand, ValueHint};
use clap_complete::engine::{ArgValueCandidates, CompletionCandidate};
use clap_complete::CompleteEnv;
use clap_complete::Shell;
use tabled::{settings::Style, Table};

use models::{Priority, TodoRow};

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
        /// The to-do content / description
        content: String,

        /// Priority 1 (highest) to 5 (lowest)
        #[arg(short, long, default_value_t = 3)]
        priority: u8,

        /// Due date: YYYY-MM-DD or relative (0D, 1W, 2M, 1Y, -1D). Defaults to today.
        #[arg(short, long, default_value = "0D", allow_hyphen_values = true)]
        due: String,

        /// Optional notes
        #[arg(short, long)]
        notes: Option<String>,
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

        /// Show items due within this timeframe (e.g., 0d, 1d, 3w, 4m)
        #[arg(short = 'd', long = "due", allow_hyphen_values = true)]
        due: Option<String>,

        /// Include done items from the past timeframe (e.g., 30d, 2w, 1m)
        #[arg(short = 'p', long, allow_hyphen_values = true)]
        past: Option<String>,

        /// Include done items with no due date
        #[arg(long)]
        include_nodate: bool,

        /// Output only the count of matching items
        #[arg(short = 'c', long = "count")]
        count: bool,

        /// Simple output format for piping: "[x] - YYYY-MM-DD - ID - Content"
        #[arg(short, long)]
        simple: bool,

        /// Show day alongside date (e.g., Tuesday, December 31, 2015)
        #[arg(long)]
        full_date: bool,

        /// Show priority as text (e.g., "1 (highest)")
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
        id: Option<i64>,

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
        // Fallback: current directory
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
            let notes_str = notes.as_deref().unwrap_or("(no notes)");
            let help = format!("{date_str} - {status} - {content} - {notes_str} - {created_at}");
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

/// Resolve a date string to an ISO 8601 date (YYYY-MM-DD).
///
/// Accepted formats:
///   - Relative shorthand: `0D` (today), `1D` (tomorrow), `-1D` (yesterday),
///     `1W` (+7 days), `2M` (+2 months), `1Y` (+1 year), etc.
///   - Absolute ISO 8601: `2026-04-15`
fn resolve_date(s: &str) -> Result<String, String> {
    // Try relative shorthand first.
    if let Some((n, unit)) = parse_relative(s) {
        let today = chrono::Local::now().date_naive();
        let target = match unit {
            'D' => today
                .checked_add_signed(chrono::Duration::days(n))
                .ok_or_else(|| format!("date overflow for '{s}'"))?,
            'W' => today
                .checked_add_signed(chrono::Duration::weeks(n))
                .ok_or_else(|| format!("date overflow for '{s}'"))?,
            'M' => {
                if n >= 0 {
                    today
                        .checked_add_months(chrono::Months::new(n as u32))
                        .ok_or_else(|| format!("date overflow for '{s}'"))?
                } else {
                    today
                        .checked_sub_months(chrono::Months::new(n.unsigned_abs() as u32))
                        .ok_or_else(|| format!("date overflow for '{s}'"))?
                }
            }
            'Y' => {
                let months = n
                    .checked_mul(12)
                    .ok_or_else(|| format!("date overflow for '{s}'"))?;
                if months >= 0 {
                    today
                        .checked_add_months(chrono::Months::new(months as u32))
                        .ok_or_else(|| format!("date overflow for '{s}'"))?
                } else {
                    today
                        .checked_sub_months(chrono::Months::new(months.unsigned_abs() as u32))
                        .ok_or_else(|| format!("date overflow for '{s}'"))?
                }
            }
            _ => unreachable!(),
        };
        return Ok(target.format("%Y-%m-%d").to_string());
    }

    // Fall back to absolute YYYY-MM-DD.
    chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d")
        .map(|d| d.format("%Y-%m-%d").to_string())
        .map_err(|_| {
            format!("invalid date '{s}' — expected YYYY-MM-DD or relative (e.g. 0D, 1W, 2M)")
        })
}

fn fail(msg: &str) -> ! {
    eprintln!("error: {msg}");
    process::exit(1);
}

// ---------------------------------------------------------------------------
// Command handlers
// ---------------------------------------------------------------------------

fn cmd_add(
    conn: &rusqlite::Connection,
    content: String,
    priority: u8,
    due: String,
    notes: Option<String>,
) {
    if let Err(e) = Priority::new(priority) {
        fail(&e);
    }
    let resolved = match resolve_date(&due) {
        Ok(d) => d,
        Err(e) => fail(&e),
    };

    match db::add(conn, &content, priority, Some(&resolved), notes.as_deref()) {
        Ok(id) => println!("Added to-do #{id}"),
        Err(e) => fail(&format!("failed to add to-do: {e}")),
    }
}

fn cmd_list(
    conn: &rusqlite::Connection,
    all: bool,
    done: bool,
    priority: Option<u8>,
    due: Option<String>,
    past: Option<String>,
    include_nodate: bool,
    count: bool,
    simple: bool,
    full_date: bool,
    priority_text: bool,
) {
    if let Some(p) = priority {
        if let Err(e) = Priority::new(p) {
            fail(&e);
        }
    }

    let show_pending = !done;
    let show_done = done || all;

    let due_before: Option<String> = if let Some(ref due_str) = due {
        if let Some((n, unit)) = parse_relative(due_str) {
            let today = chrono::Local::now().date_naive();
            let target = match unit {
                'D' => today
                    .checked_add_signed(chrono::Duration::days(n))
                    .unwrap_or_else(|| fail(&format!("date overflow for '{due_str}'"))),
                'W' => today
                    .checked_add_signed(chrono::Duration::weeks(n))
                    .unwrap_or_else(|| fail(&format!("date overflow for '{due_str}'"))),
                'M' => {
                    if n >= 0 {
                        today
                            .checked_add_months(chrono::Months::new(n as u32))
                            .unwrap_or_else(|| fail(&format!("date overflow for '{due_str}'")))
                    } else {
                        today
                            .checked_sub_months(chrono::Months::new(n.unsigned_abs() as u32))
                            .unwrap_or_else(|| fail(&format!("date overflow for '{due_str}'")))
                    }
                }
                'Y' => {
                    let months = n
                        .checked_mul(12)
                        .unwrap_or_else(|| fail(&format!("date overflow for '{due_str}'")));
                    if months >= 0 {
                        today
                            .checked_add_months(chrono::Months::new(months as u32))
                            .unwrap_or_else(|| fail(&format!("date overflow for '{due_str}'")))
                    } else {
                        today
                            .checked_sub_months(chrono::Months::new(months.unsigned_abs() as u32))
                            .unwrap_or_else(|| fail(&format!("date overflow for '{due_str}'")))
                    }
                }
                _ => fail(&format!("invalid unit '{unit}' in '{due_str}'")),
            };
            Some(target.format("%Y-%m-%d").to_string())
        } else {
            fail(&format!(
                "invalid due filter '{due_str}' — expected relative format (e.g., 1d, 3w, 4m)"
            ))
        }
    } else {
        None
    };

    let done_since: Option<String> = if let Some(ref past_str) = past {
        if let Some((n, unit)) = parse_relative(past_str) {
            let today = chrono::Local::now().date_naive();
            let target = match unit {
                'D' => today
                    .checked_sub_signed(chrono::Duration::days(n))
                    .unwrap_or_else(|| fail(&format!("date overflow for '{past_str}'"))),
                'W' => today
                    .checked_sub_signed(chrono::Duration::weeks(n))
                    .unwrap_or_else(|| fail(&format!("date overflow for '{past_str}'"))),
                'M' => {
                    if n >= 0 {
                        today
                            .checked_sub_months(chrono::Months::new(n as u32))
                            .unwrap_or_else(|| fail(&format!("date overflow for '{past_str}'")))
                    } else {
                        today
                            .checked_add_months(chrono::Months::new(n.unsigned_abs() as u32))
                            .unwrap_or_else(|| fail(&format!("date overflow for '{past_str}'")))
                    }
                }
                'Y' => {
                    let months = n
                        .checked_mul(12)
                        .unwrap_or_else(|| fail(&format!("date overflow for '{past_str}'")));
                    if months >= 0 {
                        today
                            .checked_sub_months(chrono::Months::new(months as u32))
                            .unwrap_or_else(|| fail(&format!("date overflow for '{past_str}'")))
                    } else {
                        today
                            .checked_add_months(chrono::Months::new(months.unsigned_abs() as u32))
                            .unwrap_or_else(|| fail(&format!("date overflow for '{past_str}'")))
                    }
                }
                _ => fail(&format!("invalid unit '{unit}' in '{past_str}'")),
            };
            Some(target.format("%Y-%m-%d").to_string())
        } else {
            fail(&format!(
                "invalid past filter '{past_str}' — expected relative format (e.g., 30d, 2w, 1m)"
            ))
        }
    } else {
        None
    };

    match db::list(
        conn,
        show_pending,
        show_done,
        priority,
        due_before.as_deref(),
        done_since.as_deref(),
        include_nodate,
    ) {
        Ok(todos) => {
            if count {
                println!("{}", todos.len());
                return;
            }
            if todos.is_empty() {
                println!("No to-do items found.");
                return;
            }
            if simple {
                for t in &todos {
                    let status = if t.done { "[x]" } else { "[ ]" };
                    let due = if full_date {
                        if let Some(date_str) = t.due_date.as_deref() {
                            if let Ok(date) =
                                chrono::NaiveDate::parse_from_str(date_str, "%Y-%m-%d")
                            {
                                date.format("%A, %B %d, %Y").to_string()
                            } else {
                                date_str.to_string()
                            }
                        } else {
                            "-".to_string()
                        }
                    } else {
                        t.due_date.as_deref().unwrap_or("-").to_string()
                    };
                    let content = if priority_text {
                        format!("{} ({})", t.content, t.priority.label())
                    } else {
                        t.content.clone()
                    };
                    println!("{status} - {due} - {} - {}", t.id, content);
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
        Err(e) => fail(&format!("failed to list to-dos: {e}")),
    }
}

fn cmd_show_single(conn: &rusqlite::Connection, id: i64) -> Result<(), String> {
    match db::get(conn, id) {
        Ok(Some(t)) => {
            println!("ID:        {}", t.id);
            println!("Status:    {}", if t.done { "Done" } else { "Pending" });
            println!("Priority:  {}", t.priority.label());
            println!("Content:   {}", t.content);
            println!("Due:       {}", t.due_date.as_deref().unwrap_or("—"));
            println!("Notes:     {}", t.notes.as_deref().unwrap_or("—"));
            println!("Created:   {}", t.created_at);
            println!("Updated:   {}", t.updated_at);
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

fn cmd_edit(
    conn: &rusqlite::Connection,
    id: i64,
    content: Option<String>,
    priority: Option<u8>,
    due: Option<String>,
    notes: Option<String>,
) {
    // Verify it exists first.
    match db::get(conn, id) {
        Ok(None) => fail(&format!("to-do #{id} not found")),
        Err(e) => fail(&format!("failed to fetch to-do: {e}")),
        Ok(Some(_)) => {}
    }

    let mut updated = false;

    if let Some(ref c) = content {
        db::update_content(conn, id, c)
            .unwrap_or_else(|e| fail(&format!("failed to update content: {e}")));
        updated = true;
    }
    if let Some(p) = priority {
        if let Err(e) = Priority::new(p) {
            fail(&e);
        }
        db::update_priority(conn, id, p)
            .unwrap_or_else(|e| fail(&format!("failed to update priority: {e}")));
        updated = true;
    }
    if let Some(ref d) = due {
        let due_val = if d.eq_ignore_ascii_case("none") {
            None
        } else {
            let resolved = match resolve_date(d) {
                Ok(r) => r,
                Err(e) => fail(&e),
            };
            Some(resolved)
        };
        db::update_due_date(conn, id, due_val.as_deref())
            .unwrap_or_else(|e| fail(&format!("failed to update due date: {e}")));
        updated = true;
    }
    if let Some(ref n) = notes {
        let notes_val = if n.eq_ignore_ascii_case("none") {
            None
        } else {
            Some(n.as_str())
        };
        db::update_notes(conn, id, notes_val)
            .unwrap_or_else(|e| fail(&format!("failed to update notes: {e}")));
        updated = true;
    }

    if updated {
        println!("Updated to-do #{id}");
    } else {
        eprintln!("Nothing to update. Specify at least one field to change.");
        eprintln!("Run `fazerei edit --help` for usage.");
        process::exit(1);
    }
}

fn cmd_done_single(conn: &rusqlite::Connection, id: i64) -> Result<(), String> {
    match db::get(conn, id) {
        Ok(None) => return Err(format!("to-do #{id} not found")),
        Err(e) => return Err(format!("failed to fetch to-do: {e}")),
        Ok(Some(_)) => {}
    }
    db::set_done(conn, id, true).map_err(|e| format!("failed to mark to-do as done: {e}"))?;
    Ok(())
}

fn cmd_done_multi(conn: &rusqlite::Connection, ids: Vec<i64>) {
    let mut errors = Vec::new();
    for id in ids {
        match cmd_done_single(conn, id) {
            Ok(()) => println!("Marked to-do #{id} as done"),
            Err(e) => errors.push((id, e)),
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

fn cmd_undone_single(conn: &rusqlite::Connection, id: i64) -> Result<(), String> {
    match db::get(conn, id) {
        Ok(None) => return Err(format!("to-do #{id} not found")),
        Err(e) => return Err(format!("failed to fetch to-do: {e}")),
        Ok(Some(_)) => {}
    }
    db::set_done(conn, id, false).map_err(|e| format!("failed to revert to-do: {e}"))?;
    Ok(())
}

fn cmd_undone_multi(conn: &rusqlite::Connection, ids: Vec<i64>) {
    let mut errors = Vec::new();
    for id in ids {
        match cmd_undone_single(conn, id) {
            Ok(()) => println!("Marked to-do #{id} as pending"),
            Err(e) => errors.push((id, e)),
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

fn cmd_rm_single(conn: &rusqlite::Connection, id: i64) -> Result<(), String> {
    match db::get(conn, id) {
        Ok(None) => return Err(format!("to-do #{id} not found")),
        Err(e) => return Err(format!("failed to fetch to-do: {e}")),
        Ok(Some(_)) => {}
    }
    db::delete(conn, id).map_err(|e| format!("failed to delete to-do: {e}"))?;
    Ok(())
}

fn cmd_rm_multi(conn: &rusqlite::Connection, ids: Vec<i64>) {
    let mut errors = Vec::new();
    for id in ids {
        match cmd_rm_single(conn, id) {
            Ok(()) => println!("Deleted to-do #{id}"),
            Err(e) => errors.push((id, e)),
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

fn main() {
    CompleteEnv::with_factory(Cli::command).complete();
    let cli = Cli::parse();
    let db_path = resolve_db_path(cli.db);

    let conn = db::open(&db_path).unwrap_or_else(|e| {
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
        } => {
            cmd_add(&conn, content, priority, due, notes);
        }
        Commands::List {
            all,
            done,
            priority,
            due,
            past,
            include_nodate,
            count,
            simple,
            full_date,
            priority_text,
        } => {
            cmd_list(
                &conn,
                all,
                done,
                priority,
                due,
                past,
                include_nodate,
                count,
                simple,
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
        } => {
            let id = id.unwrap_or_else(|| fail("missing required argument: id"));
            cmd_edit(&conn, id, content, priority, due, notes);
        }
        Commands::Done { ids } => {
            cmd_done_multi(&conn, ids);
        }
        Commands::Undone { ids } => {
            cmd_undone_multi(&conn, ids);
        }
        Commands::Rm { ids } => {
            cmd_rm_multi(&conn, ids);
        }
        Commands::InstallCompletion { shell, output } => {
            cmd_install_completion(shell, output);
        }
    }
}
