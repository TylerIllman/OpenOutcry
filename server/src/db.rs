//! Persistence.
//!
//! The command log is the source of truth: replaying it through the engine must
//! reproduce the session exactly. Everything else in here is a projection kept
//! for convenient export.
//!
//! TODO(tyler): nothing is wired up yet. The session actor should append to
//! `command_log` inside `handle()` before broadcasting, so a crash between the
//! two loses the broadcast but never the command.

use rusqlite::Connection;

pub const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS session (
    code            TEXT PRIMARY KEY,
    question        TEXT NOT NULL,
    unit            TEXT NOT NULL,
    tick_size       REAL,
    position_limit  INTEGER NOT NULL,
    phase           TEXT NOT NULL,
    created_at      INTEGER NOT NULL,
    true_value      REAL
);

CREATE TABLE IF NOT EXISTS player (
    id          TEXT PRIMARY KEY,
    code        TEXT NOT NULL REFERENCES session(code),
    name        TEXT NOT NULL,
    joined_at   INTEGER NOT NULL
);

-- The replay source. One row per accepted command, in the order the actor
-- applied them. `seq` is the engine's own counter, not a timestamp.
CREATE TABLE IF NOT EXISTS command_log (
    code        TEXT NOT NULL REFERENCES session(code),
    seq         INTEGER NOT NULL,
    player_id   TEXT,
    payload     TEXT NOT NULL,
    ts          INTEGER NOT NULL,
    PRIMARY KEY (code, seq)
);

-- A projection, for CSV export and for the post-game debrief.
CREATE TABLE IF NOT EXISTS trade (
    id          TEXT PRIMARY KEY,
    code        TEXT NOT NULL REFERENCES session(code),
    seq         INTEGER NOT NULL,
    price       REAL NOT NULL,
    buyer_id    TEXT NOT NULL,
    seller_id   TEXT NOT NULL,
    aggressor   TEXT NOT NULL,
    self_trade  INTEGER NOT NULL,
    ts          INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS trade_by_session ON trade(code, seq);
"#;

pub fn open(path: &str) -> rusqlite::Result<Connection> {
    let conn = Connection::open(path)?;
    conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL;")?;
    conn.execute_batch(SCHEMA)?;
    Ok(conn)
}
