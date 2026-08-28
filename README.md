# Open Outcry

A trading floor for a room full of people. The host projects an order book on a
screen, everyone else joins on their phone by scanning a QR code, and the room
trades a contract on a question nobody knows the answer to — *how many bouncy
balls are there in the world?* — by shouting at each other and pressing **MINE**
and **YOURS**. At the end the host types in the real answer and everyone finds
out how they did.

No accounts. No sign-up. One URL and a six-character code.

```bash
cd web && npm install && npm run build && cd ..
cargo run -p server          # http://localhost:8080
```

---

## The rules

- **One question per session.** The session *is* the market.
- **Every order is one lot.** No sizes, so a trade always fully consumes exactly
  one resting order and there are no partial fills anywhere in the system.
- **Full limit order book**, price-then-time priority.
- **MINE** lifts the best offer, **YOURS** hits the best bid.
- A **crossing limit order** trades at the *resting* order's price. Bid 60 into
  an offer at 47 and you pay 47.
- **Position limits** count resting orders, not just filled position, so you can
  never breach the limit even if everything fills at once.
- **Self-trades are allowed** — and tagged, so they don't look like a bug.
- The **host doesn't trade**. They type the settlement value at the end, so
  giving them a position would be an open goal.

`P&L = Σ(sells) − Σ(buys) + position × true value`

## How it's built

A single Rust binary serves the React bundle, the REST endpoints and the
WebSocket on one origin. One deploy, no CORS, no cross-origin socket URL.

```
engine/    the matching engine — no I/O, no async, no clock
server/    axum, one actor per session, SQLite
web/       React + TypeScript
```

**One actor per session.** Each session owns its state on a single task, reached
only through a channel. Nothing else touches a `Market`, so there is no lock to
contend on and no way for two commands to interleave inside the engine.

**Deltas, not snapshots.** The server publishes sequenced events — `orderAdded`,
`trade`, `orderCancelled` — and clients apply them locally. A gap in the sequence
triggers a resync. New joiners and reconnects both get a full snapshot first, so
there is only one code path.

**Types are generated, not written twice.** `ts-rs` derives the TypeScript from
the Rust structs during `cargo test`. If the two drift, the build breaks instead
of the game.

## The engine

`Market::apply(Command) -> Result<Vec<Event>, Reject>`. That is the whole
surface. It does no I/O, never reads the clock, and its ordering comes from a
counter rather than a timestamp — so the same commands always produce the same
market.

Prices are scaled `i64` millionths, not floats. Exact P&L, exact tick checks, and
they can key a `BTreeMap`, which `f64` cannot because it isn't `Ord`.

The book is `BTreeMap<Price, VecDeque<Order>>` on each side: the map keeps prices
sorted so the best is the first or last key, and the queue at each price is
arrival order. Price-then-time priority falls out of the structure rather than
being something the code has to remember.

### Replay

Every accepted command is written to SQLite. Feeding that log back through the
engine reproduces the session exactly — same book, same queue positions, same
order ids, same cash.

The server will check this against itself while a game is running:

```
GET /api/sessions/:code/verify?hostToken=...
  -> { "matches": true, "live": "...", "replayed": "..." }
```

If those ever disagree, non-determinism has got into the engine.

## Tests

```bash
cargo test --workspace     # 43 tests
```

- **20 rule tests** covering each rule above, including the awkward ones —
  priority preserved across a mid-queue cancel, a self-trade leaving position
  and cash untouched, the market being zero-sum.
- **4 property tests** throwing 1,600 random command sequences at the engine and
  asserting the invariants after *every* command: the book is never crossed, no
  empty price levels are left behind, queues stay in sequence order, order ids
  are unique, cash and positions net to zero, nobody exceeds their limit, and the
  cached working-order counters agree with walking the book.
- **A replay test** asserting a logged session reproduces itself.

## Benchmarks

```bash
cargo bench -p engine
```

Each figure is a **pair** of commands against a book of the given depth, on
Apple Silicon, release build:

| | depth 10 | depth 1,000 | depth 10,000 |
|---|---|---|---|
| place + take | 460 ns | 515 ns | 545 ns |
| place + cancel | 359 ns | 347 ns | 369 ns |

Two things worth saying about how these were produced, because both were
mistakes first.

**The benchmarks were wrong before they were right.** The obvious shape — build a
book in `iter_batched`, time one command against it — puts the destructor of a
20,000-order book inside the measured region. The numbers scale linearly with
depth and look like a `BTreeMap` that isn't logarithmic. Every benchmark here now
works against a book built once and performs a pair of operations that leaves it
in the same shape, so nothing is allocated or freed while the clock is running.

**Then they found a real bug.** Even after that fix, `place + take` still scaled
linearly: 460 ns at depth 10 but **115 µs at depth 10,000**. The risk check was
counting a player's resting orders by walking the entire book, on every single
order. Caching those counts on `Position` made it flat — a 210× improvement at
depth 10,000. `Book::working()` still does the slow walk, and the property tests
assert the cached counters agree with it.

## Reading further

- [SPEC.md](SPEC.md) — what the thing is
- [DECISIONS.md](DECISIONS.md) — every design decision, what was rejected, and
  the two that were reversed
- [BACKEND.md](BACKEND.md) — endpoints, layout, what's wired to what
- [GUIDE.md](GUIDE.md) — a staged walkthrough for building the engine yourself
  from the `pre-backend` tag
