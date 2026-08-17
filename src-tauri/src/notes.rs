//! The daily note.
//!
//! One note per day, created the first time you look at that day. The only
//! rule with any real weight is carry-forward: unfinished work follows you
//! into tomorrow, finished work stays behind in the day you did it. Without
//! that, a daily note becomes a graveyard of abandoned lists within a fortnight.
//!
//! Notes live in one of two places. By default, the app's own database — no
//! setup, works immediately. Point the app at a folder and that folder becomes
//! the source of truth instead: one `YYYY-MM-DD.md` file per day, readable in
//! any editor, picked up by whatever backup or sync you already run, and
//! openable in Obsidian without a plugin.

use std::path::{Path, PathBuf};

use rusqlite::Connection;

use crate::db::{self, Note};
use crate::error::{Error, Result};

pub const CARRIED_HEADING: &str = "Carried over";
pub const NOTES_DIR_KEY: &str = "notes_dir";

/// Today in *your* timezone, not UTC — the day has to roll over at your
/// midnight, not at some server's.
pub fn today() -> String {
    chrono::Local::now().format("%Y-%m-%d").to_string()
}

fn is_unfinished_todo(line: &str) -> bool {
    let t = line.trim_start();
    t.starts_with("- [ ]") || t.starts_with("* [ ]")
}

fn is_finished_todo(line: &str) -> bool {
    let t = line.trim_start().to_ascii_lowercase();
    t.starts_with("- [x]") || t.starts_with("* [x]")
}

/// Everything still open from the previous day, kept verbatim so indentation
/// and any inline links survive the move.
fn carry_over(previous_body: &str) -> String {
    let unfinished: Vec<&str> = previous_body
        .lines()
        .filter(|l| is_unfinished_todo(l))
        .collect();

    if unfinished.is_empty() {
        return String::new();
    }
    format!("{CARRIED_HEADING}\n{}\n\n", unfinished.join("\n"))
}

/// Counts for the footer and, later, the morning digest.
pub fn tally(body: &str) -> (usize, usize) {
    let done = body.lines().filter(|l| is_finished_todo(l)).count();
    let open = body.lines().filter(|l| is_unfinished_todo(l)).count();
    (done, open)
}

// ---------------------------------------------------------------------------
// Where notes live
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub enum Store {
    /// The app's own database. The default: nothing to configure.
    Db,
    /// A folder of `YYYY-MM-DD.md` files, which is then the source of truth.
    Folder(PathBuf),
}

pub fn store_for(conn: &Connection) -> Result<Store> {
    match db::setting_get(conn, NOTES_DIR_KEY)? {
        Some(dir) if !dir.trim().is_empty() => Ok(Store::Folder(PathBuf::from(dir))),
        _ => Ok(Store::Db),
    }
}

fn day_file(dir: &Path, day: &str) -> PathBuf {
    dir.join(format!("{day}.md"))
}

fn looks_like_a_day(stem: &str) -> bool {
    let b = stem.as_bytes();
    b.len() == 10
        && b[4] == b'-'
        && b[7] == b'-'
        && b.iter()
            .enumerate()
            .all(|(i, c)| i == 4 || i == 7 || c.is_ascii_digit())
}

/// Every day present, oldest first. Files that aren't dated notes are ignored,
/// so the folder can hold anything else you keep there.
fn days_in(dir: &Path) -> Result<Vec<String>> {
    let mut days = Vec::new();
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        // A folder that has gone away — unplugged drive, moved directory —
        // should read as empty rather than take the app down.
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(days),
        Err(e) => return Err(e.into()),
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("md") {
            continue;
        }
        if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
            if looks_like_a_day(stem) {
                days.push(stem.to_string());
            }
        }
    }
    days.sort();
    Ok(days)
}

pub fn get(conn: &Connection, store: &Store, day: &str) -> Result<Option<Note>> {
    match store {
        Store::Db => db::note_get(conn, day),
        Store::Folder(dir) => {
            let path = day_file(dir, day);
            match std::fs::read_to_string(&path) {
                Ok(body) => Ok(Some(Note {
                    day: day.to_string(),
                    body,
                    updated_at: modified_at(&path),
                })),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
                Err(e) => Err(e.into()),
            }
        }
    }
}

pub fn save(conn: &Connection, store: &Store, day: &str, body: &str) -> Result<Note> {
    match store {
        Store::Db => db::note_save(conn, day, body),
        Store::Folder(dir) => {
            std::fs::create_dir_all(dir)?;
            let path = day_file(dir, day);
            std::fs::write(&path, body)?;
            Ok(Note {
                day: day.to_string(),
                body: body.to_string(),
                updated_at: modified_at(&path),
            })
        }
    }
}

/// The most recent day written before `day` — where carried-over work comes
/// from. Not simply "yesterday": a Monday should pick up Friday's loose ends.
pub fn previous(conn: &Connection, store: &Store, day: &str) -> Result<Option<Note>> {
    match store {
        Store::Db => db::note_previous(conn, day),
        Store::Folder(dir) => {
            let found = days_in(dir)?
                .into_iter()
                .filter(|d| d.as_str() < day)
                .next_back();
            match found {
                Some(d) => get(conn, store, &d),
                None => Ok(None),
            }
        }
    }
}

pub fn neighbours(
    conn: &Connection,
    store: &Store,
    day: &str,
) -> Result<(Option<String>, Option<String>)> {
    match store {
        Store::Db => db::note_neighbours(conn, day),
        Store::Folder(dir) => {
            let days = days_in(dir)?;
            let prev = days.iter().filter(|d| d.as_str() < day).next_back().cloned();
            let next = days.iter().find(|d| d.as_str() > day).cloned();
            Ok((prev, next))
        }
    }
}

/// Opens a day, creating it if it has never been written.
///
/// Only a *fresh* day inherits: opening an old empty day in the history should
/// show what that day actually looked like, not today's leftovers.
pub fn open_day(conn: &Connection, store: &Store, day: &str) -> Result<Note> {
    if let Some(existing) = get(conn, store, day)? {
        return Ok(existing);
    }

    let seed = if day == today() {
        previous(conn, store, day)?
            .map(|prev| carry_over(&prev.body))
            .unwrap_or_default()
    } else {
        String::new()
    };

    save(conn, store, day, &seed)
}

fn modified_at(path: &Path) -> i64 {
    std::fs::metadata(path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs() as i64)
        .unwrap_or_else(db::now_secs)
}

/// Points notes at a folder, copying across everything written so far.
///
/// Existing files are never overwritten — if the folder already holds notes
/// (a previous install, a folder synced from another machine), those win. The
/// database copies are left in place as a fallback rather than deleted.
pub fn adopt_folder(conn: &Connection, dir: &Path) -> Result<usize> {
    std::fs::create_dir_all(dir)?;

    // Prove we can actually write there before committing to it — a read-only
    // folder must fail now, loudly, not silently at the first autosave.
    let probe = dir.join(".workbench-write-test");
    std::fs::write(&probe, b"")
        .map_err(|e| Error::msg(format!("cannot write to that folder — {e}")))?;
    let _ = std::fs::remove_file(&probe);

    let mut copied = 0;
    for note in db::note_all(conn)? {
        let path = day_file(dir, &note.day);
        if path.exists() {
            continue;
        }
        std::fs::write(&path, &note.body)?;
        copied += 1;
    }

    db::setting_set(conn, NOTES_DIR_KEY, &dir.to_string_lossy())?;
    Ok(copied)
}

/// Back to the app's own storage. The folder is left exactly as it is.
pub fn release_folder(conn: &Connection) -> Result<()> {
    db::setting_delete(conn, NOTES_DIR_KEY)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn carries_only_unfinished_work() {
        let yesterday = "\
- [x] ship the release
- [ ] review acme/api#412
  - [ ] and its follow-up
- [X] done, capitalised
notes that are not todos";

        let carried = carry_over(yesterday);
        assert!(carried.contains("review acme/api#412"));
        assert!(carried.contains("  - [ ] and its follow-up"), "indentation kept");
        assert!(!carried.contains("ship the release"));
        assert!(!carried.contains("done, capitalised"));
        assert!(!carried.contains("notes that are not todos"));
    }

    #[test]
    fn a_finished_day_carries_nothing() {
        assert_eq!(carry_over("- [x] all done\njust notes"), "");
    }

    #[test]
    fn tally_counts_both_states() {
        assert_eq!(tally("- [x] a\n- [ ] b\n- [ ] c\ntext"), (1, 2));
    }

    /// The question this answers: when today ends, is today still there?
    #[test]
    fn a_past_day_survives_the_rollover_untouched() {
        let conn = db::open_memory().unwrap();
        let store = Store::Db;
        let earlier = "- [x] shipped the release\n- [ ] chase the flaky test";
        save(&conn, &store, "2000-01-01", earlier).unwrap();

        let fresh = open_day(&conn, &store, &today()).unwrap();
        assert!(fresh.body.contains("chase the flaky test"), "open work follows you");
        assert!(!fresh.body.contains("shipped the release"), "finished work stays put");

        let kept = get(&conn, &store, "2000-01-01").unwrap().unwrap();
        assert_eq!(kept.body, earlier);

        let (previous_day, _) = neighbours(&conn, &store, &today()).unwrap();
        assert_eq!(previous_day.as_deref(), Some("2000-01-01"));
    }

    #[test]
    fn an_old_day_reopened_does_not_inherit() {
        let conn = db::open_memory().unwrap();
        let store = Store::Db;
        save(&conn, &store, "2000-01-01", "- [ ] ancient business").unwrap();
        assert_eq!(open_day(&conn, &store, "2000-01-02").unwrap().body, "");
    }

    /// A folder must behave exactly like the database, or switching storage
    /// would quietly change how the app works.
    #[test]
    fn a_folder_behaves_the_same_as_the_database() {
        let conn = db::open_memory().unwrap();
        let dir = std::env::temp_dir().join(format!("workbench-same-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let store = Store::Folder(dir.clone());

        save(&conn, &store, "2000-01-01", "- [x] done\n- [ ] carried").unwrap();

        // It really is a plain markdown file on disk.
        let raw = std::fs::read_to_string(dir.join("2000-01-01.md")).unwrap();
        assert_eq!(raw, "- [x] done\n- [ ] carried");

        let fresh = open_day(&conn, &store, &today()).unwrap();
        assert!(fresh.body.contains("carried"));
        assert!(!fresh.body.contains("done"));

        let (previous_day, next) = neighbours(&conn, &store, &today()).unwrap();
        assert_eq!(previous_day.as_deref(), Some("2000-01-01"));
        assert_eq!(next, None);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn unrelated_files_in_the_folder_are_ignored() {
        let dir = std::env::temp_dir().join(format!("workbench-mixed-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("2000-01-01.md"), "a note").unwrap();
        std::fs::write(dir.join("README.md"), "not a note").unwrap();
        std::fs::write(dir.join("shopping-list.md"), "also not").unwrap();
        std::fs::write(dir.join("2000-01-02.txt"), "wrong extension").unwrap();

        assert_eq!(days_in(&dir).unwrap(), vec!["2000-01-01".to_string()]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn adopting_a_folder_copies_notes_without_clobbering_what_is_there() {
        let conn = db::open_memory().unwrap();
        let dir = std::env::temp_dir().join(format!("workbench-adopt-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        db::note_save(&conn, "2000-01-01", "from the database").unwrap();
        db::note_save(&conn, "2000-01-02", "also from the database").unwrap();
        // A file already in the folder — from another machine, say.
        std::fs::write(dir.join("2000-01-02.md"), "already here").unwrap();

        let copied = adopt_folder(&conn, &dir).unwrap();
        assert_eq!(copied, 1, "only the day the folder did not have");
        assert_eq!(
            std::fs::read_to_string(dir.join("2000-01-02.md")).unwrap(),
            "already here",
            "the folder wins over the database"
        );
        assert_eq!(
            std::fs::read_to_string(dir.join("2000-01-01.md")).unwrap(),
            "from the database"
        );

        // And the setting now points there.
        assert!(matches!(store_for(&conn).unwrap(), Store::Folder(_)));

        let _ = std::fs::remove_dir_all(&dir);
    }
}
