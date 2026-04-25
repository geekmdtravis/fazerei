use std::fmt;
use std::io::IsTerminal;

use chrono::NaiveDate;
use clap::ValueEnum;
use tabled::Tabled;

/// Priority levels 1 (highest) through 5 (lowest).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Priority(u8);

impl Priority {
    pub fn new(value: u8) -> Result<Self, String> {
        if (1..=5).contains(&value) {
            Ok(Self(value))
        } else {
            Err(format!("priority must be between 1 and 5, got {value}"))
        }
    }

    pub fn value(self) -> u8 {
        self.0
    }

    pub fn label(self) -> &'static str {
        match self.0 {
            1 => "1 (highest)",
            2 => "2 (high)",
            3 => "3 (medium)",
            4 => "4 (low)",
            5 => "5 (lowest)",
            _ => unreachable!(),
        }
    }
}

impl fmt::Display for Priority {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// A single to-do item.
#[derive(Debug, Clone)]
pub struct Todo {
    pub id: i64,
    pub content: String,
    pub notes: Option<String>,
    pub priority: Priority,
    pub done: bool,
    pub due_date: Option<String>,
    pub tags: Option<String>,
    pub recurrence: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

impl Todo {
    /// One-line status indicator.
    pub fn status_icon(&self) -> &'static str {
        if self.done {
            "[x]"
        } else {
            "[ ]"
        }
    }

    /// Parse the stored `,tag1,tag2,` form into a vec of tag names.
    pub fn tags_vec(&self) -> Vec<String> {
        match &self.tags {
            Some(s) => s
                .split(',')
                .filter(|t| !t.is_empty())
                .map(|t| t.to_string())
                .collect(),
            None => Vec::new(),
        }
    }
}

/// Normalize free-form tag input into the `,tag1,tag2,` storage form.
///
/// Each incoming string may itself be comma-delimited (from clap's
/// value_delimiter). Tags are trimmed, lowercased, de-duplicated, and empty
/// strings dropped. Returns `None` if the result is empty.
pub fn normalize_tags<I, S>(inputs: I) -> Option<String>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let mut seen = Vec::new();
    for raw in inputs {
        for part in raw.as_ref().split(',') {
            let t = part.trim().to_lowercase();
            if !t.is_empty() && !seen.contains(&t) {
                seen.push(t);
            }
        }
    }
    if seen.is_empty() {
        None
    } else {
        Some(format!(",{},", seen.join(",")))
    }
}

/// Sort orders for `fazerei list`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum Sort {
    /// Default: done last, nearest due date first, higher priority first, newer first.
    Due,
    /// Recently changed first.
    Updated,
    /// Higher priority first.
    Priority,
    /// Newest first.
    Created,
}

impl Sort {
    /// SQL ORDER BY clause (without the `ORDER BY` keyword).
    pub fn order_by(self) -> &'static str {
        match self {
            Sort::Due => {
                "done ASC, due_date ASC NULLS LAST, priority ASC, created_at DESC"
            }
            Sort::Updated => "updated_at DESC",
            Sort::Priority => "priority ASC, due_date ASC NULLS LAST, created_at DESC",
            Sort::Created => "created_at DESC",
        }
    }
}

/// Fields available in the `--parsable` output.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum Field {
    Id,
    Status,
    Priority,
    Due,
    Updated,
    Created,
    Tags,
    Recurrence,
    Content,
    Notes,
}

impl Field {
    /// The default field set when `--fields` is not specified.
    pub fn defaults() -> Vec<Field> {
        vec![Field::Id, Field::Status, Field::Due, Field::Content]
    }

    /// Rendered value for a given to-do. `-` for null fields.
    /// Content / notes are returned raw (no quoting) and callers must keep
    /// them at end-of-line to preserve whitespace-splittability.
    fn render(self, t: &Todo) -> String {
        match self {
            Field::Id => t.id.to_string(),
            Field::Status => t.status_icon().to_string(),
            Field::Priority => t.priority.to_string(),
            Field::Due => t.due_date.clone().unwrap_or_else(|| "-".into()),
            Field::Updated => t.updated_at.clone(),
            Field::Created => t.created_at.clone(),
            Field::Tags => {
                let v = t.tags_vec();
                if v.is_empty() {
                    "-".into()
                } else {
                    v.join(",")
                }
            }
            Field::Recurrence => t.recurrence.clone().unwrap_or_else(|| "-".into()),
            Field::Content => t.content.clone(),
            Field::Notes => t.notes.clone().unwrap_or_else(|| "-".into()),
        }
    }

    /// Fields whose value may contain whitespace; these must appear last.
    fn is_free_text(self) -> bool {
        matches!(self, Field::Content | Field::Notes)
    }

    /// Header label used by the pretty-table view when `--fields` selects this column.
    pub fn header(self) -> &'static str {
        match self {
            Field::Id => "#",
            Field::Status => "",
            Field::Priority => "Pri",
            Field::Due => "Due",
            Field::Updated => "Updated",
            Field::Created => "Created",
            Field::Tags => "Tags",
            Field::Recurrence => "Recur",
            Field::Content => "Content",
            Field::Notes => "Notes",
        }
    }

    /// Cell value for the pretty-table view. Mirrors the formatting of the
    /// fixed `TodoRow` columns (overdue colorization, local timestamps,
    /// truncation, em-dash for nulls).
    pub fn render_pretty(self, t: &Todo, full_date: bool, priority_text: bool) -> String {
        match self {
            Field::Id => t.id.to_string(),
            Field::Status => t.status_icon().to_string(),
            Field::Priority => {
                if priority_text {
                    t.priority.label().to_string()
                } else {
                    t.priority.to_string()
                }
            }
            Field::Due => format_due_date(t.due_date.as_deref(), t.done, full_date),
            Field::Updated => format_ts_local(&t.updated_at),
            Field::Created => format_ts_local(&t.created_at),
            Field::Tags => {
                let v = t.tags_vec();
                if v.is_empty() {
                    "—".into()
                } else {
                    truncate(&v.join(","), 30)
                }
            }
            Field::Recurrence => t.recurrence.clone().unwrap_or_else(|| "—".into()),
            Field::Content => truncate(&t.content, 50),
            Field::Notes => match t.notes.as_deref() {
                Some(n) => truncate(n, 50),
                None => "—".into(),
            },
        }
    }
}

/// Render the pretty (default) list view restricted to `fields`. Used when
/// the user passes `--fields` without `--parsable` to fit the table into a
/// narrower window.
pub fn render_pretty_table(
    todos: &[Todo],
    fields: &[Field],
    full_date: bool,
    priority_text: bool,
) -> String {
    use tabled::builder::Builder;
    use tabled::settings::Style;

    let mut b = Builder::default();
    b.push_record(fields.iter().map(|f| f.header().to_string()));
    for t in todos {
        b.push_record(
            fields
                .iter()
                .map(|f| f.render_pretty(t, full_date, priority_text)),
        );
    }
    b.build().with(Style::rounded()).to_string()
}

/// Row type used by `tabled` for the list view.
#[derive(Tabled)]
pub struct TodoRow {
    #[tabled(rename = "#")]
    pub id: i64,
    #[tabled(rename = "")]
    pub status: String,
    #[tabled(rename = "Pri")]
    pub priority: String,
    #[tabled(rename = "Content")]
    pub content: String,
    #[tabled(rename = "Tags")]
    pub tags: String,
    #[tabled(rename = "Due")]
    pub due_date: String,
    #[tabled(rename = "Updated")]
    pub updated_at: String,
    #[tabled(rename = "Created")]
    pub created_at: String,
}

impl TodoRow {
    pub fn new(t: &Todo, full_date: bool, priority_text: bool) -> Self {
        let due_date = format_due_date(t.due_date.as_deref(), t.done, full_date);
        let tags = {
            let v = t.tags_vec();
            if v.is_empty() {
                "—".into()
            } else {
                truncate(&v.join(","), 30)
            }
        };
        Self {
            id: t.id,
            status: t.status_icon().to_string(),
            priority: if priority_text {
                t.priority.label().to_string()
            } else {
                t.priority.to_string()
            },
            content: truncate(&t.content, 50),
            tags,
            due_date,
            updated_at: format_ts_local(&t.updated_at),
            created_at: format_ts_local(&t.created_at),
        }
    }
}

/// Render the bare-bones `--simple` line for a todo.
///
/// Format: `{status} {due|-} {content}`. Invariant — ignores decorator flags.
/// Single space separated. No ANSI.
pub fn render_simple(t: &Todo) -> String {
    let due = t.due_date.as_deref().unwrap_or("-");
    format!("{} {} {}", t.status_icon(), due, t.content)
}

/// Render a `--parsable` line for a todo, given a field order.
///
/// Single space separated. `-` for null fields. Free-text fields (content,
/// notes) are reordered to the end if present so whitespace-splitting on
/// `fields.len() - 1` spaces always yields an intact tail.
pub fn render_parsable(t: &Todo, fields: &[Field]) -> String {
    let mut ordered: Vec<Field> = fields.iter().copied().filter(|f| !f.is_free_text()).collect();
    ordered.extend(fields.iter().copied().filter(|f| f.is_free_text()));

    ordered
        .iter()
        .map(|f| f.render(t))
        .collect::<Vec<_>>()
        .join(" ")
}

/// Format the due-date cell for the list view.
/// Overdue items (past due and not done) are shown in bold red with an
/// "OVERDUE" suffix when stdout is a terminal.
fn format_due_date(due: Option<&str>, done: bool, full_date: bool) -> String {
    let Some(date_str) = due else {
        return "—".into();
    };

    let Ok(due_date) = NaiveDate::parse_from_str(date_str, "%Y-%m-%d") else {
        return date_str.to_string();
    };

    let formatted_date = if full_date {
        due_date.format("%A, %B %d, %Y").to_string()
    } else {
        date_str.to_string()
    };

    if done {
        return formatted_date;
    }

    let today = chrono::Local::now().date_naive();
    if due_date < today {
        if std::io::stdout().is_terminal() {
            format!("\x1b[1;31m{formatted_date} OVERDUE\x1b[0m")
        } else {
            format!("{formatted_date} OVERDUE")
        }
    } else {
        formatted_date
    }
}

/// Render a stored timestamp string as a local-time "YYYY-MM-DD HH:MM:SS".
/// Accepts both the new RFC-3339 UTC form (`2026-04-20T15:30:00Z`) and the
/// legacy naive form (`2026-04-20 15:30:00`, assumed UTC). Returns the
/// original string unchanged if it can't be parsed.
pub fn format_ts_local(stored: &str) -> String {
    use chrono::{DateTime, NaiveDateTime, TimeZone, Utc};

    let utc: DateTime<Utc> = if let Ok(dt) = DateTime::parse_from_rfc3339(stored) {
        dt.with_timezone(&Utc)
    } else if let Ok(naive) = NaiveDateTime::parse_from_str(stored, "%Y-%m-%d %H:%M:%S") {
        Utc.from_utc_datetime(&naive)
    } else {
        return stored.to_string();
    };
    let local: DateTime<chrono::Local> = utc.with_timezone(&chrono::Local);
    local.format("%Y-%m-%d %H:%M:%S").to_string()
}

/// Render a todo as a `serde_json::Value`. Callers that want a string can
/// pass this through `serde_json::to_string`.
pub fn render_json_value(t: &Todo) -> serde_json::Value {
    serde_json::json!({
        "id": t.id,
        "status": if t.done { "done" } else { "pending" },
        "priority": t.priority.value(),
        "content": t.content,
        "notes": t.notes,
        "due": t.due_date,
        "tags": t.tags_vec(),
        "recurrence": t.recurrence,
        "created": t.created_at,
        "updated": t.updated_at,
    })
}

/// Render a todo as a compact JSON object string.
pub fn render_json(t: &Todo) -> String {
    serde_json::to_string(&render_json_value(t))
        .expect("json serialization should never fail for plain data")
}

/// Truncate a string to `max` characters, appending "…" if truncated.
pub fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let truncated: String = s.chars().take(max - 1).collect();
        format!("{truncated}…")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_todo() -> Todo {
        Todo {
            id: 42,
            content: "Review PR".into(),
            notes: Some("look at the tests".into()),
            priority: Priority::new(1).unwrap(),
            done: false,
            due_date: Some("2026-04-22".into()),
            tags: Some(",work,urgent,".into()),
            recurrence: None,
            created_at: "2026-04-20 10:00:00".into(),
            updated_at: "2026-04-20 11:00:00".into(),
        }
    }

    #[test]
    fn priority_rejects_out_of_range() {
        assert!(Priority::new(0).is_err());
        assert!(Priority::new(6).is_err());
        for p in 1..=5 {
            assert!(Priority::new(p).is_ok());
        }
    }

    #[test]
    fn priority_labels_are_stable() {
        assert_eq!(Priority::new(1).unwrap().label(), "1 (highest)");
        assert_eq!(Priority::new(3).unwrap().label(), "3 (medium)");
        assert_eq!(Priority::new(5).unwrap().label(), "5 (lowest)");
    }

    #[test]
    fn truncate_handles_unicode_and_bounds() {
        assert_eq!(truncate("hello", 10), "hello");
        assert_eq!(truncate("hello", 5), "hello");
        assert_eq!(truncate("hello world", 7), "hello …");
        assert_eq!(truncate("héllo", 5), "héllo");
        assert_eq!(truncate("日本語テスト", 4), "日本語…");
    }

    #[test]
    fn normalize_tags_basic() {
        assert_eq!(
            normalize_tags(vec!["work", "urgent"]),
            Some(",work,urgent,".into())
        );
    }

    #[test]
    fn normalize_tags_trims_and_lowercases() {
        assert_eq!(
            normalize_tags(vec![" Work ", "URGENT"]),
            Some(",work,urgent,".into())
        );
    }

    #[test]
    fn normalize_tags_splits_embedded_commas() {
        assert_eq!(
            normalize_tags(vec!["work,urgent", "home"]),
            Some(",work,urgent,home,".into())
        );
    }

    #[test]
    fn normalize_tags_dedupes() {
        assert_eq!(
            normalize_tags(vec!["work", "work", "WORK"]),
            Some(",work,".into())
        );
    }

    #[test]
    fn normalize_tags_empty_is_none() {
        let empty: Vec<&str> = vec![];
        assert_eq!(normalize_tags(empty), None);
        assert_eq!(normalize_tags(vec![""]), None);
        assert_eq!(normalize_tags(vec![" , , "]), None);
    }

    #[test]
    fn tags_vec_parses_storage_form() {
        let t = sample_todo();
        assert_eq!(t.tags_vec(), vec!["work", "urgent"]);
    }

    #[test]
    fn tags_vec_empty_when_none() {
        let mut t = sample_todo();
        t.tags = None;
        assert_eq!(t.tags_vec(), Vec::<String>::new());
    }

    #[test]
    fn render_simple_uses_dash_for_null_due() {
        let mut t = sample_todo();
        t.due_date = None;
        assert_eq!(render_simple(&t), "[ ] - Review PR");
    }

    #[test]
    fn render_simple_shows_done_marker() {
        let mut t = sample_todo();
        t.done = true;
        assert_eq!(render_simple(&t), "[x] 2026-04-22 Review PR");
    }

    #[test]
    fn render_parsable_defaults() {
        let t = sample_todo();
        let s = render_parsable(&t, &Field::defaults());
        assert_eq!(s, "42 [ ] 2026-04-22 Review PR");
    }

    #[test]
    fn render_parsable_moves_content_last() {
        let t = sample_todo();
        // Content requested first — should still end up last.
        let s = render_parsable(&t, &[Field::Content, Field::Id, Field::Status]);
        assert_eq!(s, "42 [ ] Review PR");
    }

    #[test]
    fn render_parsable_tags_field_joins_values() {
        let t = sample_todo();
        let s = render_parsable(&t, &[Field::Id, Field::Tags, Field::Content]);
        assert_eq!(s, "42 work,urgent Review PR");
    }

    #[test]
    fn render_parsable_null_fields_become_dash() {
        let mut t = sample_todo();
        t.due_date = None;
        t.tags = None;
        t.notes = None;
        let s = render_parsable(&t, &[Field::Id, Field::Tags, Field::Due, Field::Notes]);
        // Notes is free-text so it goes last.
        assert_eq!(s, "42 - - -");
    }

    #[test]
    fn render_json_roundtrips() {
        let t = sample_todo();
        let s = render_json(&t);
        let v: serde_json::Value = serde_json::from_str(&s).unwrap();
        assert_eq!(v["id"], 42);
        assert_eq!(v["status"], "pending");
        assert_eq!(v["priority"], 1);
        assert_eq!(v["content"], "Review PR");
        assert_eq!(v["due"], "2026-04-22");
        assert_eq!(v["tags"], serde_json::json!(["work", "urgent"]));
    }

    #[test]
    fn render_json_escapes_quotes_in_content() {
        let mut t = sample_todo();
        t.content = "She said \"hi\"\nand left".into();
        let s = render_json(&t);
        let v: serde_json::Value = serde_json::from_str(&s).unwrap();
        assert_eq!(v["content"], "She said \"hi\"\nand left");
    }

    #[test]
    fn sort_order_by_strings_are_stable() {
        // Just a shape check so refactors don't silently change SQL.
        assert!(Sort::Due.order_by().contains("due_date"));
        assert_eq!(Sort::Updated.order_by(), "updated_at DESC");
        assert!(Sort::Priority.order_by().starts_with("priority ASC"));
        assert_eq!(Sort::Created.order_by(), "created_at DESC");
    }
}
