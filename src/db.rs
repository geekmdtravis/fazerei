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
    show_done: Option<bool>, // None = all, Some(true) = done only, Some(false) = pending only
    priority_filter: Option<u8>,
    due_before: Option<&str>, // If set, only items with a due_date <= this date (YYYY-MM-DD)
) -> SqlResult<Vec<Todo>> {
    let mut sql = String::from("SELECT * FROM todos WHERE 1=1");
    let mut param_values: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();

    if let Some(done) = show_done {
        sql.push_str(" AND done = ?");
        param_values.push(Box::new(if done { 1i32 } else { 0i32 }));
    }
    if let Some(pri) = priority_filter {
        sql.push_str(" AND priority = ?");
        param_values.push(Box::new(pri));
    }
    if let Some(date) = due_before {
        sql.push_str(" AND due_date IS NOT NULL AND due_date <= ?");
        param_values.push(Box::new(date.to_string()));
    }

    sql.push_str(" ORDER BY done ASC, priority ASC, due_date ASC NULLS LAST, created_at DESC");

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
