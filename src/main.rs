mod db;
mod models;

use std::path::PathBuf;
use std::process;

use clap::{Parser, Subcommand};
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
    #[arg(long, global = true, env = "FAZEREI_DB")]
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
        #[arg(short, long)]
        priority: Option<u8>,

        /// Show items due within this timeframe (e.g., 0d, 1d, 3w, 4m)
        #[arg(short = 'd', long = "due", allow_hyphen_values = true)]
        due: Option<String>,

        /// Output only the count of matching items
        #[arg(short = 'c', long = "count")]
        count: bool,

        /// Simple output format for piping: "[x] - YYYY-MM-DD - ID - Content"
        #[arg(short, long)]
        simple: bool,
    },

    /// Show full details of a to-do item
    Show {
        /// Database ID of the to-do item. Run `fazerei list` to see IDs.
        id: i64,
    },

    /// Edit an existing to-do item (only specified fields are updated)
    Edit {
        /// Database ID of the to-do item. Run `fazerei list` to see IDs.
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
    },

    /// Mark a to-do item as done
    Done {
        /// Database ID of the to-do item. Run `fazerei list` to see IDs.
        id: i64,
    },

    /// Revert a to-do item to pending
    Undone {
        /// Database ID of the to-do item. Run `fazerei list` to see IDs.
        id: i64,
    },

    /// Delete a to-do item permanently
    Rm {
        /// Database ID of the to-do item. Run `fazerei list` to see IDs.
        id: i64,
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
    count: bool,
    simple: bool,
) {
    if let Some(p) = priority {
        if let Err(e) = Priority::new(p) {
            fail(&e);
        }
    }

    let show_done = if all {
        None
    } else if done {
        Some(true)
    } else {
        Some(false)
    };

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

    match db::list(conn, show_done, priority, due_before.as_deref()) {
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
                    let due = t.due_date.as_deref().unwrap_or("-");
                    println!("{status} - {due} - {} - {}", t.id, t.content);
                }
                return;
            }
            let rows: Vec<TodoRow> = todos.iter().map(TodoRow::new).collect();
            let table = Table::new(rows).with(Style::rounded()).to_string();
            println!("{table}");
        }
        Err(e) => fail(&format!("failed to list to-dos: {e}")),
    }
}

fn cmd_show(conn: &rusqlite::Connection, id: i64) {
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
        }
        Ok(None) => fail(&format!("to-do #{id} not found")),
        Err(e) => fail(&format!("failed to fetch to-do: {e}")),
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

fn cmd_done(conn: &rusqlite::Connection, id: i64) {
    match db::get(conn, id) {
        Ok(None) => fail(&format!("to-do #{id} not found")),
        Err(e) => fail(&format!("failed to fetch to-do: {e}")),
        Ok(Some(_)) => {}
    }
    db::set_done(conn, id, true)
        .unwrap_or_else(|e| fail(&format!("failed to mark to-do as done: {e}")));
    println!("Marked to-do #{id} as done");
}

fn cmd_undone(conn: &rusqlite::Connection, id: i64) {
    match db::get(conn, id) {
        Ok(None) => fail(&format!("to-do #{id} not found")),
        Err(e) => fail(&format!("failed to fetch to-do: {e}")),
        Ok(Some(_)) => {}
    }
    db::set_done(conn, id, false).unwrap_or_else(|e| fail(&format!("failed to revert to-do: {e}")));
    println!("Marked to-do #{id} as pending");
}

fn cmd_rm(conn: &rusqlite::Connection, id: i64) {
    match db::get(conn, id) {
        Ok(None) => fail(&format!("to-do #{id} not found")),
        Err(e) => fail(&format!("failed to fetch to-do: {e}")),
        Ok(Some(_)) => {}
    }
    db::delete(conn, id).unwrap_or_else(|e| fail(&format!("failed to delete to-do: {e}")));
    println!("Deleted to-do #{id}");
}

// ---------------------------------------------------------------------------
// Main
// ---------------------------------------------------------------------------

fn main() {
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
            count,
            simple,
        } => {
            cmd_list(&conn, all, done, priority, due, count, simple);
        }
        Commands::Show { id } => {
            cmd_show(&conn, id);
        }
        Commands::Edit {
            id,
            content,
            priority,
            due,
            notes,
        } => {
            cmd_edit(&conn, id, content, priority, due, notes);
        }
        Commands::Done { id } => {
            cmd_done(&conn, id);
        }
        Commands::Undone { id } => {
            cmd_undone(&conn, id);
        }
        Commands::Rm { id } => {
            cmd_rm(&conn, id);
        }
    }
}
