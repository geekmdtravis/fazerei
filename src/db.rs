use rusqlite::{params, Connection, Result as SqlResult};
use std::path::Path;

use crate::models::{Priority, Todo};

/// Open (or create) the database and ensure the schema exists.
pub fn open(path: &Path) -> SqlResult<Connection> {
    // Ensure parent directory exists.
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).ok();
    }
    let conn = Connection::open(path)?;
    conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")?;
    migrate(&conn)?;
    Ok(conn)
}

fn migrate(conn: &Connection) -> SqlResult<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS todos (
            id          INTEGER PRIMARY KEY AUTOINCREMENT,
            content     TEXT    NOT NULL,
            notes       TEXT,
            priority    INTEGER NOT NULL DEFAULT 3 CHECK (priority BETWEEN 1 AND 5),
            done        INTEGER NOT NULL DEFAULT 0,
            due_date    TEXT,
            created_at  TEXT    NOT NULL DEFAULT (datetime('now')),
            updated_at  TEXT    NOT NULL DEFAULT (datetime('now'))
        );",
    )?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Queries
// ---------------------------------------------------------------------------

fn row_to_todo(row: &rusqlite::Row) -> SqlResult<Todo> {
    Ok(Todo {
        id: row.get("id")?,
        content: row.get("content")?,
        notes: row.get("notes")?,
        priority: Priority::new(row.get::<_, u8>("priority")?).unwrap(),
        done: row.get::<_, i32>("done")? != 0,
        due_date: row.get("due_date")?,
        created_at: row.get("created_at")?,
        updated_at: row.get("updated_at")?,
    })
}

pub fn add(
    conn: &Connection,
    content: &str,
    priority: u8,
    due_date: Option<&str>,
    notes: Option<&str>,
) -> SqlResult<i64> {
    conn.execute(
        "INSERT INTO todos (content, priority, due_date, notes) VALUES (?1, ?2, ?3, ?4)",
        params![content, priority, due_date, notes],
    )?;
    Ok(conn.last_insert_rowid())
}

pub fn get(conn: &Connection, id: i64) -> SqlResult<Option<Todo>> {
    let mut stmt = conn.prepare("SELECT * FROM todos WHERE id = ?1")?;
    let mut rows = stmt.query_map(params![id], row_to_todo)?;
    match rows.next() {
        Some(Ok(todo)) => Ok(Some(todo)),
        Some(Err(e)) => Err(e),
        None => Ok(None),
    }
}

/// List to-dos with optional filters.
pub fn list(
    conn: &Connection,
    show_pending: bool, // Show pending items
    show_done: bool,    // Show done items
    priority_filter: Option<u8>,
    due_before: Option<&str>, // If set, items with due_date <= this date
    done_since: Option<&str>, // If set, only done items with updated_at >= this date (YYYY-MM-DD)
    include_nodate: bool,     // Include done items with no due date (only when show_done is true)
) -> SqlResult<Vec<Todo>> {
    let mut sql = String::from("SELECT * FROM todos WHERE 1=1");
    let mut param_values: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();

    let today = chrono::Local::now()
        .date_naive()
        .format("%Y-%m-%d")
        .to_string();

    // Build the main status filter
    if show_pending && !show_done {
        sql.push_str(" AND done = 0");
    } else if !show_pending && show_done {
        sql.push_str(" AND done = 1");
    } else if show_pending && show_done {
        // --all: show all pending, and done items constrained by -d if given
        let done_date_cond = if include_nodate {
            "due_date IS NOT NULL"
        } else {
            "1=1"
        };
        if let Some(date) = due_before {
            if let Some(since) = done_since {
                // --all -d X -p Y: pending due <= date AND done updated_at >= since AND due <= date
                sql.push_str(&format!(
                    " AND (done = 0 OR done = 1 AND {} AND date(updated_at) >= '{}' AND due_date IS NOT NULL AND due_date <= '{}')",
                    done_date_cond, since, date
                ));
            } else {
                // With -d only: pending due <= date AND done due >= today AND <= date
                sql.push_str(&format!(
                    " AND (done = 0 OR done = 1 AND {} AND due_date >= '{}' AND due_date <= '{}')",
                    done_date_cond, today, date
                ));
            }
        } else if let Some(since) = done_since {
            // --all -p Y: pending + done updated_at >= since (from past timeframe)
            sql.push_str(&format!(
                " AND (done = 0 OR done = 1 AND {} AND date(updated_at) >= '{}')",
                done_date_cond, since
            ));
        } else {
            // No -d: pending + done from today forward
            sql.push_str(&format!(
                " AND (done = 0 OR done = 1 AND {} AND due_date >= '{}')",
                done_date_cond, today
            ));
        }
    }

    if let Some(pri) = priority_filter {
        sql.push_str(" AND priority = ?");
        param_values.push(Box::new(pri));
    }

    // Apply due_before to pending items only (not done)
    if show_pending && !show_done {
        if let Some(date) = due_before {
            sql.push_str(" AND due_date IS NOT NULL AND due_date <= ?");
            param_values.push(Box::new(date.to_string()));
        }
    }

    // Apply done_since filter for done items (and due constraint if no -p)
    if show_done && !show_pending {
        let done_date_cond = if include_nodate {
            "due_date IS NOT NULL"
        } else {
            "1=1"
        };
        if done_since.is_none() {
            // --done without --past: show done due >= today
            sql.push_str(&format!(
                " AND {} AND due_date >= '{}'",
                done_date_cond, today
            ));
        } else if let Some(date) = done_since {
            sql.push_str(&format!(
                " AND {} AND date(updated_at) >= '{}'",
                done_date_cond, date
            ));
        }
    }

    sql.push_str(" ORDER BY done ASC, due_date ASC NULLS LAST, priority ASC, created_at DESC");

    let mut stmt = conn.prepare(&sql)?;
    let params_ref: Vec<&dyn rusqlite::types::ToSql> =
        param_values.iter().map(|b| b.as_ref()).collect();
    let rows = stmt.query_map(params_ref.as_slice(), row_to_todo)?;
    rows.collect()
}

pub fn update_content(conn: &Connection, id: i64, content: &str) -> SqlResult<usize> {
    conn.execute(
        "UPDATE todos SET content = ?1, updated_at = datetime('now') WHERE id = ?2",
        params![content, id],
    )
}

pub fn update_priority(conn: &Connection, id: i64, priority: u8) -> SqlResult<usize> {
    conn.execute(
        "UPDATE todos SET priority = ?1, updated_at = datetime('now') WHERE id = ?2",
        params![priority, id],
    )
}

pub fn update_due_date(conn: &Connection, id: i64, due_date: Option<&str>) -> SqlResult<usize> {
    conn.execute(
        "UPDATE todos SET due_date = ?1, updated_at = datetime('now') WHERE id = ?2",
        params![due_date, id],
    )
}

pub fn update_notes(conn: &Connection, id: i64, notes: Option<&str>) -> SqlResult<usize> {
    conn.execute(
        "UPDATE todos SET notes = ?1, updated_at = datetime('now') WHERE id = ?2",
        params![notes, id],
    )
}

pub fn set_done(conn: &Connection, id: i64, done: bool) -> SqlResult<usize> {
    conn.execute(
        "UPDATE todos SET done = ?1, updated_at = datetime('now') WHERE id = ?2",
        params![if done { 1i32 } else { 0i32 }, id],
    )
}

pub fn delete(conn: &Connection, id: i64) -> SqlResult<usize> {
    conn.execute("DELETE FROM todos WHERE id = ?1", params![id])
}
