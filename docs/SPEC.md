# Open Outcry — Scope

A hosted website where a group in one room plays an open-outcry estimation market.
The host opens a session on a projector, players join on phones by QR, shout at each
other, and trade a contract on a question with a hidden answer. No accounts, ever.

Agreed 2026-08-28, before any code existed. Built since, and still
accurate — the one addition is bots, described in the README.

---

## The market

- **One question per session.** e.g. "how many bouncy balls in the world", with a
  host-defined **unit label**. The session *is* the market:
  `lobby → open → closed → settled`.
- **Full limit order book**, price-time priority.
- **Prices are floats**; the host may optionally set a **tick size** (blank = any price).
- **No size.** Every order and every trade is one lot. Position is an integer.
- **MINE** lifts the best offer. **YOURS** hits the best bid. Plus bid and offer inputs.
- Players may rest **multiple orders per side**, with a cancel list.
- **Self-trades are allowed** — you can trade against your own resting order.
- **Host-set position limit.** Breaching orders are rejected with a visible message.
- Host enters the **true value at the end**, then settles.

```
P&L = Σ(sell prices) − Σ(buy prices) + (final position × true value)
```

- The **host operates and does not trade.** This is what keeps an end-entered
  settlement value honest.

## Screens

**Projector (host)**
- Order book with **names on each order**
- Scrolling trade tape with names — "Tyler bought from Sam @ 47"
- Last price, high/low
- **No positions or P&L until settle**, then the reveal and the leaderboard

**Phone (player)**
- Book at top
- MINE / YOURS / bid / offer
- Your resting orders, with per-order cancel
- Your fills and your position (with average price)
- **No P&L counter during trading**

## Sessions and identity

- Open join, **6-character codes**, **late joins allowed**
- Name only, no accounts
- `localStorage` token restores your seat after a screen-lock or refresh
- Host holds a separate host-token, gating open/close/settle
- **Orders survive disconnects** — your book is your responsibility
- Idle sessions evict from memory, persist in SQLite
- Rate limit on session creation

## Architecture

- **Rust** backend, **TypeScript / React** front end
- **One Fly.io deploy** — the binary serves the Vite bundle and `/ws` on one domain
  (`tower-http` `ServeDir` with SPA fallback to `index.html`)
- `fly.toml`: `auto_stop_machines = false`, `min_machines_running = 1`.
  In-memory state means exactly one instance — never zero, never two.
- axum + `tokio::sync::broadcast`; **one session actor per session** behind a channel,
  so no `Arc<Mutex<_>>` contention
- Wire protocol: **incremental events with monotonic sequence numbers**; full snapshot
  on join or on a detected sequence gap
- **`ts-rs`** generates the TS types from the Rust types, so drift fails CI rather than
  showing up as a blank order book in the pub
- **`rusqlite`** — every order, cancel, trade and settlement appended. The event log
  doubles as the replay source.

### The engine

A **pure crate**: `apply(&mut self, cmd) -> Vec<Event>`. No I/O, no clock, no async.

This is the portfolio piece:
- `proptest` for invariants (price-time priority preserved across cancels, etc.)
- `criterion` for benchmarks — orders/sec, p50/p99 match latency in the README
- Deterministic replay from the event log

### Implementation notes

- Prices stored as **scaled `i64` micro-units** internally, so P&L is exact and
  `BTreeMap` ordering is trivial. Floats only at the UI edge.
- ~20 players per session is the design target; nothing should break at 100.
- CSV export of the tape and the final book at settlement, for host and players.

## Why it's built this way

It is a **portfolio project aimed at trading firms**, and a deliberate excuse to learn
Rust. That drives several choices that would otherwise be over-engineering:

1. Deterministic, event-sourced engine with replay — the most legible signal in the project
2. Property-based tests over the nasty cases, not 50 hand-written ones
3. Benchmarks with real numbers
4. Rust itself — a distant fourth in terms of signal, but it does signal

Vercel was ruled out for the backend: it cannot hold a WebSocket open.

## Open questions

- **Division of labour.** Is Tyler taking just the `engine` crate, or the whole Rust
  backend? Not yet decided.
- **Self-trades + names on the book** is an odd pairing. Anyone can paint the last price,
  and the room can see whose order they painted against. It won't corrupt P&L (settlement
  is against the true value) but it will look like a bug the first time it happens.
  Suggested minimum: a `self-trade` tag on the tape.
