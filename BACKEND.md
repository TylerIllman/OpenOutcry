# Backend guide

What exists, what's stubbed, and exactly which front-end call hits which endpoint.

## Layout

```
Cargo.toml              workspace: engine + server
engine/                 YOURS. Pure matching engine, no I/O, no async, no clock.
  src/lib.rs            types, Book, Market::apply  <- the seam, currently todo!()
  src/ids.rs            OrderId / PlayerId newtypes
  tests/rules.rs        20 failing tests. This is the spec. Make them green.
server/                 MINE. Plumbing around the engine.
  src/main.rs           axum router, static file serving, PORT/STATIC_DIR env
  src/protocol.rs       wire types, mirrors web/src/protocol.ts
  src/state.rs          session registry + the per-session actor
  src/http.rs           the four REST endpoints
  src/ws.rs             websocket upgrade, auth, fanout
  src/db.rs             SQLite schema (written, not yet wired)
web/                    front end, built to web/dist and served by the binary
  src/protocol.ts       wire types, mirrors server/src/protocol.rs
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
| `GET` | `/api/sessions/:code/export.csv?hostToken=` | `HostSession.tsx` via `api.exportUrl` | Tape + final book. **Stubbed — returns headers only.** |

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

Verified end to end against the running server:

- Create session, join, QR, roster updating live over the socket
- `openTrading` / `closeTrading` / `settle` and the phase transitions
- Settlement computes results and the leaderboard renders
- A player is refused host commands (`not_host`)
- The host is refused a trade (`not_host`, "The host does not trade")
- Orders come back `rejected` with "Matching engine not implemented yet"

## What is stubbed

1. **`Market::apply` is `todo!()`.** Nothing matches. This is the seam.
2. **The actor does not call the engine.** `state.rs` `handle()` has the
   `PlaceOrder | CancelOrder | Take` arm returning a rejection. Wiring it up
   means: translate the wire command into an `engine::Command`, call
   `market.apply`, map `Vec<engine::Event>` onto `ServerEvent`s with fresh
   sequence numbers, broadcast. `Reject` maps 1:1 onto `RejectReason`.
3. **Settlement recomputes from the tape** rather than reading the engine's
   position and cash. Once the engine exists, use `Market::settle_pnl`.
4. **SQLite is not wired.** `db.rs` has the schema. The actor should append to
   `command_log` before broadcasting, so a crash loses the broadcast but never
   the command.
5. **CSV export returns a header row only.**
6. **No session eviction.** Abandoned sessions leak. Needs an idle sweep.
7. **No rate limit** on session creation.
8. **`ts-rs` is not set up.** `protocol.rs` and `protocol.ts` are hand-synced,
   which will drift. Add `#[derive(TS)]` and generate the TS in `cargo test`.

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
