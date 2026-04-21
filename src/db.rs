use rusqlite::{params, Connection, Result as SqlResult};
use std::error::Error;
use std::path::Path;

use crate::models::{Priority, Sort, Todo};

/// Open (or create) the database and ensure the schema exists.
pub fn open(path: &Path) -> Result<Connection, Box<dyn Error>> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let conn = Connection::open(path)?;
    conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")?;
    migrate(&conn)?;
    Ok(conn)
}

/// Ordered list of schema migrations. Each entry is a pure function applied
/// to the connection. All steps are idempotent (IF NOT EXISTS / column_exists
/// guards) so re-running on a DB at the current state is safe. `PRAGMA
/// user_version` tracks how many have been applied.
const MIGRATIONS: &[fn(&Connection) -> SqlResult<()>] = &[
    migration_1_initial,
    migration_2_indexes,
    migration_3_tags,
    migration_4_recurrence_and_undo,
];

fn migrate(conn: &Connection) -> SqlResult<()> {
    let current: i32 =
        conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    for (i, step) in MIGRATIONS.iter().enumerate() {
        let target = (i + 1) as i32;
        if target > current {
            step(conn)?;
            conn.execute_batch(&format!("PRAGMA user_version = {target}"))?;
        }
    }
    Ok(())
}

fn migration_1_initial(conn: &Connection) -> SqlResult<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS todos (
            id          INTEGER PRIMARY KEY AUTOINCREMENT,
            content     TEXT    NOT NULL,
            notes       TEXT,
            priority    INTEGER NOT NULL DEFAULT 3 CHECK (priority BETWEEN 1 AND 5),
            done        INTEGER NOT NULL DEFAULT 0,
            due_date    TEXT,
            created_at  TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now')),
            updated_at  TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now'))
        );",
    )
}

fn migration_2_indexes(conn: &Connection) -> SqlResult<()> {
    conn.execute_batch(
        "CREATE INDEX IF NOT EXISTS idx_todos_done_due_pri
            ON todos (done, due_date, priority);
         CREATE INDEX IF NOT EXISTS idx_todos_updated_at
            ON todos (updated_at);",
    )
}

fn migration_3_tags(conn: &Connection) -> SqlResult<()> {
    if !column_exists(conn, "todos", "tags")? {
        conn.execute("ALTER TABLE todos ADD COLUMN tags TEXT", [])?;
    }
    Ok(())
}

fn migration_4_recurrence_and_undo(conn: &Connection) -> SqlResult<()> {
    if !column_exists(conn, "todos", "recurrence")? {
        conn.execute("ALTER TABLE todos ADD COLUMN recurrence TEXT", [])?;
    }
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS undo_last (
            id         INTEGER PRIMARY KEY CHECK(id = 1),
            action     TEXT    NOT NULL,
            payload    TEXT    NOT NULL,
            summary    TEXT    NOT NULL,
            created_at TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now'))
        );",
    )?;
    Ok(())
}

fn column_exists(conn: &Connection, table: &str, column: &str) -> SqlResult<bool> {
    let mut stmt = conn.prepare(&format!("PRAGMA table_info({table})"))?;
    let mut rows = stmt.query([])?;
    while let Some(row) = rows.next()? {
        let name: String = row.get("name")?;
        if name == column {
            return Ok(true);
        }
    }
    Ok(false)
}

// ---------------------------------------------------------------------------
// Queries
// ---------------------------------------------------------------------------

fn row_to_todo(row: &rusqlite::Row) -> SqlResult<Todo> {
    let id: i64 = row.get("id")?;
    let raw_pri: u8 = row.get("priority")?;
    let priority = Priority::new(raw_pri).unwrap_or_else(|_| {
        eprintln!("warn: row #{id} has invalid priority {raw_pri}; using default");
        Priority::new(3).expect("3 is a valid priority")
    });
    Ok(Todo {
        id,
        content: row.get("content")?,
        notes: row.get("notes")?,
        priority,
        done: row.get::<_, i32>("done")? != 0,
        due_date: row.get("due_date")?,
        tags: row.get("tags")?,
        recurrence: row.get("recurrence")?,
        created_at: row.get("created_at")?,
        updated_at: row.get("updated_at")?,
    })
}

#[allow(clippy::too_many_arguments)]
pub fn add(
    conn: &Connection,
    content: &str,
    priority: u8,
    due_date: Option<&str>,
    notes: Option<&str>,
    tags: Option<&str>,
    recurrence: Option<&str>,
) -> SqlResult<i64> {
    conn.execute(
        "INSERT INTO todos (content, priority, due_date, notes, tags, recurrence)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![content, priority, due_date, notes, tags, recurrence],
    )?;
    Ok(conn.last_insert_rowid())
}

/// Insert a row with an explicit id (used by undo and import). If the id
/// already exists the row is replaced.
#[allow(clippy::too_many_arguments)]
pub fn upsert_row(
    conn: &Connection,
    id: i64,
    content: &str,
    priority: u8,
    done: bool,
    due_date: Option<&str>,
    notes: Option<&str>,
    tags: Option<&str>,
    recurrence: Option<&str>,
    created_at: &str,
    updated_at: &str,
) -> SqlResult<usize> {
    conn.execute(
        "INSERT OR REPLACE INTO todos
         (id, content, priority, done, due_date, notes, tags, recurrence, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
        params![
            id,
            content,
            priority,
            if done { 1i32 } else { 0i32 },
            due_date,
            notes,
            tags,
            recurrence,
            created_at,
            updated_at
        ],
    )
}

/// Insert a row without id (used by import). The database assigns a new id.
#[allow(clippy::too_many_arguments)]
pub fn insert_full(
    conn: &Connection,
    content: &str,
    priority: u8,
    done: bool,
    due_date: Option<&str>,
    notes: Option<&str>,
    tags: Option<&str>,
    recurrence: Option<&str>,
    created_at: &str,
    updated_at: &str,
) -> SqlResult<i64> {
    conn.execute(
        "INSERT INTO todos
         (content, priority, done, due_date, notes, tags, recurrence, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        params![
            content,
            priority,
            if done { 1i32 } else { 0i32 },
            due_date,
            notes,
            tags,
            recurrence,
            created_at,
            updated_at
        ],
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

/// List to-dos with optional filters and a sort order.
#[allow(clippy::too_many_arguments)]
pub fn list(
    conn: &Connection,
    show_pending: bool,
    show_done: bool,
    priority_filter: Option<u8>,
    due_before: Option<&str>,
    due_from: Option<&str>,
    done_since: Option<&str>,
    include_nodate: bool,
    tags_any: &[String],
    search: Option<&str>,
    sort: Sort,
) -> SqlResult<Vec<Todo>> {
    let mut sql = String::from("SELECT * FROM todos WHERE 1=1");
    let mut values: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();

    let today = chrono::Local::now()
        .date_naive()
        .format("%Y-%m-%d")
        .to_string();

    let nodate_filter = if include_nodate {
        "1=1"
    } else {
        "due_date IS NOT NULL"
    };

    // Build the main status filter. All date / since values are bound as `?`
    // parameters rather than interpolated into the SQL string.
    if show_pending && !show_done {
        sql.push_str(" AND done = 0");
        if let Some(date) = due_before {
            sql.push_str(" AND due_date IS NOT NULL AND due_date <= ?");
            values.push(Box::new(date.to_string()));
        }
    } else if !show_pending && show_done {
        sql.push_str(" AND done = 1");
        if let Some(since) = done_since {
            sql.push_str(&format!(
                " AND {nodate_filter} AND date(updated_at) >= ?"
            ));
            values.push(Box::new(since.to_string()));
            if let Some(date) = due_before {
                sql.push_str(" AND due_date IS NOT NULL AND due_date <= ?");
                values.push(Box::new(date.to_string()));
            }
        } else {
            sql.push_str(&format!(" AND {nodate_filter} AND due_date >= ?"));
            values.push(Box::new(today.clone()));
            if let Some(date) = due_before {
                sql.push_str(" AND due_date <= ?");
                values.push(Box::new(date.to_string()));
            }
        }
    } else if show_pending && show_done {
        if let Some(date) = due_before {
            if let Some(since) = done_since {
                sql.push_str(&format!(
                    " AND ((done = 0 AND due_date IS NOT NULL AND due_date <= ?) OR (done = 1 AND {nodate_filter} AND date(updated_at) >= ? AND due_date IS NOT NULL AND due_date <= ?))"
                ));
                values.push(Box::new(date.to_string()));
                values.push(Box::new(since.to_string()));
                values.push(Box::new(date.to_string()));
            } else {
                sql.push_str(&format!(
                    " AND ((done = 0 AND due_date IS NOT NULL AND due_date <= ?) OR (done = 1 AND {nodate_filter} AND due_date >= ? AND due_date <= ?))"
                ));
                values.push(Box::new(date.to_string()));
                values.push(Box::new(today.clone()));
                values.push(Box::new(date.to_string()));
            }
        } else if let Some(since) = done_since {
            sql.push_str(&format!(
                " AND (done = 0 OR (done = 1 AND {nodate_filter} AND date(updated_at) >= ?))"
            ));
            values.push(Box::new(since.to_string()));
        } else {
            sql.push_str(&format!(
                " AND (done = 0 OR (done = 1 AND {nodate_filter} AND due_date >= ?))"
            ));
            values.push(Box::new(today.clone()));
        }
    }

    if let Some(pri) = priority_filter {
        sql.push_str(" AND priority = ?");
        values.push(Box::new(pri));
    }

    if let Some(from) = due_from {
        sql.push_str(" AND due_date IS NOT NULL AND due_date >= ?");
        values.push(Box::new(from.to_string()));
    }

    if !tags_any.is_empty() {
        sql.push_str(" AND (");
        for (i, tag) in tags_any.iter().enumerate() {
            if i > 0 {
                sql.push_str(" OR ");
            }
            sql.push_str("tags LIKE ?");
            values.push(Box::new(format!("%,{},%", tag.to_lowercase())));
        }
        sql.push(')');
    }

    if let Some(q) = search {
        sql.push_str(" AND (LOWER(content) LIKE LOWER(?) OR LOWER(COALESCE(notes, '')) LIKE LOWER(?))");
        let pattern = format!("%{q}%");
        values.push(Box::new(pattern.clone()));
        values.push(Box::new(pattern));
    }

    sql.push_str(" ORDER BY ");
    sql.push_str(sort.order_by());

    let mut stmt = conn.prepare(&sql)?;
    let params_ref: Vec<&dyn rusqlite::types::ToSql> =
        values.iter().map(|b| b.as_ref()).collect();
    let rows = stmt.query_map(params_ref.as_slice(), row_to_todo)?;
    rows.collect()
}

pub fn update_content(conn: &Connection, id: i64, content: &str) -> SqlResult<usize> {
    conn.execute(
        "UPDATE todos SET content = ?1, updated_at = strftime('%Y-%m-%dT%H:%M:%SZ','now') WHERE id = ?2",
        params![content, id],
    )
}

pub fn update_priority(conn: &Connection, id: i64, priority: u8) -> SqlResult<usize> {
    conn.execute(
        "UPDATE todos SET priority = ?1, updated_at = strftime('%Y-%m-%dT%H:%M:%SZ','now') WHERE id = ?2",
        params![priority, id],
    )
}

pub fn update_due_date(conn: &Connection, id: i64, due_date: Option<&str>) -> SqlResult<usize> {
    conn.execute(
        "UPDATE todos SET due_date = ?1, updated_at = strftime('%Y-%m-%dT%H:%M:%SZ','now') WHERE id = ?2",
        params![due_date, id],
    )
}

pub fn update_notes(conn: &Connection, id: i64, notes: Option<&str>) -> SqlResult<usize> {
    conn.execute(
        "UPDATE todos SET notes = ?1, updated_at = strftime('%Y-%m-%dT%H:%M:%SZ','now') WHERE id = ?2",
        params![notes, id],
    )
}

pub fn update_tags(conn: &Connection, id: i64, tags: Option<&str>) -> SqlResult<usize> {
    conn.execute(
        "UPDATE todos SET tags = ?1, updated_at = strftime('%Y-%m-%dT%H:%M:%SZ','now') WHERE id = ?2",
        params![tags, id],
    )
}

pub fn update_recurrence(
    conn: &Connection,
    id: i64,
    recurrence: Option<&str>,
) -> SqlResult<usize> {
    conn.execute(
        "UPDATE todos SET recurrence = ?1, updated_at = strftime('%Y-%m-%dT%H:%M:%SZ','now') WHERE id = ?2",
        params![recurrence, id],
    )
}

/// Mark an item done. If it has a recurrence and a due date, also insert a
/// clone with the same fields and due_date shifted by the recurrence. Returns
/// the id of the clone if one was created.
pub fn complete_with_recurrence(
    conn: &Connection,
    id: i64,
    compute_next_due: impl FnOnce(&str, &str) -> Option<String>,
) -> SqlResult<Option<i64>> {
    let todo = match get(conn, id)? {
        Some(t) => t,
        None => {
            return Err(rusqlite::Error::QueryReturnedNoRows);
        }
    };
    set_done(conn, id, true)?;

    if let (Some(rec), Some(due)) = (todo.recurrence.as_deref(), todo.due_date.as_deref()) {
        if let Some(next_due) = compute_next_due(due, rec) {
            let new_id = add(
                conn,
                &todo.content,
                todo.priority.value(),
                Some(&next_due),
                todo.notes.as_deref(),
                todo.tags.as_deref(),
                Some(rec),
            )?;
            return Ok(Some(new_id));
        }
    }
    Ok(None)
}

// ---------------------------------------------------------------------------
// Undo journal (single-entry)
// ---------------------------------------------------------------------------

pub fn write_journal(
    conn: &Connection,
    action: &str,
    payload: &str,
    summary: &str,
) -> SqlResult<usize> {
    conn.execute(
        "INSERT OR REPLACE INTO undo_last (id, action, payload, summary, created_at)
         VALUES (1, ?1, ?2, ?3, strftime('%Y-%m-%dT%H:%M:%SZ','now'))",
        params![action, payload, summary],
    )
}

pub fn read_journal(conn: &Connection) -> SqlResult<Option<(String, String, String)>> {
    let mut stmt = conn.prepare("SELECT action, payload, summary FROM undo_last WHERE id = 1")?;
    let mut rows = stmt.query_map([], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, String>(2)?,
        ))
    })?;
    match rows.next() {
        Some(Ok(v)) => Ok(Some(v)),
        Some(Err(e)) => Err(e),
        None => Ok(None),
    }
}

pub fn clear_journal(conn: &Connection) -> SqlResult<usize> {
    conn.execute("DELETE FROM undo_last WHERE id = 1", [])
}

pub fn set_done(conn: &Connection, id: i64, done: bool) -> SqlResult<usize> {
    conn.execute(
        "UPDATE todos SET done = ?1, updated_at = strftime('%Y-%m-%dT%H:%M:%SZ','now') WHERE id = ?2",
        params![if done { 1i32 } else { 0i32 }, id],
    )
}

pub fn delete(conn: &Connection, id: i64) -> SqlResult<usize> {
    conn.execute("DELETE FROM todos WHERE id = ?1", params![id])
}

/// Count totals by done/pending.
pub fn count_by_status(conn: &Connection) -> SqlResult<(i64, i64)> {
    let pending: i64 =
        conn.query_row("SELECT COUNT(*) FROM todos WHERE done = 0", [], |r| r.get(0))?;
    let done: i64 =
        conn.query_row("SELECT COUNT(*) FROM todos WHERE done = 1", [], |r| r.get(0))?;
    Ok((pending, done))
}

/// Count pending items per priority, returned as a 5-element array indexed by
/// priority-1 (i.e. index 0 = priority 1).
pub fn count_pending_by_priority(conn: &Connection) -> SqlResult<[i64; 5]> {
    let mut out = [0i64; 5];
    let mut stmt = conn
        .prepare("SELECT priority, COUNT(*) FROM todos WHERE done = 0 GROUP BY priority")?;
    let rows = stmt.query_map([], |r| Ok((r.get::<_, u8>(0)?, r.get::<_, i64>(1)?)))?;
    for row in rows {
        let (p, c) = row?;
        if (1..=5).contains(&p) {
            out[(p - 1) as usize] = c;
        }
    }
    Ok(out)
}

/// Count pending rows whose due_date is strictly before `date`.
pub fn count_overdue(conn: &Connection, date: &str) -> SqlResult<i64> {
    conn.query_row(
        "SELECT COUNT(*) FROM todos WHERE done = 0 AND due_date IS NOT NULL AND due_date < ?1",
        params![date],
        |r| r.get(0),
    )
}

/// Count pending rows whose due_date falls in the inclusive range [from, to].
pub fn count_due_range(conn: &Connection, from: &str, to: &str) -> SqlResult<i64> {
    conn.query_row(
        "SELECT COUNT(*) FROM todos
         WHERE done = 0 AND due_date IS NOT NULL
         AND due_date >= ?1 AND due_date <= ?2",
        params![from, to],
        |r| r.get(0),
    )
}

/// Count done rows whose updated_at date is >= `since`.
pub fn count_completed_since(conn: &Connection, since: &str) -> SqlResult<i64> {
    conn.query_row(
        "SELECT COUNT(*) FROM todos WHERE done = 1 AND date(updated_at) >= ?1",
        params![since],
        |r| r.get(0),
    )
}

/// Delete done rows whose updated_at date is strictly before `cutoff`.
/// Returns the number of rows deleted. When `dry_run` is true, runs the query
/// but returns 0 without committing (caller passes a transaction).
pub fn delete_done_older_than(conn: &Connection, cutoff: &str) -> SqlResult<usize> {
    conn.execute(
        "DELETE FROM todos WHERE done = 1 AND date(updated_at) < ?1",
        params![cutoff],
    )
}

/// Count done rows whose updated_at date is strictly before `cutoff`
/// (preview for --dry-run).
pub fn count_done_older_than(conn: &Connection, cutoff: &str) -> SqlResult<i64> {
    conn.query_row(
        "SELECT COUNT(*) FROM todos WHERE done = 1 AND date(updated_at) < ?1",
        params![cutoff],
        |r| r.get(0),
    )
}

/// Full rows that `prune` would delete — used for undo capture.
pub fn list_done_older_than(conn: &Connection, cutoff: &str) -> SqlResult<Vec<Todo>> {
    let mut stmt = conn.prepare(
        "SELECT * FROM todos WHERE done = 1 AND date(updated_at) < ?1",
    )?;
    let rows = stmt.query_map(params![cutoff], row_to_todo)?;
    rows.collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::Sort;

    fn fresh_conn() -> Connection {
        let c = Connection::open_in_memory().unwrap();
        migrate(&c).unwrap();
        c
    }

    fn ids(todos: &[Todo]) -> Vec<i64> {
        todos.iter().map(|t| t.id).collect()
    }

    fn list_default(conn: &Connection) -> Vec<Todo> {
        list(conn, true, false, None, None, None, None, false, &[], None, Sort::Due).unwrap()
    }

    #[test]
    fn migration_is_idempotent() {
        let c = fresh_conn();
        migrate(&c).unwrap();
        migrate(&c).unwrap();
        assert!(column_exists(&c, "todos", "tags").unwrap());
    }

    #[test]
    fn migration_adds_tags_to_preexisting_schema() {
        let c = Connection::open_in_memory().unwrap();
        c.execute_batch(
            "CREATE TABLE todos (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                content TEXT NOT NULL,
                notes TEXT,
                priority INTEGER NOT NULL DEFAULT 3,
                done INTEGER NOT NULL DEFAULT 0,
                due_date TEXT,
                created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now')),
                updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now'))
            );
            INSERT INTO todos(content) VALUES('legacy');",
        )
        .unwrap();
        assert!(!column_exists(&c, "todos", "tags").unwrap());
        migrate(&c).unwrap();
        assert!(column_exists(&c, "todos", "tags").unwrap());

        // The legacy row is still readable and has null tags.
        let rows = list_default(&c);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].content, "legacy");
        assert!(rows[0].tags.is_none());
    }

    #[test]
    fn add_and_get_roundtrip() {
        let c = fresh_conn();
        let id = add(&c, "hello", 2, Some("2026-05-01"), Some("n"), Some(",a,b,"), None).unwrap();
        let t = get(&c, id).unwrap().unwrap();
        assert_eq!(t.content, "hello");
        assert_eq!(t.priority.label(), "2 (high)");
        assert_eq!(t.due_date.as_deref(), Some("2026-05-01"));
        assert_eq!(t.notes.as_deref(), Some("n"));
        assert_eq!(t.tags.as_deref(), Some(",a,b,"));
        assert!(!t.done);
    }

    #[test]
    fn get_missing_returns_none() {
        let c = fresh_conn();
        assert!(get(&c, 9999).unwrap().is_none());
    }

    #[test]
    fn list_pending_only_by_default() {
        let c = fresh_conn();
        let a = add(&c, "pending", 3, Some("2030-01-01"), None, None, None).unwrap();
        let b = add(&c, "done", 3, Some("2030-01-01"), None, None, None).unwrap();
        set_done(&c, b, true).unwrap();
        let rows = list_default(&c);
        assert_eq!(ids(&rows), vec![a]);
    }

    #[test]
    fn list_priority_filter() {
        let c = fresh_conn();
        let p1 = add(&c, "p1", 1, Some("2030-01-01"), None, None, None).unwrap();
        let _p3 = add(&c, "p3", 3, Some("2030-01-01"), None, None, None).unwrap();
        let rows =
            list(&c, true, false, Some(1), None, None, None, false, &[], None, Sort::Due).unwrap();
        assert_eq!(ids(&rows), vec![p1]);
    }

    #[test]
    fn list_due_before_filters_pending() {
        let c = fresh_conn();
        let near = add(&c, "near", 3, Some("2030-01-01"), None, None, None).unwrap();
        let _far = add(&c, "far", 3, Some("2030-12-31"), None, None, None).unwrap();
        let rows = list(
            &c,
            true,
            false,
            None,
            Some("2030-06-01"),
            None,
            None,
            false,
            &[],
            None,
            Sort::Due,
        )
        .unwrap();
        assert_eq!(ids(&rows), vec![near]);
    }

    #[test]
    fn list_due_from_floors_the_range() {
        let c = fresh_conn();
        let _past = add(&c, "past", 3, Some("2020-01-01"), None, None, None).unwrap();
        let future = add(&c, "future", 3, Some("2030-01-01"), None, None, None).unwrap();
        let rows = list(
            &c,
            true,
            false,
            None,
            None,
            Some("2025-01-01"),
            None,
            false,
            &[],
            None,
            Sort::Due,
        )
        .unwrap();
        assert_eq!(ids(&rows), vec![future]);
    }

    #[test]
    fn list_tag_filter_or_semantics() {
        let c = fresh_conn();
        let a = add(&c, "a", 3, Some("2030-01-01"), None, Some(",work,urgent,"), None).unwrap();
        let b = add(&c, "b", 3, Some("2030-01-01"), None, Some(",home,"), None).unwrap();
        let _c = add(&c, "c", 3, Some("2030-01-01"), None, Some(",personal,"), None).unwrap();

        let tags = vec!["work".to_string(), "home".to_string()];
        let rows = list(
            &c,
            true,
            false,
            None,
            None,
            None,
            None,
            false,
            &tags,
            None,
            Sort::Due,
        )
        .unwrap();
        let mut got = ids(&rows);
        got.sort();
        let mut want = vec![a, b];
        want.sort();
        assert_eq!(got, want);
    }

    #[test]
    fn list_search_matches_content_and_notes_case_insensitive() {
        let c = fresh_conn();
        let a = add(&c, "Review Migration", 3, Some("2030-01-01"), None, None, None).unwrap();
        let b = add(&c, "Unrelated", 3, Some("2030-01-01"), Some("has migration in notes"), None, None)
            .unwrap();
        let _c = add(&c, "No match", 3, Some("2030-01-01"), None, None, None).unwrap();

        let rows = list(
            &c,
            true,
            false,
            None,
            None,
            None,
            None,
            false,
            &[],
            Some("MIGRATION"),
            Sort::Due,
        )
        .unwrap();
        let mut got = ids(&rows);
        got.sort();
        let mut want = vec![a, b];
        want.sort();
        assert_eq!(got, want);
    }

    #[test]
    fn list_sort_updated_puts_most_recent_first() {
        let c = fresh_conn();
        let a = add(&c, "a", 3, Some("2030-01-01"), None, None, None).unwrap();
        let b = add(&c, "b", 3, Some("2030-01-01"), None, None, None).unwrap();
        // Manually set updated_at to force ordering without sleeping.
        c.execute(
            "UPDATE todos SET updated_at = '2026-01-01 00:00:00' WHERE id = ?1",
            params![a],
        )
        .unwrap();
        c.execute(
            "UPDATE todos SET updated_at = '2026-06-01 00:00:00' WHERE id = ?1",
            params![b],
        )
        .unwrap();
        let rows = list(
            &c,
            true,
            false,
            None,
            None,
            None,
            None,
            false,
            &[],
            None,
            Sort::Updated,
        )
        .unwrap();
        assert_eq!(ids(&rows), vec![b, a]);
    }

    #[test]
    fn update_tags_persists() {
        let c = fresh_conn();
        let id = add(&c, "x", 3, Some("2030-01-01"), None, None, None).unwrap();
        update_tags(&c, id, Some(",work,")).unwrap();
        assert_eq!(get(&c, id).unwrap().unwrap().tags.as_deref(), Some(",work,"));
        update_tags(&c, id, None).unwrap();
        assert!(get(&c, id).unwrap().unwrap().tags.is_none());
    }

    #[test]
    fn delete_removes_row() {
        let c = fresh_conn();
        let id = add(&c, "x", 3, Some("2030-01-01"), None, None, None).unwrap();
        assert_eq!(delete(&c, id).unwrap(), 1);
        assert!(get(&c, id).unwrap().is_none());
    }

    #[test]
    fn count_by_status_reports_both() {
        let c = fresh_conn();
        let a = add(&c, "a", 3, Some("2030-01-01"), None, None, None).unwrap();
        let _b = add(&c, "b", 3, Some("2030-01-01"), None, None, None).unwrap();
        set_done(&c, a, true).unwrap();
        assert_eq!(count_by_status(&c).unwrap(), (1, 1));
    }

    #[test]
    fn count_pending_by_priority_ignores_done() {
        let c = fresh_conn();
        let a = add(&c, "p1", 1, Some("2030-01-01"), None, None, None).unwrap();
        let _a2 = add(&c, "p1b", 1, Some("2030-01-01"), None, None, None).unwrap();
        let _b = add(&c, "p3", 3, Some("2030-01-01"), None, None, None).unwrap();
        set_done(&c, a, true).unwrap();
        let by = count_pending_by_priority(&c).unwrap();
        assert_eq!(by[0], 1); // priority 1 pending
        assert_eq!(by[2], 1); // priority 3 pending
    }

    #[test]
    fn count_overdue_is_strict_less_than() {
        let c = fresh_conn();
        let _today = add(&c, "t", 3, Some("2026-04-20"), None, None, None).unwrap();
        let _past = add(&c, "p", 3, Some("2026-04-15"), None, None, None).unwrap();
        // Only "p" is strictly before 2026-04-20.
        assert_eq!(count_overdue(&c, "2026-04-20").unwrap(), 1);
    }

    #[test]
    fn count_due_range_inclusive() {
        let c = fresh_conn();
        let _in = add(&c, "in", 3, Some("2026-04-22"), None, None, None).unwrap();
        let _edge_start = add(&c, "es", 3, Some("2026-04-20"), None, None, None).unwrap();
        let _edge_end = add(&c, "ee", 3, Some("2026-04-27"), None, None, None).unwrap();
        let _out = add(&c, "out", 3, Some("2026-04-28"), None, None, None).unwrap();
        assert_eq!(count_due_range(&c, "2026-04-20", "2026-04-27").unwrap(), 3);
    }

    #[test]
    fn prune_respects_cutoff_and_only_done() {
        let c = fresh_conn();
        let old_done = add(&c, "old_done", 3, Some("2026-01-01"), None, None, None).unwrap();
        let fresh_done = add(&c, "fresh_done", 3, Some("2026-04-01"), None, None, None).unwrap();
        let old_pending = add(&c, "old_pending", 3, Some("2026-01-01"), None, None, None).unwrap();

        set_done(&c, old_done, true).unwrap();
        set_done(&c, fresh_done, true).unwrap();
        // Backdate the done items so prune sees them as old.
        c.execute(
            "UPDATE todos SET updated_at = '2020-01-01 00:00:00' WHERE id = ?1",
            params![old_done],
        )
        .unwrap();
        c.execute(
            "UPDATE todos SET updated_at = '2099-12-31 00:00:00' WHERE id = ?1",
            params![fresh_done],
        )
        .unwrap();

        let n = count_done_older_than(&c, "2026-01-01").unwrap();
        assert_eq!(n, 1);
        let removed = delete_done_older_than(&c, "2026-01-01").unwrap();
        assert_eq!(removed, 1);
        assert!(get(&c, old_done).unwrap().is_none());
        assert!(get(&c, fresh_done).unwrap().is_some());
        assert!(get(&c, old_pending).unwrap().is_some()); // pending never pruned
    }

    #[test]
    fn complete_with_recurrence_no_clone_when_none() {
        let c = fresh_conn();
        let id = add(&c, "x", 3, Some("2026-04-20"), None, None, None).unwrap();
        let spawned = complete_with_recurrence(&c, id, |_, _| None).unwrap();
        assert!(spawned.is_none());
        assert!(get(&c, id).unwrap().unwrap().done);
    }

    #[test]
    fn complete_with_recurrence_clones_with_shifted_due() {
        let c = fresh_conn();
        let id = add(
            &c,
            "weekly",
            2,
            Some("2026-04-20"),
            Some("note"),
            Some(",a,b,"),
            Some("1W"),
        )
        .unwrap();
        let spawned = complete_with_recurrence(&c, id, |due, rec| {
            // Emulate main.rs::next_occurrence for "1W"
            assert_eq!(rec, "1W");
            let d = chrono::NaiveDate::parse_from_str(due, "%Y-%m-%d").unwrap();
            Some(
                d.checked_add_signed(chrono::Duration::weeks(1))
                    .unwrap()
                    .format("%Y-%m-%d")
                    .to_string(),
            )
        })
        .unwrap();
        let new_id = spawned.expect("expected a clone");
        let orig = get(&c, id).unwrap().unwrap();
        let clone = get(&c, new_id).unwrap().unwrap();
        assert!(orig.done);
        assert!(!clone.done);
        assert_eq!(clone.content, "weekly");
        assert_eq!(clone.notes.as_deref(), Some("note"));
        assert_eq!(clone.tags.as_deref(), Some(",a,b,"));
        assert_eq!(clone.recurrence.as_deref(), Some("1W"));
        assert_eq!(clone.due_date.as_deref(), Some("2026-04-27"));
    }

    #[test]
    fn journal_roundtrip() {
        let c = fresh_conn();
        assert!(read_journal(&c).unwrap().is_none());
        write_journal(&c, "rm", "{\"rows\":[]}", "rm 0").unwrap();
        let got = read_journal(&c).unwrap().unwrap();
        assert_eq!(got.0, "rm");
        assert_eq!(got.2, "rm 0");

        // Overwrite
        write_journal(&c, "edit", "{\"before\":{}}", "edit #5").unwrap();
        let got = read_journal(&c).unwrap().unwrap();
        assert_eq!(got.0, "edit");

        clear_journal(&c).unwrap();
        assert!(read_journal(&c).unwrap().is_none());
    }

    #[test]
    fn upsert_row_restores_deleted() {
        let c = fresh_conn();
        let id = add(&c, "x", 2, Some("2030-01-01"), Some("n"), Some(",a,"), Some("1D")).unwrap();
        let orig = get(&c, id).unwrap().unwrap();
        delete(&c, id).unwrap();
        assert!(get(&c, id).unwrap().is_none());

        upsert_row(
            &c,
            orig.id,
            &orig.content,
            orig.priority.value(),
            orig.done,
            orig.due_date.as_deref(),
            orig.notes.as_deref(),
            orig.tags.as_deref(),
            orig.recurrence.as_deref(),
            &orig.created_at,
            &orig.updated_at,
        )
        .unwrap();
        let restored = get(&c, id).unwrap().unwrap();
        assert_eq!(restored.content, orig.content);
        assert_eq!(restored.recurrence.as_deref(), Some("1D"));
        assert_eq!(restored.tags.as_deref(), Some(",a,"));
    }

    #[test]
    fn set_done_updates_timestamp() {
        let c = fresh_conn();
        let id = add(&c, "x", 3, Some("2030-01-01"), None, None, None).unwrap();
        // Backdate so we can detect that set_done bumped updated_at.
        c.execute(
            "UPDATE todos SET updated_at = '2000-01-01 00:00:00' WHERE id = ?1",
            params![id],
        )
        .unwrap();
        set_done(&c, id, true).unwrap();
        let after = get(&c, id).unwrap().unwrap();
        assert!(after.done);
        assert_ne!(after.updated_at, "2000-01-01 00:00:00");
    }
}
