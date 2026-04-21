//! End-to-end tests spawning the compiled binary. Each test gets its own
//! scratch SQLite DB via `FAZEREI_DB`.

use assert_cmd::Command;
use predicates::prelude::*;
use tempfile::TempDir;

fn bin(db: &TempDir) -> Command {
    let mut cmd = Command::cargo_bin("fazerei").unwrap();
    cmd.env("FAZEREI_DB", db.path().join("f.db"));
    cmd
}

fn tmp() -> TempDir {
    tempfile::tempdir().unwrap()
}

#[test]
fn add_then_list_simple_roundtrip() {
    let db = tmp();
    bin(&db).args(["add", "buy milk", "-d", "0D"]).assert().success();
    bin(&db)
        .args(["list", "--simple"])
        .assert()
        .success()
        .stdout(predicate::str::contains("buy milk"));
}

#[test]
fn edit_requires_id() {
    let db = tmp();
    bin(&db)
        .args(["edit"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("required"));
}

#[test]
fn list_mutex_simple_and_parsable_is_rejected() {
    let db = tmp();
    bin(&db)
        .args(["list", "--simple", "--parsable"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("cannot be used with"));
}

#[test]
fn list_mutex_fields_requires_parsable() {
    let db = tmp();
    bin(&db)
        .args(["list", "--fields", "id"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("--parsable"));
}

#[test]
fn list_mutex_shortcut_filters() {
    let db = tmp();
    bin(&db)
        .args(["list", "--overdue", "--today"])
        .assert()
        .failure();
    bin(&db)
        .args(["list", "--overdue", "--done"])
        .assert()
        .failure();
}

#[test]
fn tag_filter_is_or() {
    let db = tmp();
    bin(&db).args(["add", "A", "-t", "work"]).assert().success();
    bin(&db).args(["add", "B", "-t", "home"]).assert().success();
    bin(&db).args(["add", "C", "-t", "other"]).assert().success();

    bin(&db)
        .args(["list", "--tag", "work,home", "--parsable", "--fields", "content"])
        .assert()
        .success()
        .stdout(predicate::str::contains("A").and(predicate::str::contains("B")))
        .stdout(predicate::str::contains("C").not());
}

#[test]
fn search_is_case_insensitive() {
    let db = tmp();
    bin(&db).args(["add", "Review Migration"]).assert().success();
    bin(&db).args(["add", "unrelated"]).assert().success();

    bin(&db)
        .args(["list", "--search", "MIGRATION", "--parsable", "--fields", "content"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Review Migration"))
        .stdout(predicate::str::contains("unrelated").not());
}

#[test]
fn json_output_is_parseable() {
    let db = tmp();
    bin(&db).args(["add", "alpha", "-t", "x"]).assert().success();
    let out = bin(&db)
        .args(["list", "--json"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let s = String::from_utf8(out).unwrap();
    let v: serde_json::Value = serde_json::from_str(s.trim()).expect("valid JSON");
    assert!(v.is_array());
    assert_eq!(v[0]["content"], "alpha");
    assert_eq!(v[0]["tags"], serde_json::json!(["x"]));
}

#[test]
fn reverse_flips_order() {
    let db = tmp();
    bin(&db).args(["add", "A", "-d", "2026-04-22"]).assert().success();
    bin(&db).args(["add", "B", "-d", "2026-04-23"]).assert().success();

    let forward = bin(&db)
        .args(["list", "--parsable", "--fields", "content"])
        .output()
        .unwrap()
        .stdout;
    let reverse = bin(&db)
        .args(["list", "--reverse", "--parsable", "--fields", "content"])
        .output()
        .unwrap()
        .stdout;
    let f = String::from_utf8(forward).unwrap();
    let r = String::from_utf8(reverse).unwrap();
    let f_lines: Vec<&str> = f.lines().collect();
    let mut r_expected: Vec<&str> = f_lines.clone();
    r_expected.reverse();
    let r_lines: Vec<&str> = r.lines().collect();
    assert_eq!(r_lines, r_expected);
}

#[test]
fn bulk_rm_rolls_back_on_missing_id() {
    let db = tmp();
    bin(&db).args(["add", "one"]).assert().success();
    bin(&db).args(["add", "two"]).assert().success();

    bin(&db)
        .args(["rm", "1", "99", "2"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("No changes were committed"));

    // Both should still exist.
    bin(&db)
        .args(["list", "--count"])
        .assert()
        .stdout(predicate::str::starts_with("2"));
}

#[test]
fn export_import_roundtrip_into_fresh_db() {
    let src = tmp();
    bin(&src).args(["add", "alpha", "-t", "x,y", "-r", "1W"]).assert().success();
    bin(&src).args(["add", "beta", "-p", "1"]).assert().success();

    let export_out = bin(&src)
        .args(["export"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    // Write to a file the import command can read.
    let export_file = src.path().join("export.json");
    std::fs::write(&export_file, &export_out).unwrap();

    let dest = tmp();
    bin(&dest)
        .args(["import", export_file.to_str().unwrap()])
        .assert()
        .success()
        .stdout(predicate::str::contains("Imported 2"));

    bin(&dest)
        .args(["list", "--all", "--parsable", "--fields", "content"])
        .assert()
        .success()
        .stdout(predicate::str::contains("alpha"))
        .stdout(predicate::str::contains("beta"));
}

#[test]
fn undo_rm_restores_rows() {
    let db = tmp();
    bin(&db).args(["add", "keep me"]).assert().success();
    bin(&db).args(["add", "also keep"]).assert().success();

    bin(&db).args(["rm", "1"]).assert().success();
    bin(&db)
        .args(["list", "--count"])
        .assert()
        .stdout(predicate::str::starts_with("1"));

    bin(&db)
        .args(["undo"])
        .assert()
        .success()
        .stdout(predicate::str::contains("restored 1"));

    bin(&db)
        .args(["list", "--count"])
        .assert()
        .stdout(predicate::str::starts_with("2"));
}

#[test]
fn undo_with_empty_journal_is_noop() {
    let db = tmp();
    bin(&db)
        .args(["undo"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Nothing to undo"));
}

#[test]
fn recurrence_spawns_clone_on_done() {
    let db = tmp();
    bin(&db)
        .args(["add", "weekly", "-d", "2026-04-20", "-r", "1W"])
        .assert()
        .success();

    bin(&db)
        .args(["done", "1"])
        .assert()
        .success()
        .stdout(predicate::str::contains("spawned"));

    // The clone's due should be 2026-04-27.
    bin(&db)
        .args(["list", "--parsable", "--fields", "due,content"])
        .assert()
        .stdout(predicate::str::contains("2026-04-27"));
}

#[test]
fn recurrence_validation_rejects_non_positive() {
    let db = tmp();
    bin(&db)
        .args(["add", "x", "-r", "0D"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("positive"));
    bin(&db)
        .args(["add", "x", "-r", "bogus"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("invalid recurrence"));
}

#[test]
fn stdin_bulk_add() {
    let db = tmp();
    bin(&db)
        .args(["add", "--stdin", "-t", "batch"])
        .write_stdin("line one\nline two\nline three\n")
        .assert()
        .success();
    bin(&db)
        .args(["list", "--tag", "batch", "--count"])
        .assert()
        .stdout(predicate::str::starts_with("3"));
}

#[test]
fn stats_output_contains_totals() {
    let db = tmp();
    bin(&db).args(["add", "a"]).assert().success();
    bin(&db).args(["add", "b"]).assert().success();
    bin(&db).args(["done", "1"]).assert().success();

    bin(&db)
        .args(["stats"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Total:"))
        .stdout(predicate::str::contains("(1 pending, 1 done)"));
}

#[test]
fn list_simple_has_no_ansi_escape_bytes() {
    let db = tmp();
    bin(&db).args(["add", "overdue", "-d", "-1D"]).assert().success();
    let out = bin(&db)
        .args(["list", "--simple"])
        .output()
        .unwrap()
        .stdout;
    assert!(!out.contains(&0x1b), "simple output should not emit ANSI escapes");
}
