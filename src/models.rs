use std::fmt;

use chrono::NaiveDate;
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
    #[tabled(rename = "Due")]
    pub due_date: String,
    #[tabled(rename = "Created")]
    pub created_at: String,
}

impl TodoRow {
    pub fn new(t: &Todo, full_date: bool) -> Self {
        let due_date = format_due_date(t.due_date.as_deref(), t.done, full_date);
        Self {
            id: t.id,
            status: t.status_icon().to_string(),
            priority: t.priority.to_string(),
            content: truncate(&t.content, 50),
            due_date,
            created_at: t.created_at.clone(),
        }
    }
}

/// Format the due-date cell for the list view.
/// Overdue items (past due and not done) are shown in bold red with an
/// "OVERDUE" suffix.
fn format_due_date(due: Option<&str>, done: bool, full_date: bool) -> String {
    let Some(date_str) = due else {
        return "—".into();
    };

    // Parse the date to check if overdue and optionally reformat
    let Ok(due_date) = NaiveDate::parse_from_str(date_str, "%Y-%m-%d") else {
        return date_str.to_string();
    };

    // Format date according to full_date flag
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
        // Bold red: \x1b[1;31m ... \x1b[0m
        // For overdue items, we need to preserve the formatted date
        format!("\x1b[1;31m{formatted_date} OVERDUE\x1b[0m")
    } else {
        formatted_date
    }
}

/// Truncate a string to `max` characters, appending "…" if truncated.
fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let truncated: String = s.chars().take(max - 1).collect();
        format!("{truncated}…")
    }
}
