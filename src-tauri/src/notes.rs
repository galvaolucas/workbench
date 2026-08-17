//! The daily note.
//!
//! One note per day, created the first time you look at that day. The only
//! rule with any real weight is carry-forward: unfinished work follows you
//! into tomorrow, finished work stays behind in the day you did it. Without
//! that, a daily note becomes a graveyard of abandoned lists within a fortnight.

use rusqlite::Connection;

use crate::db::{self, Note};
use crate::error::Result;

pub const CARRIED_HEADING: &str = "Carried over";

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

/// Opens a day, creating it if it has never been written.
///
/// Only a *fresh* day inherits: opening an old empty day in the history should
/// show you what that day actually looked like, not today's leftovers.
pub fn open_day(conn: &Connection, day: &str) -> Result<Note> {
    if let Some(existing) = db::note_get(conn, day)? {
        return Ok(existing);
    }

    let seed = if day == today() {
        db::note_previous(conn, day)?
            .map(|prev| carry_over(&prev.body))
            .unwrap_or_default()
    } else {
        String::new()
    };

    db::note_save(conn, day, &seed)
}

/// Counts for the end-of-day line and, later, the morning digest.
pub fn tally(body: &str) -> (usize, usize) {
    let done = body.lines().filter(|l| is_finished_todo(l)).count();
    let open = body.lines().filter(|l| is_unfinished_todo(l)).count();
    (done, open)
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

    /// The question this answers: when today ends, is today still there?
    #[test]
    fn a_past_day_survives_the_rollover_untouched() {
        let conn = db::open_memory().unwrap();
        let earlier = "- [x] shipped the release\n- [ ] chase the flaky test";
        db::note_save(&conn, "2000-01-01", earlier).unwrap();

        // Opening a new day seeds it from the last one written...
        let fresh = open_day(&conn, &today()).unwrap();
        assert!(fresh.body.contains("chase the flaky test"), "open work follows you");
        assert!(!fresh.body.contains("shipped the release"), "finished work stays put");

        // ...and leaves that day exactly as it was. Byte for byte.
        let kept = db::note_get(&conn, "2000-01-01").unwrap().unwrap();
        assert_eq!(kept.body, earlier);

        // And it is still reachable by walking back from today.
        let (previous, _) = db::note_neighbours(&conn, &today()).unwrap();
        assert_eq!(previous.as_deref(), Some("2000-01-01"));
    }

    #[test]
    fn an_old_day_reopened_does_not_inherit() {
        let conn = db::open_memory().unwrap();
        db::note_save(&conn, "2000-01-01", "- [ ] ancient business").unwrap();
        // Looking at history should show what that day was, not today's leftovers.
        let older = open_day(&conn, "2000-01-02").unwrap();
        assert_eq!(older.body, "");
    }

    #[test]
    fn tally_counts_both_states() {
        assert_eq!(tally("- [x] a\n- [ ] b\n- [ ] c\ntext"), (1, 2));
    }
}
