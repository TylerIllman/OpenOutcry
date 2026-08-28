# Backend guide

What exists, what's stubbed, and exactly which front-end call hits which endpoint.

## Layout

```
Cargo.toml              workspace: engine + server
engine/                 Pure matching engine. No I/O, no async, no clock.
  src/lib.rs            types, Book, Market::apply
  src/ids.rs            OrderId / PlayerId newtypes
  tests/rules.rs        20 rule tests. This is the spec.
  tests/properties.rs   invariants, over 1,600 random command sequences
  benches/matching.rs   criterion benchmarks
server/                 Plumbing around the engine.
  src/main.rs           axum router, static file serving, PORT/STATIC_DIR env
  src/protocol.rs       wire types, mirrors web/src/protocol.ts
  src/state.rs          session registry + the per-session actor
  src/http.rs           the four REST endpoints
  src/ws.rs             websocket upgrade, auth, fanout
  src/db.rs             SQLite schema and writes
  src/replay.rs         rebuild a market from the command log
  src/translate.rs      wire commands -> engine commands, shared with replay
web/                    front end, built to web/dist and served by the binary
  src/generated/        TypeScript GENERATED from the Rust by ts-rs — do not edit
  src/protocol.ts       re-exports ./generated plus the helpers
  src/api.ts            the four REST calls
  src/useSession.ts     socket + reducer that applies sequenced events
  src/routes/           Landing, CreateSession, Join, HostSession, PlayerSession
  src/components/       OrderBook, TradeTape, JoinPanel, Bits
```

## Running it

```bash
cd web && npm install && npm run build && cd ..
cargo run -p server          # http://localhost:8080
```

`STATIC_DIR` (default `web/dist`) and `PORT` (default `8080`) are the only knobs.
For front-end work, `cd web && npm run dev` proxies `/api` and `/ws` to 8080.

## HTTP endpoints

| Method | Path | Called from | Purpose |
|---|---|---|---|
| `POST` | `/api/sessions` | `CreateSession.tsx` via `api.createSession` | Create a market. Returns `{ code, hostToken }`. Host token goes straight into localStorage. |
| `GET` | `/api/sessions/:code` | `Join.tsx` via `api.getSession` | Public metadata so the join screen can show the question before you commit a name. |
| `POST` | `/api/sessions/:code/join` | `Join.tsx` via `api.joinSession` | Claim a seat. Returns `{ playerId, playerToken }`. |
| `GET` | `/api/sessions/:code/export.csv?hostToken=` | `HostSession.tsx` via `api.exportUrl` | The tape as CSV, read from SQLite. |
| `GET` | `/api/sessions/:code/verify?hostToken=` | not called by the UI | Replays the command log and compares it against live state. |

Everything after connect happens on the socket. That is deliberate: the host
already holds one, and routing commands through a single ordered channel is what
makes sequence numbers meaningful.

## WebSocket

```
GET /ws?code=ABC123&hostToken=...     -> host connection  (you: null)
GET /ws?code=ABC123&playerToken=...   -> player connection (you: {...})
```

A bad or missing token is refused, not silently downgraded to a spectator.
Every connection opens with a `snapshot`, so a first join and a reconnect take
exactly the same code path.

**Client -> server** (`ClientCommand`): `placeOrder`, `cancelOrder`, `take`,
`resync`, and host-only `openTrading`, `closeTrading`, `settle`.

**Server -> client** (`ServerEvent`): `snapshot`, `playerJoined`, `phaseChanged`,
`orderAdded`, `orderCancelled`, `trade`, `settled` — all carrying a monotonic
`seq` and broadcast to the room — plus `rejected` and `error`, which are
addressed to one connection and carry no seq.

The client tracks `seq` and sends `resync` on a gap. `ws.rs` treats a broadcast
`Lagged` as exactly that case and lets the client notice.

## What works today

The whole game. Verified end to end against the running server, not just
compiled:

- Create a session, join by QR, roster updating live over the socket
- Resting orders, with names on both sides of the book
- **MINE / YOURS** taking the best price on the far side
- **Crossing limit orders** trading at the resting price — a bid of 60 into an
  offer at 47 prints at 47, and never rests
- **Price-then-time priority**, including after a mid-queue cancel
- **Cancel**, with ownership and unknown-order checks
- **Position limit** counting working orders, freeing headroom when you pull
- **Self-trades**, allowed and tagged `self` on the tape
- **Tick and price validation**, rejected back to the one player who tried it
- **Settlement** from real engine positions, leaderboard sorted by P&L, zero-sum
- **CSV export** of the tape
- **SQLite persistence** of session, players, the command log and every trade

Engine tests: `cargo test -p engine` — 20 passing.

## What is left

One thing, and it is a deliberate omission rather than an unfinished edge:

**Sessions do not survive a process restart.** The registry is in memory, so a
redeploy ends every game in progress. Everything needed to fix it exists —
`replay::replay` rebuilds a market from the command log — but restoring sessions
on boot would also mean restoring host and player tokens, and a game whose room
has gone home should not come back. For a party game the honest behaviour is to
lose it; if this ever needs to survive deploys, replay is the way in.

Idle sessions *are* swept: `AppState::evict_idle` runs every five minutes and
drops anything untouched for `SESSION_TTL_MS` (default six hours). Dropping the
handle closes the channel, so the actor task exits on its own.

## Environment

| | |
|---|---|
| `PORT` | default `8080` |
| `STATIC_DIR` | default `web/dist` |
| `DB_PATH` | default `open_outcry.db` |
| `SESSION_TTL_MS` | default six hours |

## Decisions I had to make to write this — confirm or change

These weren't settled during scoping and the code assumes them. All are recorded
in `DECISIONS.md` as #25-28.

- **Crossing limit orders match immediately**, at the resting order's price.
  A bid at 50 into an offer at 47 prints at 47 — the aggressor gets price
  improvement. Otherwise the book could sit crossed, which is nonsense.
- **No partial fills exist.** Every order is one lot, so a trade always fully
  consumes exactly one resting order. This falls out of your no-size decision and
  makes the engine markedly simpler than a real one.
- **The position limit counts working orders**, not just filled position:
  `net + working_bids <= limit` and `-net + working_offers <= limit`. So you can
  never breach the limit even if every resting order fills at once. The
  alternative — checking only at fill time — means rejecting a trade after the
  fact, which is worse.
- **Self-trades are tagged** `selfTrade: true` and the tape renders a `self`
  chip. You allowed self-trades; this stops them reading as a bug on a projector.
