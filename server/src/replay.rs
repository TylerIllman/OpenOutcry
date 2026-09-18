//! Rebuild a session by feeding its command log back through the engine.
//!
//! This is the reason the engine is pure. Because `Market::apply` does no I/O
//! and never reads the clock, replaying the same commands in the same order
//! must produce an identical market — same book, same queue positions, same
//! order ids, same positions and cash.
//!
//! If this ever disagrees with the live session, something has snuck
//! non-determinism into the engine.

use engine::{Command, Market, PlayerId, Price};
use rusqlite::Connection;

use crate::protocol::ClientCommand;
use crate::translate::to_engine_command;

pub fn replay(conn: &Connection, code: &str) -> rusqlite::Result<Market> {
    let (tick, limit): (Option<f64>, i64) = conn.query_row(
        "SELECT tick_size, position_limit FROM session WHERE code = ?1",
        [code],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;

    let mut market = Market::new(engine::Config {
        tick: tick.map(Price::from_f64),
        position_limit: limit,
    });

    // Players first. `AddPlayer` only registers a position, so the order it
    // happens in cannot affect the book.
    let mut stmt = conn.prepare("SELECT id FROM player WHERE code = ?1 ORDER BY joined_at, id")?;
    let ids = stmt
        .query_map([code], |r| r.get::<_, String>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    for id in ids {
        let _ = market.apply(Command::AddPlayer {
            player: PlayerId(id),
        });
    }

    // Then every accepted command, in the order the actor applied them.
    let mut stmt =
        conn.prepare("SELECT player_id, payload FROM command_log WHERE code = ?1 ORDER BY seq")?;
    let rows = stmt
        .query_map([code], |r| {
            Ok((r.get::<_, Option<String>>(0)?, r.get::<_, String>(1)?))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;

    for (player_id, payload) in rows {
        let Ok(cmd) = serde_json::from_str::<ClientCommand>(&payload) else {
            continue;
        };
        if let Some(engine_cmd) = to_engine_command(player_id.as_deref(), &cmd) {
            // Rejections were never logged, so anything here should apply. If
            // one does not, the log and the engine have diverged.
            let _ = market.apply(engine_cmd);
        }
    }

    Ok(market)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;
    use crate::protocol::{Side as WireSide, *};

    /// Write a session, its players and a command log into a fresh in-memory
    /// database, then check that replaying it reproduces the market exactly.
    #[test]
    fn replaying_a_logged_session_reproduces_it() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(db::SCHEMA).unwrap();

        db::insert_session(&conn, "TEST01", "q", "u", None, 10, 0).unwrap();
        db::insert_player(&conn, "TEST01", "alice", "Alice", 1).unwrap();
        db::insert_player(&conn, "TEST01", "bob", "Bob", 2).unwrap();

        // A session with resting orders, a cancel, a crossing order and a take —
        // enough that any ordering mistake in replay would show up.
        let script: Vec<(Option<&str>, ClientCommand)> = vec![
            (None, ClientCommand::OpenTrading),
            (
                Some("bob"),
                ClientCommand::PlaceOrder {
                    side: WireSide::Offer,
                    price: 47.0,
                },
            ),
            (
                Some("bob"),
                ClientCommand::PlaceOrder {
                    side: WireSide::Offer,
                    price: 47.0,
                },
            ),
            (
                Some("alice"),
                ClientCommand::PlaceOrder {
                    side: WireSide::Bid,
                    price: 45.0,
                },
            ),
            (
                Some("alice"),
                ClientCommand::CancelOrder {
                    order_id: "3".into(),
                },
            ),
            (
                Some("alice"),
                ClientCommand::PlaceOrder {
                    side: WireSide::Bid,
                    price: 50.0,
                },
            ),
            (
                Some("alice"),
                ClientCommand::Take {
                    direction: Direction::Buy,
                },
            ),
            (
                Some("bob"),
                ClientCommand::PlaceOrder {
                    side: WireSide::Bid,
                    price: 44.0,
                },
            ),
        ];

        // Build the "live" market directly, and log the same commands.
        let mut live = Market::new(engine::Config {
            tick: None,
            position_limit: 10,
        });
        for id in ["alice", "bob"] {
            live.apply(Command::AddPlayer {
                player: PlayerId(id.into()),
            })
            .unwrap();
        }
        for (seq, (who, cmd)) in script.iter().enumerate() {
            let engine_cmd = to_engine_command(*who, cmd).unwrap();
            // Only accepted commands are logged, exactly as the actor does it.
            if live.apply(engine_cmd).is_ok() {
                db::append_command(
                    &conn,
                    "TEST01",
                    seq as u64 + 1,
                    *who,
                    &serde_json::to_string(cmd).unwrap(),
                    0,
                )
                .unwrap();
            }
        }

        let replayed = replay(&conn, "TEST01").unwrap();

        assert_eq!(
            live.fingerprint(),
            replayed.fingerprint(),
            "replaying the log must reproduce the market"
        );
        assert_eq!(live.seq, replayed.seq);
    }
}
