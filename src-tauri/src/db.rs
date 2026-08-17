use std::path::Path;

use rusqlite::{Connection, OptionalExtension};
use serde::Serialize;

use crate::error::Result;

/// Bump by appending, never by editing an existing entry.
const MIGRATIONS: &[&str] = &[
    // v1 — accounts, settings, and the event log everything else derives from.
    r#"
    CREATE TABLE accounts (
        id           INTEGER PRIMARY KEY,
        provider     TEXT    NOT NULL,
        host         TEXT    NOT NULL,
        login        TEXT    NOT NULL,
        name         TEXT,
        avatar_url   TEXT,
        connected_at INTEGER NOT NULL,
        UNIQUE (host, login)
    );

    CREATE TABLE settings (
        key   TEXT PRIMARY KEY,
        value TEXT NOT NULL
    );

    -- The spine of the app. Every observed state transition is appended here and
    -- never mutated, so notifications fire from persisted facts rather than from
    -- "we happened to see this in a poll". Restart-safe, no double-fires, and
    -- nothing missed while the app was closed. Digests and stats read from it too.
    CREATE TABLE events (
        id           INTEGER PRIMARY KEY,
        account_id   INTEGER NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
        subject_type TEXT    NOT NULL,   -- pull_request | check | thread | app
        subject_id   TEXT    NOT NULL,
        kind         TEXT    NOT NULL,   -- review_requested | checks_failed | ...
        from_state   TEXT,
        to_state     TEXT,
        occurred_at  INTEGER NOT NULL,
        seen_at      INTEGER,
        notified_at  INTEGER
    );

    CREATE INDEX events_pending ON events (notified_at) WHERE notified_at IS NULL;
    CREATE INDEX events_subject ON events (subject_type, subject_id, occurred_at);

    -- Conditional-request bookkeeping: a 304 costs nothing against the rate limit.
    CREATE TABLE sync_meta (
        key            TEXT PRIMARY KEY,
        etag           TEXT,
        last_polled_at INTEGER,
        next_poll_after INTEGER
    );
    "#,
    // v2 — pull requests. Facts only: which lane a PR belongs in is computed at
    // read time from the relation flags, so changing the lane rules never needs
    // a migration or a re-sync.
    r#"
    CREATE TABLE pull_requests (
        id               TEXT    PRIMARY KEY,   -- GraphQL node id, stable across renames
        account_id       INTEGER NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
        repo             TEXT    NOT NULL,      -- owner/name
        number           INTEGER NOT NULL,
        title            TEXT    NOT NULL,
        url              TEXT    NOT NULL,
        author           TEXT,
        author_avatar    TEXT,
        is_draft         INTEGER NOT NULL DEFAULT 0,
        additions        INTEGER NOT NULL DEFAULT 0,
        deletions        INTEGER NOT NULL DEFAULT 0,
        changed_files    INTEGER NOT NULL DEFAULT 0,
        comment_count    INTEGER NOT NULL DEFAULT 0,
        review_decision  TEXT,                  -- APPROVED | CHANGES_REQUESTED | REVIEW_REQUIRED
        checks_state     TEXT,                  -- SUCCESS | FAILURE | PENDING | EXPECTED | ERROR
        head_oid         TEXT,                  -- changes when new commits land
        is_author        INTEGER NOT NULL DEFAULT 0,
        is_reviewer      INTEGER NOT NULL DEFAULT 0,  -- review explicitly requested of you
        is_mentioned     INTEGER NOT NULL DEFAULT 0,
        created_at       INTEGER NOT NULL,
        updated_at       INTEGER NOT NULL,
        -- Bookkeeping, not GitHub state.
        first_seen_at    INTEGER NOT NULL,
        last_synced_at   INTEGER NOT NULL,
        opened_at        INTEGER                -- when you last read it
    );

    CREATE INDEX pull_requests_account ON pull_requests (account_id, updated_at DESC);
    "#,
    // v3 — daily notes. Deliberately not scoped to an account: these are yours,
    // not GitHub's, and they must survive signing out or switching accounts.
    // The body is stored as plain text, never as parsed structure — the text
    // you typed is the only source of truth, so nothing can drift out of sync
    // with it and your notes stay readable without this app.
    r#"
    CREATE TABLE notes (
        day        TEXT PRIMARY KEY,   -- YYYY-MM-DD, local time
        body       TEXT NOT NULL DEFAULT '',
        created_at INTEGER NOT NULL,
        updated_at INTEGER NOT NULL
    );
    "#,
];

pub fn open(path: &Path) -> Result<Connection> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let conn = Connection::open(path)?;
    conn.pragma_update(None, "journal_mode", "WAL")?;
    conn.pragma_update(None, "synchronous", "NORMAL")?;
    conn.pragma_update(None, "foreign_keys", "ON")?;
    migrate(&conn)?;
    Ok(conn)
}

fn migrate(conn: &Connection) -> Result<()> {
    let version: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    for (i, sql) in MIGRATIONS.iter().enumerate().skip(version as usize) {
        conn.execute_batch(sql)?;
        conn.pragma_update(None, "user_version", (i + 1) as i64)?;
    }
    Ok(())
}

#[cfg(test)]
pub fn open_memory() -> Result<Connection> {
    let conn = Connection::open_in_memory()?;
    migrate(&conn)?;
    Ok(conn)
}

pub fn schema_version(conn: &Connection) -> Result<i64> {
    Ok(conn.query_row("PRAGMA user_version", [], |r| r.get(0))?)
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Account {
    pub id: i64,
    pub login: String,
    pub name: Option<String>,
    pub avatar_url: Option<String>,
    pub host: String,
    pub provider: String,
    pub connected_at: i64,
}

pub fn upsert_account(
    conn: &Connection,
    provider: &str,
    host: &str,
    login: &str,
    name: Option<&str>,
    avatar_url: Option<&str>,
) -> Result<Account> {
    let now = now_secs();
    conn.execute(
        "INSERT INTO accounts (provider, host, login, name, avatar_url, connected_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)
         ON CONFLICT (host, login) DO UPDATE SET
             provider     = excluded.provider,
             name         = excluded.name,
             avatar_url   = excluded.avatar_url,
             connected_at = excluded.connected_at",
        rusqlite::params![provider, host, login, name, avatar_url, now],
    )?;
    current_account(conn)?.ok_or_else(|| crate::error::Error::msg("account vanished after write"))
}

/// M0 is single-account; multi-account arrives with the account switcher.
pub fn current_account(conn: &Connection) -> Result<Option<Account>> {
    Ok(conn
        .query_row(
            "SELECT id, login, name, avatar_url, host, provider, connected_at
             FROM accounts ORDER BY connected_at DESC LIMIT 1",
            [],
            |r| {
                Ok(Account {
                    id: r.get(0)?,
                    login: r.get(1)?,
                    name: r.get(2)?,
                    avatar_url: r.get(3)?,
                    host: r.get(4)?,
                    provider: r.get(5)?,
                    connected_at: r.get(6)?,
                })
            },
        )
        .optional()?)
}

pub fn delete_account(conn: &Connection, id: i64) -> Result<()> {
    conn.execute("DELETE FROM accounts WHERE id = ?1", [id])?;
    Ok(())
}

/// Append-only. Nothing in here is ever updated except `seen_at` / `notified_at`,
/// which is what makes notifications idempotent across restarts.
pub fn record_event(
    conn: &Connection,
    account_id: i64,
    subject_type: &str,
    subject_id: &str,
    kind: &str,
    from_state: Option<&str>,
    to_state: Option<&str>,
) -> Result<i64> {
    conn.execute(
        "INSERT INTO events
           (account_id, subject_type, subject_id, kind, from_state, to_state, occurred_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        rusqlite::params![
            account_id,
            subject_type,
            subject_id,
            kind,
            from_state,
            to_state,
            now_secs()
        ],
    )?;
    Ok(conn.last_insert_rowid())
}

pub fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

// ---------------------------------------------------------------------------
// Pull requests
// ---------------------------------------------------------------------------

/// What a sync wants to write. Separate from the GitHub response type so the
/// database layer never depends on the shape of the API.
#[derive(Debug, Clone)]
pub struct PrInput {
    pub id: String,
    pub repo: String,
    pub number: i64,
    pub title: String,
    pub url: String,
    pub author: Option<String>,
    pub author_avatar: Option<String>,
    pub is_draft: bool,
    pub additions: i64,
    pub deletions: i64,
    pub changed_files: i64,
    pub comment_count: i64,
    pub review_decision: Option<String>,
    pub checks_state: Option<String>,
    pub head_oid: Option<String>,
    pub is_author: bool,
    pub is_reviewer: bool,
    pub is_mentioned: bool,
    pub created_at: i64,
    pub updated_at: i64,
}

/// The fields a change is worth noticing on. Everything else (title edits,
/// line counts) updates silently.
#[derive(Debug, Default)]
struct PrState {
    review_decision: Option<String>,
    checks_state: Option<String>,
    head_oid: Option<String>,
    comment_count: i64,
    is_reviewer: bool,
}

fn previous_state(conn: &Connection, id: &str) -> Result<Option<PrState>> {
    Ok(conn
        .query_row(
            "SELECT review_decision, checks_state, head_oid, comment_count, is_reviewer
             FROM pull_requests WHERE id = ?1",
            [id],
            |r| {
                Ok(PrState {
                    review_decision: r.get(0)?,
                    checks_state: r.get(1)?,
                    head_oid: r.get(2)?,
                    comment_count: r.get(3)?,
                    is_reviewer: r.get::<_, i64>(4)? != 0,
                })
            },
        )
        .optional()?)
}

/// Writes a pull request and, in the same transaction, appends an event for
/// every transition worth telling you about. M2 turns these into notifications
/// by reading the rows where `notified_at IS NULL` — which is why nothing is
/// ever notified twice, and why a change that landed while the app was closed
/// is still waiting when it opens.
pub fn upsert_pull_request(conn: &Connection, account_id: i64, pr: &PrInput) -> Result<usize> {
    let now = now_secs();
    let previous = previous_state(conn, &pr.id)?;
    let mut events = 0;

    let mut note = |kind: &str, from: Option<&str>, to: Option<&str>| -> Result<()> {
        record_event(conn, account_id, "pull_request", &pr.id, kind, from, to)?;
        events += 1;
        Ok(())
    };

    match &previous {
        None => {
            // First sighting. Only worth an event if it actually wants you —
            // otherwise the first sync of a busy account fires fifty of them.
            if pr.is_reviewer {
                note("review_requested", None, Some(&pr.repo))?;
            }
        }
        Some(prev) => {
            if !prev.is_reviewer && pr.is_reviewer {
                note("review_requested", None, Some(&pr.repo))?;
            }
            if prev.review_decision.as_deref() != pr.review_decision.as_deref() {
                note(
                    "review_decision",
                    prev.review_decision.as_deref(),
                    pr.review_decision.as_deref(),
                )?;
            }
            if prev.checks_state.as_deref() != pr.checks_state.as_deref() {
                note("checks", prev.checks_state.as_deref(), pr.checks_state.as_deref())?;
            }
            if prev.head_oid.as_deref() != pr.head_oid.as_deref() {
                note("new_commits", prev.head_oid.as_deref(), pr.head_oid.as_deref())?;
            }
            if pr.comment_count > prev.comment_count {
                note(
                    "new_comments",
                    Some(&prev.comment_count.to_string()),
                    Some(&pr.comment_count.to_string()),
                )?;
            }
        }
    }

    conn.execute(
        "INSERT INTO pull_requests (
             id, account_id, repo, number, title, url, author, author_avatar, is_draft,
             additions, deletions, changed_files, comment_count, review_decision,
             checks_state, head_oid, is_author, is_reviewer, is_mentioned,
             created_at, updated_at, first_seen_at, last_synced_at
         ) VALUES (
             ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9,
             ?10, ?11, ?12, ?13, ?14,
             ?15, ?16, ?17, ?18, ?19,
             ?20, ?21, ?22, ?22
         )
         ON CONFLICT (id) DO UPDATE SET
             title           = excluded.title,
             author          = excluded.author,
             author_avatar   = excluded.author_avatar,
             is_draft        = excluded.is_draft,
             additions       = excluded.additions,
             deletions       = excluded.deletions,
             changed_files   = excluded.changed_files,
             comment_count   = excluded.comment_count,
             review_decision = excluded.review_decision,
             checks_state    = excluded.checks_state,
             head_oid        = excluded.head_oid,
             is_author       = excluded.is_author,
             is_reviewer     = excluded.is_reviewer,
             is_mentioned    = excluded.is_mentioned,
             updated_at      = excluded.updated_at,
             last_synced_at  = excluded.last_synced_at",
        rusqlite::params![
            pr.id,
            account_id,
            pr.repo,
            pr.number,
            pr.title,
            pr.url,
            pr.author,
            pr.author_avatar,
            pr.is_draft as i64,
            pr.additions,
            pr.deletions,
            pr.changed_files,
            pr.comment_count,
            pr.review_decision,
            pr.checks_state,
            pr.head_oid,
            pr.is_author as i64,
            pr.is_reviewer as i64,
            pr.is_mentioned as i64,
            pr.created_at,
            pr.updated_at,
            now,
        ],
    )?;

    Ok(events)
}

/// A PR that stopped coming back from GitHub was merged or closed. Record the
/// fact, then drop the row — the Desk only ever shows open work.
pub fn retire_missing(conn: &Connection, account_id: i64, synced_at: i64) -> Result<usize> {
    let gone: Vec<(String, String)> = conn
        .prepare(
            "SELECT id, repo FROM pull_requests
             WHERE account_id = ?1 AND last_synced_at < ?2",
        )?
        .query_map(rusqlite::params![account_id, synced_at], |r| {
            Ok((r.get(0)?, r.get(1)?))
        })?
        .collect::<std::result::Result<_, _>>()?;

    for (id, repo) in &gone {
        record_event(conn, account_id, "pull_request", id, "closed", Some(repo), None)?;
    }
    conn.execute(
        "DELETE FROM pull_requests WHERE account_id = ?1 AND last_synced_at < ?2",
        rusqlite::params![account_id, synced_at],
    )?;

    Ok(gone.len())
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PullRequestRow {
    pub id: String,
    pub repo: String,
    pub number: i64,
    pub title: String,
    pub url: String,
    pub author: Option<String>,
    pub author_avatar: Option<String>,
    pub is_draft: bool,
    pub additions: i64,
    pub deletions: i64,
    pub changed_files: i64,
    pub comment_count: i64,
    pub review_decision: Option<String>,
    pub checks_state: Option<String>,
    pub is_author: bool,
    pub is_reviewer: bool,
    pub is_mentioned: bool,
    pub updated_at: i64,
    /// Which lane this belongs in — computed here, never stored, so the rules
    /// can change without a migration or a re-sync.
    pub lane: String,
    /// Transitions since you last opened it, newest first.
    pub unread: Vec<String>,
}

fn lane_for(is_author: bool, is_reviewer: bool, is_mentioned: bool) -> &'static str {
    if is_author {
        "yours"
    } else if is_reviewer || is_mentioned {
        "needs_you"
    } else {
        "watching"
    }
}

pub fn list_pull_requests(conn: &Connection, account_id: i64) -> Result<Vec<PullRequestRow>> {
    let mut stmt = conn.prepare(
        "SELECT id, repo, number, title, url, author, author_avatar, is_draft,
                additions, deletions, changed_files, comment_count, review_decision,
                checks_state, is_author, is_reviewer, is_mentioned, updated_at, opened_at
         FROM pull_requests
         WHERE account_id = ?1
         ORDER BY updated_at DESC",
    )?;

    let rows = stmt
        .query_map([account_id], |r| {
            let is_author: i64 = r.get(14)?;
            let is_reviewer: i64 = r.get(15)?;
            let is_mentioned: i64 = r.get(16)?;
            Ok((
                PullRequestRow {
                    id: r.get(0)?,
                    repo: r.get(1)?,
                    number: r.get(2)?,
                    title: r.get(3)?,
                    url: r.get(4)?,
                    author: r.get(5)?,
                    author_avatar: r.get(6)?,
                    is_draft: r.get::<_, i64>(7)? != 0,
                    additions: r.get(8)?,
                    deletions: r.get(9)?,
                    changed_files: r.get(10)?,
                    comment_count: r.get(11)?,
                    review_decision: r.get(12)?,
                    checks_state: r.get(13)?,
                    is_author: is_author != 0,
                    is_reviewer: is_reviewer != 0,
                    is_mentioned: is_mentioned != 0,
                    updated_at: r.get(17)?,
                    lane: lane_for(is_author != 0, is_reviewer != 0, is_mentioned != 0).to_string(),
                    unread: Vec::new(),
                },
                r.get::<_, Option<i64>>(18)?,
            ))
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;

    // "What changed since you looked" comes from the event log, not from a
    // diff of the current state — the whole reason that table exists.
    let mut out = Vec::with_capacity(rows.len());
    for (mut pr, opened_at) in rows {
        let since = opened_at.unwrap_or(0);
        pr.unread = conn
            .prepare(
                "SELECT kind FROM events
                 WHERE subject_type = 'pull_request' AND subject_id = ?1 AND occurred_at > ?2
                 ORDER BY occurred_at DESC LIMIT 6",
            )?
            .query_map(rusqlite::params![pr.id, since], |r| r.get::<_, String>(0))?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        out.push(pr);
    }

    Ok(out)
}

/// Marks a PR read: everything before now stops counting as new.
pub fn mark_opened(conn: &Connection, id: &str) -> Result<()> {
    conn.execute(
        "UPDATE pull_requests SET opened_at = ?2 WHERE id = ?1",
        rusqlite::params![id, now_secs()],
    )?;
    Ok(())
}

pub fn set_sync_meta(conn: &Connection, key: &str, at: i64) -> Result<()> {
    conn.execute(
        "INSERT INTO sync_meta (key, last_polled_at) VALUES (?1, ?2)
         ON CONFLICT (key) DO UPDATE SET last_polled_at = excluded.last_polled_at",
        rusqlite::params![key, at],
    )?;
    Ok(())
}

pub fn get_sync_meta(conn: &Connection, key: &str) -> Result<Option<i64>> {
    Ok(conn
        .query_row("SELECT last_polled_at FROM sync_meta WHERE key = ?1", [key], |r| {
            r.get(0)
        })
        .optional()?)
}

// ---------------------------------------------------------------------------
// Notifications
// ---------------------------------------------------------------------------

/// An unnotified event, with enough of its pull request to write a sentence.
#[derive(Debug)]
pub struct PendingEvent {
    pub kind: String,
    pub to_state: Option<String>,
    pub repo: String,
    pub number: i64,
    pub title: String,
    pub author: Option<String>,
    pub is_author: bool,
    pub is_reviewer: bool,
    pub pr_id: String,
}

/// Events we have never acted on. Joined to the pull request, so anything whose
/// PR has since been retired simply drops out — you don't get told about work
/// that already closed.
pub fn pending_notifications(conn: &Connection, account_id: i64) -> Result<Vec<PendingEvent>> {
    let mut stmt = conn.prepare(
        "SELECT e.kind, e.to_state, p.repo, p.number, p.title, p.author,
                p.is_author, p.is_reviewer, p.id
         FROM events e
         JOIN pull_requests p ON p.id = e.subject_id
         WHERE e.account_id = ?1
           AND e.subject_type = 'pull_request'
           AND e.notified_at IS NULL
         ORDER BY e.occurred_at",
    )?;

    let rows = stmt
        .query_map([account_id], |r| {
            Ok(PendingEvent {
                kind: r.get(0)?,
                to_state: r.get(1)?,
                repo: r.get(2)?,
                number: r.get(3)?,
                title: r.get(4)?,
                author: r.get(5)?,
                is_author: r.get::<_, i64>(6)? != 0,
                is_reviewer: r.get::<_, i64>(7)? != 0,
                pr_id: r.get(8)?,
            })
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(rows)
}

/// Marks everything outstanding as handled — including events we chose not to
/// send, so the backlog can never grow unbounded and nothing fires twice.
pub fn mark_all_notified(conn: &Connection, account_id: i64) -> Result<usize> {
    Ok(conn.execute(
        "UPDATE events SET notified_at = ?2 WHERE account_id = ?1 AND notified_at IS NULL",
        rusqlite::params![account_id, now_secs()],
    )?)
}

/// How many pull requests are actively waiting on you — the tray badge number.
pub fn needs_you_count(conn: &Connection, account_id: i64) -> Result<i64> {
    Ok(conn.query_row(
        "SELECT COUNT(*) FROM pull_requests
         WHERE account_id = ?1 AND is_author = 0 AND (is_reviewer = 1 OR is_mentioned = 1)",
        [account_id],
        |r| r.get(0),
    )?)
}

// ---------------------------------------------------------------------------
// Daily notes
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Note {
    pub day: String,
    pub body: String,
    pub updated_at: i64,
}

pub fn note_get(conn: &Connection, day: &str) -> Result<Option<Note>> {
    Ok(conn
        .query_row(
            "SELECT day, body, updated_at FROM notes WHERE day = ?1",
            [day],
            |r| {
                Ok(Note {
                    day: r.get(0)?,
                    body: r.get(1)?,
                    updated_at: r.get(2)?,
                })
            },
        )
        .optional()?)
}

pub fn note_save(conn: &Connection, day: &str, body: &str) -> Result<Note> {
    let now = now_secs();
    conn.execute(
        "INSERT INTO notes (day, body, created_at, updated_at) VALUES (?1, ?2, ?3, ?3)
         ON CONFLICT (day) DO UPDATE SET body = excluded.body, updated_at = excluded.updated_at",
        rusqlite::params![day, body, now],
    )?;
    Ok(Note {
        day: day.to_string(),
        body: body.to_string(),
        updated_at: now,
    })
}

/// The most recent day written before `day` — where carried-over work comes from.
/// Not simply "yesterday": a Monday should pick up Friday's loose ends.
pub fn note_previous(conn: &Connection, day: &str) -> Result<Option<Note>> {
    Ok(conn
        .query_row(
            "SELECT day, body, updated_at FROM notes
             WHERE day < ?1 ORDER BY day DESC LIMIT 1",
            [day],
            |r| {
                Ok(Note {
                    day: r.get(0)?,
                    body: r.get(1)?,
                    updated_at: r.get(2)?,
                })
            },
        )
        .optional()?)
}

/// Nearest written days either side, for walking through history.
pub fn note_neighbours(conn: &Connection, day: &str) -> Result<(Option<String>, Option<String>)> {
    let prev = conn
        .query_row(
            "SELECT day FROM notes WHERE day < ?1 ORDER BY day DESC LIMIT 1",
            [day],
            |r| r.get::<_, String>(0),
        )
        .optional()?;
    let next = conn
        .query_row(
            "SELECT day FROM notes WHERE day > ?1 ORDER BY day ASC LIMIT 1",
            [day],
            |r| r.get::<_, String>(0),
        )
        .optional()?;
    Ok((prev, next))
}

pub fn note_search(conn: &Connection, needle: &str) -> Result<Vec<Note>> {
    let pattern = format!("%{needle}%");
    let mut stmt = conn.prepare(
        "SELECT day, body, updated_at FROM notes
         WHERE body LIKE ?1 ESCAPE '\\' ORDER BY day DESC LIMIT 50",
    )?;
    let rows = stmt
        .query_map([pattern], |r| {
            Ok(Note {
                day: r.get(0)?,
                body: r.get(1)?,
                updated_at: r.get(2)?,
            })
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(rows)
}
