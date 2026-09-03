//! One SQLite file, on this Mac, and nothing else. No account, no sync,
//! no telemetry. The schema records what Perch *observed*, never what it
//! concluded, so every claim the interface makes can be traced to a row here.

use crate::error::{Error, Result};
use crate::model::*;
use rusqlite::{params, Connection, OptionalExtension, Row};
use std::path::Path;
use time::OffsetDateTime;

pub struct Store {
    conn: Connection,
}

fn to_unix(t: OffsetDateTime) -> i64 {
    t.unix_timestamp()
}

fn from_unix(v: i64) -> Result<OffsetDateTime> {
    OffsetDateTime::from_unix_timestamp(v).map_err(|_| {
        Error::msg(format!(
            "a timestamp in the database is not a real time: {v}"
        ))
    })
}

fn opt_time(v: Option<i64>) -> Result<Option<OffsetDateTime>> {
    v.map(from_unix).transpose()
}

impl Store {
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let conn = Connection::open(path)?;
        Self::prepare(conn)
    }

    pub fn open_in_memory() -> Result<Self> {
        Self::prepare(Connection::open_in_memory()?)
    }

    fn prepare(conn: Connection) -> Result<Self> {
        conn.pragma_update(None, "foreign_keys", "ON")?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        let store = Self { conn };
        store.migrate()?;
        Ok(store)
    }

    fn migrate(&self) -> Result<()> {
        // Take the write lock before reading the version, or two first runs
        // both see 0 and the second fails with "table company already exists".
        self.conn.execute_batch("BEGIN IMMEDIATE")?;
        let result = self.migrate_locked();
        match result {
            Ok(()) => self.conn.execute_batch("COMMIT")?,
            Err(err) => {
                let _ = self.conn.execute_batch("ROLLBACK");
                return Err(err);
            }
        }
        Ok(())
    }

    fn migrate_locked(&self) -> Result<()> {
        let version: i64 = self
            .conn
            .query_row("PRAGMA user_version", [], |row| row.get(0))?;

        if version < 1 {
            // SQLite DDL is transactional, so the tables and the version bump
            // land together or not at all.
            self.conn.execute_batch(
                r#"
                CREATE TABLE IF NOT EXISTS company (
                    id       INTEGER PRIMARY KEY,
                    name     TEXT NOT NULL,
                    slug     TEXT NOT NULL UNIQUE,
                    added_at INTEGER NOT NULL
                );

                CREATE TABLE IF NOT EXISTS board (
                    id              INTEGER PRIMARY KEY,
                    company_id      INTEGER NOT NULL REFERENCES company(id) ON DELETE CASCADE,
                    ats             TEXT NOT NULL,
                    token           TEXT NOT NULL,
                    url             TEXT NOT NULL,
                    fill_supported  INTEGER NOT NULL,
                    last_checked_at INTEGER,
                    UNIQUE(ats, token)
                );

                CREATE TABLE IF NOT EXISTS role (
                    id                INTEGER PRIMARY KEY,
                    board_id          INTEGER NOT NULL REFERENCES board(id) ON DELETE CASCADE,
                    reference         TEXT NOT NULL,
                    external_id       TEXT NOT NULL,
                    title             TEXT NOT NULL,
                    location          TEXT NOT NULL,
                    url               TEXT NOT NULL,
                    posted_at         INTEGER,
                    remote_updated_at INTEGER,
                    first_seen_at     INTEGER NOT NULL,
                    last_seen_at      INTEGER NOT NULL,
                    closed_at         INTEGER,
                    dismissed_at      INTEGER,
                    UNIQUE(board_id, external_id)
                );

                CREATE TABLE IF NOT EXISTS role_event (
                    id      INTEGER PRIMARY KEY,
                    role_id INTEGER NOT NULL REFERENCES role(id) ON DELETE CASCADE,
                    at      INTEGER NOT NULL,
                    kind    TEXT NOT NULL,
                    detail  TEXT
                );

                CREATE INDEX IF NOT EXISTS role_by_board ON role(board_id);
                CREATE INDEX IF NOT EXISTS role_by_reference ON role(reference);
                CREATE INDEX IF NOT EXISTS role_open ON role(closed_at, dismissed_at);
                CREATE INDEX IF NOT EXISTS event_by_role ON role_event(role_id, at);

                PRAGMA user_version = 1;
                "#,
            )?;
        }

        if version < 2 {
            // Stopping watching is a date, not a deletion. Same shape as
            // dismissing a role, and for the same reason: it has to be undoable
            // and the observed history has to survive it.
            self.conn.execute_batch(
                r#"
                ALTER TABLE company ADD COLUMN unwatched_at INTEGER;
                PRAGMA user_version = 2;
                "#,
            )?;
        }

        if version < 3 {
            // Descriptions arrive lazily, and applications are the other half
            // of the queue: what the person sent, and what came back.
            self.conn.execute_batch(
                r#"
                ALTER TABLE role ADD COLUMN description TEXT;
                ALTER TABLE role ADD COLUMN description_fetched_at INTEGER;

                CREATE TABLE IF NOT EXISTS application (
                    id            INTEGER PRIMARY KEY,
                    role_id       INTEGER NOT NULL UNIQUE REFERENCES role(id) ON DELETE CASCADE,
                    state         TEXT NOT NULL,
                    applied_at    INTEGER NOT NULL,
                    note          TEXT,
                    note_at       INTEGER,
                    archived_at   INTEGER,
                    archived_quietly INTEGER NOT NULL DEFAULT 0
                );

                CREATE TABLE IF NOT EXISTS application_event (
                    id             INTEGER PRIMARY KEY,
                    application_id INTEGER NOT NULL REFERENCES application(id) ON DELETE CASCADE,
                    at             INTEGER NOT NULL,
                    kind           TEXT NOT NULL,
                    note           TEXT
                );

                CREATE INDEX IF NOT EXISTS application_by_state ON application(state);
                CREATE INDEX IF NOT EXISTS application_event_by_app
                    ON application_event(application_id, at);

                PRAGMA user_version = 3;
                "#,
            )?;
        }

        Ok(())
    }

    /// The date a role's feed line describes, as SQL.
    ///
    /// This is `timesignal::signal`'s rule, and it is written here exactly once
    /// so ordering and filtering cannot drift apart from the words the row
    /// prints. Every value interpolated is an i64 this function computed.
    fn signal_date_sql(now: OffsetDateTime) -> String {
        let posted = "COALESCE(r.posted_at, r.first_seen_at)";
        let gap = crate::timesignal::REPOST_MIN_GAP.whole_seconds();
        let floor = to_unix(now - crate::timesignal::REPOST_WINDOW);
        let now_unix = to_unix(now);
        format!(
            "CASE WHEN r.remote_updated_at IS NOT NULL
                   AND r.remote_updated_at - {posted} > {gap}
                   AND r.remote_updated_at > {floor}
                   AND r.remote_updated_at <= {now_unix}
              THEN r.remote_updated_at ELSE {posted} END"
        )
    }

    // ---- watchlist ------------------------------------------------------

    /// Resolve a company the way a person would name one: the slug of its
    /// display name, that slug without separators, or the board token they
    /// typed at `watch add`. The stored slug comes from the board's display
    /// name, so matching on it alone rejects the very word that worked.
    pub fn company_by_key(&self, key: &str) -> Result<Option<Company>> {
        let slug = slugify(key);
        let bare = slug.replace('-', "");
        self.conn
            .query_row(
                "SELECT c.id, c.name, c.slug, c.added_at FROM company c
                 WHERE c.slug = ?1
                    OR REPLACE(c.slug, '-', '') = ?2
                    OR EXISTS (SELECT 1 FROM board b
                               WHERE b.company_id = c.id AND b.token = ?2)
                 LIMIT 1",
                params![slug, bare],
                |row| {
                    Ok((
                        row.get::<_, i64>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, i64>(3)?,
                    ))
                },
            )
            .optional()?
            .map(|(id, name, slug, added_at)| {
                Ok(Company {
                    id,
                    name,
                    slug,
                    added_at: from_unix(added_at)?,
                })
            })
            .transpose()
    }

    /// A company currently being watched, as opposed to one merely on record.
    pub fn watched_company(&self, key: &str) -> Result<Option<Company>> {
        let Some(company) = self.company_by_key(key)? else {
            return Ok(None);
        };
        let watched: bool = self.conn.query_row(
            "SELECT unwatched_at IS NULL FROM company WHERE id = ?1",
            params![company.id],
            |row| row.get(0),
        )?;
        Ok(watched.then_some(company))
    }

    /// Records a watched company and the board Perch found for it.
    pub fn watch(&mut self, found: &DetectedBoard, now: OffsetDateTime) -> Result<Board> {
        let slug = slugify(&found.company_name);
        if let Some(existing) = self.company_by_key(&slug)? {
            let watched: bool = self.conn.query_row(
                "SELECT unwatched_at IS NULL FROM company WHERE id = ?1",
                params![existing.id],
                |row| row.get(0),
            )?;
            if watched {
                return Err(Error::AlreadyWatched(found.company_name.clone()));
            }
            // Watching again picks up exactly where it left off: every role,
            // dismissal and observed event is still there.
            self.conn.execute(
                "UPDATE company SET unwatched_at = NULL WHERE id = ?1",
                params![existing.id],
            )?;
            let board = self
                .boards()?
                .into_iter()
                .find(|b| b.company_id == existing.id)
                .ok_or_else(|| Error::msg("that company is on record with no board"))?;
            return Ok(board);
        }

        let tx = self.conn.transaction()?;
        tx.execute(
            "INSERT INTO company (name, slug, added_at) VALUES (?1, ?2, ?3)",
            params![found.company_name, slug, to_unix(now)],
        )?;
        let company_id = tx.last_insert_rowid();
        tx.execute(
            "INSERT INTO board (company_id, ats, token, url, fill_supported)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                company_id,
                found.ats.as_str(),
                found.token,
                found.url,
                found.fill_supported as i64
            ],
        )?;
        let board_id = tx.last_insert_rowid();
        tx.commit()?;

        Ok(Board {
            id: board_id,
            company_id,
            company_name: found.company_name.clone(),
            ats: found.ats,
            token: found.token.clone(),
            url: found.url.clone(),
            fill_supported: found.fill_supported,
            last_checked_at: None,
        })
    }

    /// Stops watching without destroying anything. Perch does not delete, so
    /// this sets a date exactly as dismissing a role does, and `watch add` on
    /// the same company clears it again with its history intact.
    pub fn unwatch(&self, key: &str, now: OffsetDateTime) -> Result<bool> {
        let Some(company) = self.watched_company(key)? else {
            return Ok(false);
        };
        self.conn.execute(
            "UPDATE company SET unwatched_at = ?1 WHERE id = ?2",
            params![to_unix(now), company.id],
        )?;
        Ok(true)
    }

    pub fn boards(&self) -> Result<Vec<Board>> {
        let mut stmt = self.conn.prepare(
            "SELECT b.id, b.company_id, c.name, b.ats, b.token, b.url,
                    b.fill_supported, b.last_checked_at
             FROM board b JOIN company c ON c.id = b.company_id
             WHERE c.unwatched_at IS NULL
             ORDER BY c.name COLLATE NOCASE",
        )?;
        let rows = stmt
            .query_map([], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, String>(5)?,
                    row.get::<_, i64>(6)?,
                    row.get::<_, Option<i64>>(7)?,
                ))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;

        rows.into_iter()
            .map(
                |(id, company_id, company_name, ats, token, url, fill, checked)| {
                    Ok(Board {
                        id,
                        company_id,
                        company_name,
                        ats: Ats::parse(&ats)?,
                        token,
                        url,
                        fill_supported: fill != 0,
                        last_checked_at: opt_time(checked)?,
                    })
                },
            )
            .collect()
    }

    /// How many roles Perch has ever seen on this board, and how many are open.
    /// Both numbers are bounded by when watching started, and the interface
    /// says so wherever it prints them.
    pub fn board_tally(&self, board_id: i64) -> Result<(i64, i64)> {
        let seen: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM role WHERE board_id = ?1",
            params![board_id],
            |row| row.get(0),
        )?;
        let open: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM role WHERE board_id = ?1 AND closed_at IS NULL",
            params![board_id],
            |row| row.get(0),
        )?;
        Ok((seen, open))
    }

    // ---- roles ----------------------------------------------------------

    const ROLE_COLUMNS: &'static str = "r.id, r.board_id, c.name, b.fill_supported, b.ats,
         r.reference, r.external_id, r.title, r.location, r.url,
         r.posted_at, r.remote_updated_at, r.first_seen_at, r.last_seen_at,
         r.closed_at, r.dismissed_at, r.description, r.description_fetched_at";

    fn hydrate(row: &Row<'_>) -> rusqlite::Result<RoleRaw> {
        Ok(RoleRaw {
            id: row.get(0)?,
            board_id: row.get(1)?,
            company_name: row.get(2)?,
            fill_supported: row.get::<_, i64>(3)? != 0,
            ats: row.get(4)?,
            reference: row.get(5)?,
            external_id: row.get(6)?,
            title: row.get(7)?,
            location: row.get(8)?,
            url: row.get(9)?,
            posted_at: row.get(10)?,
            remote_updated_at: row.get(11)?,
            first_seen_at: row.get(12)?,
            last_seen_at: row.get(13)?,
            closed_at: row.get(14)?,
            dismissed_at: row.get(15)?,
            description: row.get(16)?,
            description_fetched_at: row.get(17)?,
        })
    }

    /// Open roles, newest first. Dismissed and closed postings are not here;
    /// they are still in the database, because Perch does not delete things.
    pub fn feed(&self, filter: &FeedFilter, now: OffsetDateTime) -> Result<Vec<Role>> {
        let mut sql = format!(
            "SELECT {} FROM role r
             JOIN board b ON b.id = r.board_id
             JOIN company c ON c.id = b.company_id
             WHERE r.closed_at IS NULL AND r.dismissed_at IS NULL
               AND c.unwatched_at IS NULL",
            Self::ROLE_COLUMNS
        );
        if filter.company.is_some() {
            sql.push_str(" AND c.slug = ?1");
        }
        let signal_date = Self::signal_date_sql(now);
        if filter.fresh_only {
            // `feed --fresh` asks the same question the row's own line answers:
            // a role reposted this morning reads "reposted 4 hours ago" and is
            // fresh, so it belongs here even though it was published in March.
            let cutoff = to_unix(now - time::Duration::days(1));
            sql.push_str(&format!(" AND {signal_date} >= {cutoff}"));
        }
        sql.push_str(&format!(" ORDER BY {signal_date} DESC, r.id DESC"));

        let mut stmt = self.conn.prepare(&sql)?;
        let raws: Vec<RoleRaw> = match &filter.company {
            Some(slug) => stmt
                .query_map(params![slug], Self::hydrate)?
                .collect::<rusqlite::Result<_>>()?,
            None => stmt
                .query_map([], Self::hydrate)?
                .collect::<rusqlite::Result<_>>()?,
        };

        let mut roles = Vec::with_capacity(raws.len());
        for raw in raws {
            roles.push(raw.build()?);
        }
        Ok(roles)
    }

    /// Look a role up by the short handle the feed printed, or any unambiguous
    /// prefix of it.
    pub fn role_by_reference(&self, prefix: &str) -> Result<Role> {
        let needle = prefix.trim().to_ascii_lowercase();
        // A reference is eight hex digits. Anything else cannot name a role,
        // and must not be allowed to reach a LIKE pattern, where `%` would
        // quietly match one the person never asked for.
        if needle.is_empty() || needle.len() > 8 || !needle.chars().all(|c| c.is_ascii_hexdigit()) {
            return Err(Error::NoSuchRole(prefix.to_string()));
        }
        let sql = format!(
            "SELECT {} FROM role r
             JOIN board b ON b.id = r.board_id
             JOIN company c ON c.id = b.company_id
             WHERE r.reference >= ?1 AND r.reference < ?1 || 'g'
             LIMIT 2",
            Self::ROLE_COLUMNS
        );
        let mut stmt = self.conn.prepare(&sql)?;
        let raws: Vec<RoleRaw> = stmt
            .query_map(params![needle], Self::hydrate)?
            .collect::<rusqlite::Result<_>>()?;

        match raws.len() {
            0 => Err(Error::NoSuchRole(prefix.to_string())),
            1 => raws.into_iter().next().unwrap().build(),
            _ => Err(Error::AmbiguousRole(prefix.to_string())),
        }
    }

    /// Dismissing sets a date; it never removes the row. `x` in the interface
    /// means the same thing, and both are undoable for the same reason.
    pub fn dismiss(&self, role_id: i64, now: OffsetDateTime) -> Result<()> {
        self.conn.execute(
            "UPDATE role SET dismissed_at = ?1 WHERE id = ?2",
            params![to_unix(now), role_id],
        )?;
        Ok(())
    }

    pub fn undismiss(&self, role_id: i64) -> Result<()> {
        self.conn.execute(
            "UPDATE role SET dismissed_at = NULL WHERE id = ?1",
            params![role_id],
        )?;
        Ok(())
    }

    pub fn events(&self, role_id: i64) -> Result<Vec<RoleEvent>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, role_id, at, kind, detail FROM role_event
             WHERE role_id = ?1 ORDER BY at ASC, id ASC",
        )?;
        let rows = stmt
            .query_map(params![role_id], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, Option<String>>(4)?,
                ))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;

        rows.into_iter()
            .map(|(id, role_id, at, kind, detail)| {
                Ok(RoleEvent {
                    id,
                    role_id,
                    at: from_unix(at)?,
                    kind: EventKind::parse(&kind)?,
                    detail,
                })
            })
            .collect()
    }

    // ---- descriptions -----------------------------------------------------

    pub fn set_description(&self, role_id: i64, text: &str, now: OffsetDateTime) -> Result<()> {
        self.conn.execute(
            "UPDATE role SET description = ?1, description_fetched_at = ?2 WHERE id = ?3",
            params![text, to_unix(now), role_id],
        )?;
        Ok(())
    }

    pub fn role_by_id(&self, id: i64) -> Result<Role> {
        let sql = format!(
            "SELECT {} FROM role r
             JOIN board b ON b.id = r.board_id
             JOIN company c ON c.id = b.company_id
             WHERE r.id = ?1",
            Self::ROLE_COLUMNS
        );
        let raw = self
            .conn
            .query_row(&sql, params![id], Self::hydrate)
            .optional()?
            .ok_or_else(|| Error::NoSuchRole(id.to_string()))?;
        raw.build()
    }

    pub fn board_by_id(&self, id: i64) -> Result<Option<Board>> {
        Ok(self.boards()?.into_iter().find(|b| b.id == id))
    }

    // ---- hiring velocity ---------------------------------------------------

    /// What a board has actually done lately.
    ///
    /// Every number is measured from postings Perch watched itself, so a board
    /// added last week cannot produce a claim about last year. With too little
    /// history there is no stat at all.
    pub fn velocity(&self, board_id: i64, now: OffsetDateTime) -> Result<Option<Velocity>> {
        let watching_since: Option<i64> = self
            .conn
            .query_row(
                "SELECT MIN(first_seen_at) FROM role WHERE board_id = ?1",
                params![board_id],
                |row| row.get(0),
            )
            .optional()?
            .flatten();
        let Some(watching_since) = watching_since else {
            return Ok(None);
        };
        let watching_since = from_unix(watching_since)?;

        let window = to_unix(now - time::Duration::days(90));
        let opened_recently: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM role
             WHERE board_id = ?1 AND COALESCE(posted_at, first_seen_at) >= ?2",
            params![board_id, window],
            |row| row.get(0),
        )?;
        if opened_recently < 2 {
            // One posting is an anecdote, not a rate.
            return Ok(None);
        }
        let still_open: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM role
             WHERE board_id = ?1 AND closed_at IS NULL
               AND COALESCE(posted_at, first_seen_at) >= ?2",
            params![board_id, window],
            |row| row.get(0),
        )?;

        let mut stmt = self.conn.prepare(
            "SELECT (closed_at - COALESCE(posted_at, first_seen_at)) / 86400
             FROM role
             WHERE board_id = ?1 AND closed_at IS NOT NULL
               AND closed_at > COALESCE(posted_at, first_seen_at)
             ORDER BY 1",
        )?;
        let spans: Vec<i64> = stmt
            .query_map(params![board_id], |row| row.get(0))?
            .collect::<rusqlite::Result<_>>()?;
        // Three closures is the fewest that makes a median mean anything.
        let median_days_to_close = (spans.len() >= 3).then(|| spans[spans.len() / 2]);

        Ok(Some(Velocity {
            opened_recently: opened_recently as usize,
            still_open: still_open as usize,
            median_days_to_close,
            watching_since,
        }))
    }

    // ---- applications ------------------------------------------------------

    fn application_raw(row: &Row<'_>) -> rusqlite::Result<ApplicationRaw> {
        Ok(ApplicationRaw {
            id: row.get(0)?,
            role_id: row.get(1)?,
            company_name: row.get(2)?,
            title: row.get(3)?,
            reference: row.get(4)?,
            state: row.get(5)?,
            applied_at: row.get(6)?,
            note: row.get(7)?,
            note_at: row.get(8)?,
            archived_at: row.get(9)?,
            archived_quietly: row.get(10)?,
        })
    }

    /// After a month of silence Perch files an application away on its own.
    ///
    /// It is recorded as an event so the row can say what happened and the
    /// person can tell it apart from one they archived themselves. Nothing is
    /// deleted, and marking a reply brings it straight back.
    pub fn archive_quiet_applications(&self, now: OffsetDateTime) -> Result<usize> {
        let cutoff = to_unix(now - time::Duration::days(30));
        let stale: Vec<i64> = {
            let mut stmt = self.conn.prepare(
                "SELECT id FROM application
                 WHERE state = 'in_flight' AND COALESCE(note_at, applied_at) < ?1",
            )?;
            let rows = stmt.query_map(params![cutoff], |row| row.get(0))?;
            rows.collect::<rusqlite::Result<_>>()?
        };
        for id in &stale {
            self.conn.execute(
                "UPDATE application SET state = 'archived', archived_at = ?1, archived_quietly = 1
                 WHERE id = ?2",
                params![to_unix(now), id],
            )?;
            self.conn.execute(
                "INSERT INTO application_event (application_id, at, kind, note)
                 VALUES (?1, ?2, 'archived', 'no reply in 30 days')",
                params![id, to_unix(now)],
            )?;
        }
        Ok(stale.len())
    }

    /// Record where an application stands, creating it if this is the first
    /// Perch has heard of it.
    pub fn mark_application(
        &self,
        role_id: i64,
        state: ApplicationState,
        note: Option<&str>,
        now: OffsetDateTime,
    ) -> Result<()> {
        let existing: Option<i64> = self
            .conn
            .query_row(
                "SELECT id FROM application WHERE role_id = ?1",
                params![role_id],
                |row| row.get(0),
            )
            .optional()?;

        let id = match existing {
            Some(id) => {
                self.conn.execute(
                    "UPDATE application
                     SET state = ?1,
                         note = COALESCE(?2, note),
                         note_at = CASE WHEN ?2 IS NULL THEN note_at ELSE ?3 END,
                         archived_at = CASE WHEN ?1 = 'archived' THEN ?3 ELSE NULL END,
                         archived_quietly = 0
                     WHERE id = ?4",
                    params![state.as_str(), note, to_unix(now), id],
                )?;
                id
            }
            None => {
                self.conn.execute(
                    "INSERT INTO application (role_id, state, applied_at, note, note_at, archived_at)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                    params![
                        role_id,
                        state.as_str(),
                        to_unix(now),
                        note,
                        note.map(|_| to_unix(now)),
                        (state == ApplicationState::Archived).then(|| to_unix(now)),
                    ],
                )?;
                self.conn.last_insert_rowid()
            }
        };

        self.conn.execute(
            "INSERT INTO application_event (application_id, at, kind, note)
             VALUES (?1, ?2, ?3, ?4)",
            params![id, to_unix(now), state.as_str(), note],
        )?;
        Ok(())
    }

    /// Applications grouped the way the interface groups them, newest first.
    pub fn applications(&self) -> Result<Vec<Application>> {
        let mut stmt = self.conn.prepare(
            "SELECT a.id, a.role_id, c.name, r.title, r.reference, a.state,
                    a.applied_at, a.note, a.note_at, a.archived_at, a.archived_quietly
             FROM application a
             JOIN role r ON r.id = a.role_id
             JOIN board b ON b.id = r.board_id
             JOIN company c ON c.id = b.company_id
             ORDER BY COALESCE(a.note_at, a.applied_at) DESC, a.id DESC",
        )?;
        let rows = stmt
            .query_map([], Self::application_raw)?
            .collect::<rusqlite::Result<Vec<_>>>()?;

        rows.into_iter().map(ApplicationRaw::build).collect()
    }

    // ---- folding a fetch into the store ------------------------------------

    fn record(
        conn: &Connection,
        role_id: i64,
        at: OffsetDateTime,
        kind: EventKind,
        detail: Option<String>,
    ) -> Result<()> {
        conn.execute(
            "INSERT INTO role_event (role_id, at, kind, detail) VALUES (?1, ?2, ?3, ?4)",
            params![role_id, to_unix(at), kind.as_str(), detail],
        )?;
        Ok(())
    }

    /// Fold one board's listing into the store, recording what changed.
    pub fn absorb(
        &mut self,
        board: &Board,
        listing: &Listing,
        now: OffsetDateTime,
    ) -> Result<SyncReport> {
        let mut report = SyncReport {
            company: board.company_name.clone(),
            ..Default::default()
        };
        let tx = self.conn.transaction()?;

        for job in &listing.roles {
            let reference = role_reference(board.ats, &board.token, &job.external_id);
            let existing: Option<Known> = tx
                .query_row(
                    "SELECT id, title, location, remote_updated_at, closed_at
                     FROM role WHERE board_id = ?1 AND external_id = ?2",
                    params![board.id, job.external_id],
                    |row| {
                        Ok(Known {
                            id: row.get(0)?,
                            title: row.get(1)?,
                            location: row.get(2)?,
                            remote_updated_at: row.get(3)?,
                            closed_at: row.get(4)?,
                        })
                    },
                )
                .optional()?;

            let role_id = match existing {
                None => {
                    tx.execute(
                        "INSERT INTO role (board_id, reference, external_id, title, location, url,
                                           posted_at, remote_updated_at, first_seen_at, last_seen_at)
                         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?9)",
                        params![
                            board.id,
                            reference,
                            job.external_id,
                            job.title,
                            job.location,
                            job.url,
                            job.posted_at.map(to_unix),
                            job.updated_at.map(to_unix),
                            to_unix(now),
                        ],
                    )?;
                    let role_id = tx.last_insert_rowid();
                    Self::record(&tx, role_id, now, EventKind::FirstSeen, None)?;
                    report.first_seen += 1;
                    role_id
                }
                Some(known) => {
                    let Known {
                        id: role_id,
                        title: old_title,
                        location: old_location,
                        remote_updated_at: old_updated,
                        closed_at,
                    } = known;
                    if closed_at.is_some() {
                        Self::record(&tx, role_id, now, EventKind::Reopened, None)?;
                        report.reopened += 1;
                    }
                    if old_title != job.title {
                        Self::record(
                            &tx,
                            role_id,
                            now,
                            EventKind::Retitled,
                            Some(format!("was '{old_title}'")),
                        )?;
                        report.retitled += 1;
                    }
                    if old_location != job.location && !job.location.is_empty() {
                        Self::record(
                            &tx,
                            role_id,
                            now,
                            EventKind::Relocated,
                            Some(format!("was {old_location}")),
                        )?;
                        report.relocated += 1;
                    }
                    let new_updated = job.updated_at.map(to_unix);
                    if let (Some(old), Some(new)) = (old_updated, new_updated) {
                        if new > old {
                            Self::record(&tx, role_id, now, EventKind::Reposted, None)?;
                            report.reposted += 1;
                        }
                    }
                    tx.execute(
                        "UPDATE role SET title = ?1, location = ?2, url = ?3, posted_at = ?4,
                                         remote_updated_at = ?5, last_seen_at = ?6, closed_at = NULL
                         WHERE id = ?7",
                        params![
                            job.title,
                            job.location,
                            job.url,
                            job.posted_at.map(to_unix),
                            new_updated,
                            to_unix(now),
                            role_id
                        ],
                    )?;
                    report.still_open += 1;
                    role_id
                }
            };

            // Some boards send the posting's own words with the listing, so
            // the description is already in hand. It is written separately
            // from the branches above so the change detection still compares
            // the fields it always did. A listing carrying no description says
            // nothing about the one on file: an earlier fetch may have put it
            // there.
            if let Some(text) = &job.description {
                tx.execute(
                    "UPDATE role SET description = ?1, description_fetched_at = ?2
                     WHERE id = ?3",
                    params![text, to_unix(now), role_id],
                )?;
            }
        }

        // Anything Perch saw before and the board no longer lists has come
        // down. It is marked closed, never deleted.
        //
        // The comparison is against everything the board *listed*, not just
        // what parsed, so an entry this build could not read stays open rather
        // than being written into the history as a closure that never happened.
        let seen: Vec<String> = listing.listed_ids.clone();
        let placeholders = if seen.is_empty() {
            "''".to_string()
        } else {
            seen.iter().map(|_| "?").collect::<Vec<_>>().join(",")
        };
        let sql = format!(
            "SELECT id FROM role WHERE board_id = ? AND closed_at IS NULL
             AND external_id NOT IN ({placeholders})"
        );
        let closed_ids: Vec<i64> = {
            let mut stmt = tx.prepare(&sql)?;
            let mut binds: Vec<&dyn rusqlite::ToSql> = vec![&board.id];
            for id in &seen {
                binds.push(id);
            }
            let rows = stmt.query_map(binds.as_slice(), |row| row.get(0))?;
            rows.collect::<rusqlite::Result<_>>()?
        };
        for role_id in closed_ids {
            tx.execute(
                "UPDATE role SET closed_at = ?1 WHERE id = ?2",
                params![to_unix(now), role_id],
            )?;
            Self::record(&tx, role_id, now, EventKind::Closed, None)?;
            report.closed += 1;
        }

        tx.execute(
            "UPDATE board SET last_checked_at = ?1 WHERE id = ?2",
            params![to_unix(now), board.id],
        )?;
        tx.commit()?;
        Ok(report)
    }
}

/// An application row on its way out of SQLite.
struct ApplicationRaw {
    id: i64,
    role_id: i64,
    company_name: String,
    title: String,
    reference: String,
    state: String,
    applied_at: i64,
    note: Option<String>,
    note_at: Option<i64>,
    archived_at: Option<i64>,
    archived_quietly: i64,
}

impl ApplicationRaw {
    fn build(self) -> Result<Application> {
        Ok(Application {
            id: self.id,
            role_id: self.role_id,
            company_name: self.company_name,
            title: self.title,
            reference: self.reference,
            state: ApplicationState::parse(&self.state)?,
            applied_at: from_unix(self.applied_at)?,
            note: self.note,
            note_at: opt_time(self.note_at)?,
            archived_at: opt_time(self.archived_at)?,
            archived_quietly: self.archived_quietly != 0,
        })
    }
}

/// What the store already knows about a posting, so `absorb` can tell what
/// actually changed rather than overwriting and losing the history.
struct Known {
    id: i64,
    title: String,
    location: String,
    remote_updated_at: Option<i64>,
    closed_at: Option<i64>,
}

/// A row on its way out of SQLite, before the columns that need parsing get it.
struct RoleRaw {
    id: i64,
    board_id: i64,
    company_name: String,
    fill_supported: bool,
    ats: String,
    reference: String,
    external_id: String,
    title: String,
    location: String,
    url: String,
    posted_at: Option<i64>,
    remote_updated_at: Option<i64>,
    first_seen_at: i64,
    last_seen_at: i64,
    closed_at: Option<i64>,
    dismissed_at: Option<i64>,
    description: Option<String>,
    description_fetched_at: Option<i64>,
}

impl RoleRaw {
    fn build(self) -> Result<Role> {
        Ok(Role {
            id: self.id,
            board_id: self.board_id,
            company_name: self.company_name,
            ats: Ats::parse(&self.ats)?,
            fill_supported: self.fill_supported,
            reference: self.reference,
            external_id: self.external_id,
            title: self.title,
            location: self.location,
            url: self.url,
            posted_at: opt_time(self.posted_at)?,
            remote_updated_at: opt_time(self.remote_updated_at)?,
            first_seen_at: from_unix(self.first_seen_at)?,
            last_seen_at: from_unix(self.last_seen_at)?,
            closed_at: opt_time(self.closed_at)?,
            dismissed_at: opt_time(self.dismissed_at)?,
            description: self.description,
            description_fetched_at: opt_time(self.description_fetched_at)?,
        })
    }
}

#[derive(Debug, Default, Clone)]
pub struct FeedFilter {
    /// A company slug, when the person asked for one company only.
    pub company: Option<String>,
    /// `feed --fresh`: only what went up in the last day.
    pub fresh_only: bool,
}

#[derive(Debug, Default, Clone)]
pub struct SyncReport {
    pub company: String,
    pub first_seen: usize,
    pub reposted: usize,
    pub retitled: usize,
    pub relocated: usize,
    pub reopened: usize,
    pub closed: usize,
    pub still_open: usize,
}

impl SyncReport {
    pub fn quiet(&self) -> bool {
        self.first_seen == 0
            && self.reposted == 0
            && self.retitled == 0
            && self.relocated == 0
            && self.reopened == 0
            && self.closed == 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::macros::datetime;
    use time::Duration;

    const NOW: OffsetDateTime = datetime!(2026-08-29 12:00 UTC);

    fn days_ago(d: i64) -> OffsetDateTime {
        NOW - Duration::days(d)
    }

    fn board_fixture(store: &mut Store) -> Board {
        store
            .watch(
                &DetectedBoard {
                    ats: Ats::Greenhouse,
                    token: "valtown".into(),
                    url: "https://boards.greenhouse.io/valtown".into(),
                    company_name: "Val Town".into(),
                    fill_supported: true,
                },
                days_ago(200),
            )
            .unwrap()
    }

    fn listing(roles: Vec<RemoteRole>) -> Listing {
        let listed_ids = roles.iter().map(|r| r.external_id.clone()).collect();
        Listing { roles, listed_ids }
    }

    fn role(id: &str, posted: i64, updated: Option<i64>) -> RemoteRole {
        RemoteRole {
            external_id: id.into(),
            title: format!("Engineer {id}"),
            location: "Remote (US)".into(),
            url: format!("https://example.invalid/{id}"),
            posted_at: Some(days_ago(posted)),
            updated_at: updated.map(days_ago),
            description: None,
        }
    }

    #[test]
    fn feed_order_matches_the_printed_signal() {
        // The ordering lives in SQL and the wording lives in Rust. If they ever
        // drift, the feed prints "reposted 3 weeks ago" underneath something a
        // year older, which is exactly the incoherence this test exists to stop.
        let mut store = Store::open_in_memory().unwrap();
        let board = board_fixture(&mut store);
        let remote = vec![
            role("a", 700, None),
            role("b", 700, Some(3)), // ancient posting, reposted this week
            role("c", 2, None),
            role("d", 400, Some(380)), // reposted, but long ago
            role("e", 40, Some(39)),   // touched a day later: not a repost
            role("f", 9, None),
        ];
        store
            .absorb(&board, &listing(remote.clone()), days_ago(1))
            .unwrap();

        let feed = store.feed(&FeedFilter::default(), NOW).unwrap();
        let as_printed: Vec<String> = feed.iter().map(|r| r.external_id.clone()).collect();

        let mut expected = feed.clone();
        expected.sort_by_key(|r| std::cmp::Reverse(r.signal(NOW).at));
        let by_signal: Vec<String> = expected.iter().map(|r| r.external_id.clone()).collect();

        assert_eq!(
            as_printed, by_signal,
            "SQL order and printed signal disagree"
        );
        assert_eq!(as_printed.first().unwrap(), "c");
    }

    #[test]
    fn absorbing_twice_changes_nothing_and_says_so() {
        let mut store = Store::open_in_memory().unwrap();
        let board = board_fixture(&mut store);
        let remote = vec![role("a", 5, None), role("b", 9, None)];

        let first = store
            .absorb(&board, &listing(remote.clone()), days_ago(2))
            .unwrap();
        assert_eq!(first.first_seen, 2);
        assert!(!first.quiet());

        let second = store
            .absorb(&board, &listing(remote.clone()), days_ago(1))
            .unwrap();
        assert_eq!(second.first_seen, 0);
        assert_eq!(second.still_open, 2);
        assert!(second.quiet(), "an unchanged board should sync quietly");
    }

    #[test]
    fn a_posting_that_comes_down_is_closed_and_kept() {
        let mut store = Store::open_in_memory().unwrap();
        let board = board_fixture(&mut store);
        store
            .absorb(
                &board,
                &listing(vec![role("a", 5, None), role("b", 9, None)]),
                days_ago(3),
            )
            .unwrap();

        let gone = store
            .absorb(&board, &listing(vec![role("a", 5, None)]), days_ago(2))
            .unwrap();
        assert_eq!(gone.closed, 1);
        assert_eq!(store.feed(&FeedFilter::default(), NOW).unwrap().len(), 1);
        // Closed, never deleted: the history is the point.
        assert_eq!(store.board_tally(board.id).unwrap(), (2, 1));

        let back = store
            .absorb(
                &board,
                &listing(vec![role("a", 5, None), role("b", 9, None)]),
                days_ago(1),
            )
            .unwrap();
        assert_eq!(back.reopened, 1);
        assert_eq!(store.feed(&FeedFilter::default(), NOW).unwrap().len(), 2);
    }

    #[test]
    fn a_retitle_is_recorded_in_the_words_the_detail_pane_uses() {
        let mut store = Store::open_in_memory().unwrap();
        let board = board_fixture(&mut store);
        store
            .absorb(&board, &listing(vec![role("a", 5, None)]), days_ago(3))
            .unwrap();

        let mut renamed = role("a", 5, None);
        renamed.title = "Systems Engineer, Runtime".into();
        let report = store
            .absorb(&board, &listing(vec![renamed]), days_ago(2))
            .unwrap();
        assert_eq!(report.retitled, 1);

        let found = store.feed(&FeedFilter::default(), NOW).unwrap();
        let events = store.events(found[0].id).unwrap();
        assert_eq!(events[0].kind, EventKind::FirstSeen);
        assert_eq!(events[1].kind, EventKind::Retitled);
        assert_eq!(events[1].detail.as_deref(), Some("was 'Engineer a'"));
    }

    #[test]
    fn dismissing_hides_a_role_without_losing_it() {
        let mut store = Store::open_in_memory().unwrap();
        let board = board_fixture(&mut store);
        store
            .absorb(&board, &listing(vec![role("a", 5, None)]), days_ago(2))
            .unwrap();

        let found = store.feed(&FeedFilter::default(), NOW).unwrap();
        let reference = found[0].reference.clone();

        store.dismiss(found[0].id, NOW).unwrap();
        assert!(store.feed(&FeedFilter::default(), NOW).unwrap().is_empty());
        // Still there, still findable, still countable.
        assert_eq!(store.board_tally(board.id).unwrap(), (1, 1));
        assert!(store.role_by_reference(&reference).is_ok());

        store.undismiss(found[0].id).unwrap();
        assert_eq!(store.feed(&FeedFilter::default(), NOW).unwrap().len(), 1);

        // A later sync must not quietly resurrect something set aside.
        store.dismiss(found[0].id, NOW).unwrap();
        store
            .absorb(&board, &listing(vec![role("a", 5, None)]), NOW)
            .unwrap();
        assert!(
            store.feed(&FeedFilter::default(), NOW).unwrap().is_empty(),
            "syncing brought back a role the person had set aside"
        );
    }

    #[test]
    fn references_are_found_by_prefix_and_ambiguity_is_an_error() {
        let mut store = Store::open_in_memory().unwrap();
        let board = board_fixture(&mut store);
        store
            .absorb(&board, &listing(vec![role("a", 5, None)]), days_ago(2))
            .unwrap();
        let full = store.feed(&FeedFilter::default(), NOW).unwrap()[0]
            .reference
            .clone();

        assert!(store.role_by_reference(&full).is_ok());
        assert!(store.role_by_reference(&full[..4]).is_ok());
        assert!(matches!(
            store.role_by_reference("zzzzzzzz"),
            Err(Error::NoSuchRole(_))
        ));
        assert!(matches!(
            store.role_by_reference(""),
            Err(Error::NoSuchRole(_))
        ));
    }

    #[test]
    fn fresh_only_asks_the_same_clock_the_signal_does() {
        let mut store = Store::open_in_memory().unwrap();
        let board = board_fixture(&mut store);
        store
            .absorb(
                &board,
                &listing(vec![role("new", 0, None), role("old", 3, None)]),
                days_ago(0),
            )
            .unwrap();

        let fresh = FeedFilter {
            fresh_only: true,
            ..Default::default()
        };
        let got = store.feed(&fresh, NOW).unwrap();
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].external_id, "new");
        assert_eq!(
            got[0].signal(NOW).freshness(NOW),
            crate::timesignal::Freshness::Fresh
        );
    }

    #[test]
    fn a_company_can_only_be_watched_once() {
        let mut store = Store::open_in_memory().unwrap();
        board_fixture(&mut store);
        let again = store.watch(
            &DetectedBoard {
                ats: Ats::Greenhouse,
                token: "valtown".into(),
                url: "https://boards.greenhouse.io/valtown".into(),
                company_name: "Val Town".into(),
                fill_supported: true,
            },
            NOW,
        );
        assert!(matches!(again, Err(Error::AlreadyWatched(_))));
    }

    #[test]
    fn unwatching_keeps_everything_and_can_be_undone() {
        // "Nothing is ever deleted" has to be true of stopping watching too,
        // or the interface's `u` on the watchlist cannot exist.
        let mut store = Store::open_in_memory().unwrap();
        let board = board_fixture(&mut store);
        store
            .absorb(
                &board,
                &listing(vec![role("a", 5, None), role("b", 9, None)]),
                days_ago(2),
            )
            .unwrap();
        let dismissed = store.feed(&FeedFilter::default(), NOW).unwrap()[0].id;
        store.dismiss(dismissed, NOW).unwrap();

        assert!(store.unwatch("Val Town", NOW).unwrap());
        assert!(store.feed(&FeedFilter::default(), NOW).unwrap().is_empty());
        assert!(store.boards().unwrap().is_empty());
        assert!(
            !store.unwatch("Val Town", NOW).unwrap(),
            "unwatching twice is not a change"
        );

        // Everything survived: the roles, the events, and the dismissal.
        assert_eq!(store.board_tally(board.id).unwrap(), (2, 2));
        assert!(!store.events(dismissed).unwrap().is_empty());

        let again = store
            .watch(
                &DetectedBoard {
                    ats: Ats::Greenhouse,
                    token: "valtown".into(),
                    url: "https://boards.greenhouse.io/valtown".into(),
                    company_name: "Val Town".into(),
                    fill_supported: true,
                },
                NOW,
            )
            .unwrap();
        assert_eq!(again.id, board.id);
        // One role is back; the other is still set aside, as it was left.
        assert_eq!(store.feed(&FeedFilter::default(), NOW).unwrap().len(), 1);
    }

    #[test]
    fn a_listed_entry_that_would_not_parse_is_not_reported_as_closed() {
        // The board still lists it; this build just could not read it. Closing
        // it would write a closure into the history that never happened.
        let mut store = Store::open_in_memory().unwrap();
        let board = board_fixture(&mut store);
        store
            .absorb(
                &board,
                &listing(vec![role("a", 5, None), role("b", 9, None)]),
                days_ago(2),
            )
            .unwrap();

        let partial = Listing {
            roles: vec![role("a", 5, None)],
            listed_ids: vec!["a".into(), "b".into()],
        };
        let report = store.absorb(&board, &partial, days_ago(1)).unwrap();
        assert_eq!(report.closed, 0);
        assert_eq!(store.feed(&FeedFilter::default(), NOW).unwrap().len(), 2);
    }

    #[test]
    fn fresh_includes_a_repost_the_feed_itself_calls_fresh() {
        // The filter and the printed line must ask the same question, or
        // `--fresh` hides the rows the feed paints brightest.
        let mut store = Store::open_in_memory().unwrap();
        let board = board_fixture(&mut store);
        let old_but_reposted = RemoteRole {
            updated_at: Some(NOW - Duration::hours(4)),
            ..role("reposted", 300, None)
        };
        store
            .absorb(
                &board,
                &listing(vec![
                    old_but_reposted,
                    role("quiet", 300, None),
                    role("new", 0, None),
                ]),
                days_ago(0),
            )
            .unwrap();

        let got = store
            .feed(
                &FeedFilter {
                    fresh_only: true,
                    ..Default::default()
                },
                NOW,
            )
            .unwrap();
        let ids: Vec<&str> = got.iter().map(|r| r.external_id.as_str()).collect();
        assert!(
            ids.contains(&"reposted"),
            "a role reading 'reposted 4 hours ago' was not fresh"
        );
        assert!(ids.contains(&"new"));
        assert!(!ids.contains(&"quiet"));
        let reposted = got.iter().find(|r| r.external_id == "reposted").unwrap();
        assert!(reposted.signal(NOW).text.starts_with("reposted"));
        assert_eq!(
            reposted.signal(NOW).freshness(NOW),
            crate::timesignal::Freshness::Fresh
        );
    }

    #[test]
    fn the_repost_window_edge_agrees_with_the_words() {
        // Rust rejects a repost exactly REPOST_WINDOW old; SQL must too, or a
        // row sorts by a date its own line never mentions.
        let mut store = Store::open_in_memory().unwrap();
        let board = board_fixture(&mut store);
        let edge = RemoteRole {
            updated_at: Some(NOW - crate::timesignal::REPOST_WINDOW),
            ..role("edge", 300, None)
        };
        store
            .absorb(
                &board,
                &listing(vec![edge, role("plain", 31, None)]),
                days_ago(0),
            )
            .unwrap();

        let got = store.feed(&FeedFilter::default(), NOW).unwrap();
        let ids: Vec<&str> = got.iter().map(|r| r.external_id.as_str()).collect();
        assert_eq!(ids, vec!["plain", "edge"]);
        assert_eq!(got[1].signal(NOW).text, "open 300 days");
    }

    #[test]
    fn a_company_answers_to_whatever_name_found_its_board() {
        // The slug comes from the board's display name, but people type the
        // token. Both have to resolve, or `watch add flyio` then
        // `feed --company flyio` finds nothing.
        let mut store = Store::open_in_memory().unwrap();
        store
            .watch(
                &DetectedBoard {
                    ats: Ats::Greenhouse,
                    token: "flyio".into(),
                    url: "https://boards.greenhouse.io/flyio".into(),
                    company_name: "Fly.io".into(),
                    fill_supported: true,
                },
                NOW,
            )
            .unwrap();

        for typed in ["Fly.io", "fly-io", "flyio", "FLY.IO"] {
            assert!(
                store.watched_company(typed).unwrap().is_some(),
                "{typed} did not resolve to the company it added"
            );
        }
        assert!(store.watched_company("figma").unwrap().is_none());
    }

    #[test]
    fn a_reference_that_is_not_a_reference_matches_nothing() {
        // `%` and `_` are LIKE wildcards. A mistyped argument must not set
        // aside a role the person never named.
        let mut store = Store::open_in_memory().unwrap();
        let board = board_fixture(&mut store);
        store
            .absorb(
                &board,
                &listing(vec![role("a", 5, None), role("b", 9, None)]),
                days_ago(2),
            )
            .unwrap();
        let full = store.feed(&FeedFilter::default(), NOW).unwrap()[0]
            .reference
            .clone();

        for bad in [
            "%",
            "_",
            "%%",
            &format!("{}_", &full[..7]),
            "../",
            "zzzz",
            "abcdefghi",
        ] {
            assert!(
                matches!(store.role_by_reference(bad), Err(Error::NoSuchRole(_))),
                "{bad} resolved to a role"
            );
        }
        assert!(store.role_by_reference(&full).is_ok());
        assert!(store.role_by_reference(&full[..4]).is_ok());
    }

    #[test]
    fn a_move_is_a_change_and_the_sync_says_so() {
        let mut store = Store::open_in_memory().unwrap();
        let board = board_fixture(&mut store);
        store
            .absorb(&board, &listing(vec![role("a", 5, None)]), days_ago(3))
            .unwrap();

        let moved = RemoteRole {
            location: "Berlin, Germany".into(),
            ..role("a", 5, None)
        };
        let report = store
            .absorb(&board, &listing(vec![moved]), days_ago(2))
            .unwrap();
        assert_eq!(report.relocated, 1);
        assert!(
            !report.quiet(),
            "a sync that recorded a move reported nothing new"
        );
    }

    #[test]
    fn opening_the_same_database_twice_is_fine() {
        let dir = std::env::temp_dir().join(format!("perch-migrate-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join("perch.db");
        {
            let mut first = Store::open(&path).unwrap();
            board_fixture(&mut first);
        }
        let second = Store::open(&path).unwrap();
        assert_eq!(second.boards().unwrap().len(), 1);
        // And a third open must not replay the migration.
        let third = Store::open(&path).unwrap();
        assert_eq!(third.boards().unwrap().len(), 1);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_application_starts_in_flight_and_remembers_what_came_back() {
        let mut store = Store::open_in_memory().unwrap();
        let board = board_fixture(&mut store);
        store
            .absorb(&board, &listing(vec![role("a", 5, None)]), days_ago(4))
            .unwrap();
        let role_id = store.feed(&FeedFilter::default(), NOW).unwrap()[0].id;

        store
            .mark_application(role_id, ApplicationState::InFlight, None, days_ago(3))
            .unwrap();
        let apps = store.applications().unwrap();
        assert_eq!(apps.len(), 1);
        assert_eq!(apps[0].state, ApplicationState::InFlight);
        assert!(apps[0].note.is_none());

        store
            .mark_application(
                role_id,
                ApplicationState::Responded,
                Some("recruiter screen on Thursday"),
                days_ago(1),
            )
            .unwrap();
        let apps = store.applications().unwrap();
        assert_eq!(
            apps.len(),
            1,
            "marking a reply must not create a second row"
        );
        assert_eq!(apps[0].state, ApplicationState::Responded);
        assert_eq!(
            apps[0].note.as_deref(),
            Some("recruiter screen on Thursday")
        );
    }

    #[test]
    fn silence_past_thirty_days_files_itself_away_quietly() {
        let mut store = Store::open_in_memory().unwrap();
        let board = board_fixture(&mut store);
        store
            .absorb(
                &board,
                &listing(vec![role("old", 60, None), role("recent", 5, None)]),
                days_ago(50),
            )
            .unwrap();
        let roles = store.feed(&FeedFilter::default(), NOW).unwrap();
        let recent = roles.iter().find(|r| r.external_id == "recent").unwrap().id;
        let old = roles.iter().find(|r| r.external_id == "old").unwrap().id;

        store
            .mark_application(old, ApplicationState::InFlight, None, days_ago(40))
            .unwrap();
        store
            .mark_application(recent, ApplicationState::InFlight, None, days_ago(3))
            .unwrap();

        assert_eq!(store.archive_quiet_applications(NOW).unwrap(), 1);
        let apps = store.applications().unwrap();
        let filed = apps.iter().find(|a| a.role_id == old).unwrap();
        assert_eq!(filed.state, ApplicationState::Archived);
        assert!(
            filed.archived_quietly,
            "the row must say Perch did this, not the person"
        );
        let still = apps.iter().find(|a| a.role_id == recent).unwrap();
        assert_eq!(still.state, ApplicationState::InFlight);

        // Running it again changes nothing, and a reply brings one straight back.
        assert_eq!(store.archive_quiet_applications(NOW).unwrap(), 0);
        store
            .mark_application(
                old,
                ApplicationState::Responded,
                Some("they wrote back"),
                NOW,
            )
            .unwrap();
        let back = store
            .applications()
            .unwrap()
            .into_iter()
            .find(|a| a.role_id == old)
            .unwrap();
        assert_eq!(back.state, ApplicationState::Responded);
        assert!(!back.archived_quietly);
        assert!(back.archived_at.is_none());
    }

    #[test]
    fn a_note_alone_keeps_an_application_from_going_quiet() {
        // The clock runs from the last thing that happened, not from the day
        // it was sent. Otherwise recording a reply would not stop the sweep.
        let mut store = Store::open_in_memory().unwrap();
        let board = board_fixture(&mut store);
        store
            .absorb(&board, &listing(vec![role("a", 60, None)]), days_ago(50))
            .unwrap();
        let role_id = store.feed(&FeedFilter::default(), NOW).unwrap()[0].id;

        store
            .mark_application(role_id, ApplicationState::InFlight, None, days_ago(45))
            .unwrap();
        store
            .mark_application(
                role_id,
                ApplicationState::InFlight,
                Some("nudged them"),
                days_ago(2),
            )
            .unwrap();
        assert_eq!(store.archive_quiet_applications(NOW).unwrap(), 0);
    }

    #[test]
    fn velocity_is_absent_until_there_is_enough_to_stand_behind() {
        let mut store = Store::open_in_memory().unwrap();
        let board = board_fixture(&mut store);
        assert!(
            store.velocity(board.id, NOW).unwrap().is_none(),
            "no history, no stat"
        );

        store
            .absorb(&board, &listing(vec![role("a", 5, None)]), days_ago(5))
            .unwrap();
        assert!(
            store.velocity(board.id, NOW).unwrap().is_none(),
            "one posting is an anecdote, not a rate"
        );

        store
            .absorb(
                &board,
                &listing(vec![
                    role("a", 5, None),
                    role("b", 9, None),
                    role("c", 20, None),
                ]),
                days_ago(4),
            )
            .unwrap();
        let v = store.velocity(board.id, NOW).unwrap().unwrap();
        assert_eq!(v.opened_recently, 3);
        assert_eq!(v.still_open, 3);
        assert!(v.median_days_to_close.is_none(), "nothing has closed yet");
        // Every number is bounded by when watching started, and says so.
        assert!(v.watching_since >= days_ago(6));
    }

    #[test]
    fn velocity_counts_only_what_perch_watched_close() {
        let mut store = Store::open_in_memory().unwrap();
        let board = board_fixture(&mut store);
        let all = vec![
            role("a", 40, None),
            role("b", 30, None),
            role("c", 20, None),
            role("d", 10, None),
        ];
        store.absorb(&board, &listing(all), days_ago(45)).unwrap();
        // Three come down; one stays up.
        store
            .absorb(&board, &listing(vec![role("d", 10, None)]), days_ago(1))
            .unwrap();

        let v = store.velocity(board.id, NOW).unwrap().unwrap();
        assert_eq!(v.opened_recently, 4);
        assert_eq!(v.still_open, 1);
        assert!(
            v.median_days_to_close.is_some(),
            "three closures is enough for a median"
        );
    }

    #[test]
    fn a_description_is_stored_once_it_is_asked_for() {
        let mut store = Store::open_in_memory().unwrap();
        let board = board_fixture(&mut store);
        store
            .absorb(&board, &listing(vec![role("a", 5, None)]), days_ago(2))
            .unwrap();
        let role_id = store.feed(&FeedFilter::default(), NOW).unwrap()[0].id;
        assert!(store.role_by_id(role_id).unwrap().description.is_none());

        store
            .set_description(role_id, "<p>We are hiring.</p>", NOW)
            .unwrap();
        let stored = store.role_by_id(role_id).unwrap();
        assert_eq!(stored.description.as_deref(), Some("<p>We are hiring.</p>"));
        assert!(stored.description_fetched_at.is_some());

        // And a later sync does not wipe it.
        store
            .absorb(&board, &listing(vec![role("a", 5, None)]), NOW)
            .unwrap();
        assert!(store.role_by_id(role_id).unwrap().description.is_some());
    }

    #[test]
    fn a_description_that_came_with_the_listing_is_stored_without_being_asked_for() {
        // Ashby sends every description in the list call, so refetching one to
        // read it would be a second copy of a board Perch already has.
        let mut store = Store::open_in_memory().unwrap();
        let board = board_fixture(&mut store);

        let arriving = RemoteRole {
            description: Some("<p>We are hiring.</p>".into()),
            ..role("a", 5, None)
        };
        store
            .absorb(&board, &listing(vec![arriving]), days_ago(3))
            .unwrap();
        let role_id = store.feed(&FeedFilter::default(), NOW).unwrap()[0].id;
        let stored = store.role_by_id(role_id).unwrap();
        assert_eq!(stored.description.as_deref(), Some("<p>We are hiring.</p>"));
        assert!(stored.description_fetched_at.is_some());

        // The second sync takes the update branch, where the role already
        // exists, and the new words still land.
        let rewritten = RemoteRole {
            description: Some("<p>Still hiring.</p>".into()),
            ..role("a", 5, None)
        };
        let report = store
            .absorb(&board, &listing(vec![rewritten]), days_ago(2))
            .unwrap();
        assert_eq!(
            store.role_by_id(role_id).unwrap().description.as_deref(),
            Some("<p>Still hiring.</p>")
        );
        assert!(
            report.quiet(),
            "storing a description was reported as a change to the posting"
        );

        // A listing with no description in it is not a board saying the
        // description is gone, so what is on file stays.
        store
            .absorb(&board, &listing(vec![role("a", 5, None)]), days_ago(1))
            .unwrap();
        assert_eq!(
            store.role_by_id(role_id).unwrap().description.as_deref(),
            Some("<p>Still hiring.</p>")
        );
    }
}
