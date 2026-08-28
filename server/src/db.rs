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

pub fn insert_session(
    conn: &Connection,
    code: &str,
    question: &str,
    unit: &str,
    tick_size: Option<f64>,
    position_limit: i64,
    created_at: i64,
) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT OR REPLACE INTO session
           (code, question, unit, tick_size, position_limit, phase, created_at, true_value)
         VALUES (?1, ?2, ?3, ?4, ?5, 'lobby', ?6, NULL)",
        rusqlite::params![code, question, unit, tick_size, position_limit, created_at],
    )?;
    Ok(())
}

pub fn set_phase(conn: &Connection, code: &str, phase: &str) -> rusqlite::Result<()> {
    conn.execute("UPDATE session SET phase = ?2 WHERE code = ?1", rusqlite::params![code, phase])?;
    Ok(())
}

pub fn set_settlement(conn: &Connection, code: &str, true_value: f64) -> rusqlite::Result<()> {
    conn.execute(
        "UPDATE session SET true_value = ?2, phase = 'settled' WHERE code = ?1",
        rusqlite::params![code, true_value],
    )?;
    Ok(())
}

pub fn insert_player(
    conn: &Connection,
    code: &str,
    id: &str,
    name: &str,
    joined_at: i64,
) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT OR REPLACE INTO player (id, code, name, joined_at) VALUES (?1, ?2, ?3, ?4)",
        rusqlite::params![id, code, name, joined_at],
    )?;
    Ok(())
}

/// Append an accepted command. This is the replay source: feeding these back
/// through the engine in seq order must reproduce the session exactly.
pub fn append_command(
    conn: &Connection,
    code: &str,
    seq: u64,
    player_id: Option<&str>,
    payload: &str,
    ts: i64,
) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT OR REPLACE INTO command_log (code, seq, player_id, payload, ts)
         VALUES (?1, ?2, ?3, ?4, ?5)",
        rusqlite::params![code, seq as i64, player_id, payload, ts],
    )?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub fn insert_trade(
    conn: &Connection,
    code: &str,
    id: &str,
    seq: u64,
    price: f64,
    buyer_id: &str,
    seller_id: &str,
    aggressor: &str,
    self_trade: bool,
    ts: i64,
) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT OR REPLACE INTO trade
           (id, code, seq, price, buyer_id, seller_id, aggressor, self_trade, ts)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        rusqlite::params![id, code, seq as i64, price, buyer_id, seller_id, aggressor, self_trade, ts],
    )?;
    Ok(())
}
