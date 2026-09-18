# Open Outcry

**A trading floor for a room full of people.**

The host projects an order book on a screen. Everyone else joins on their phone
by scanning a QR code. The room then trades a contract on a question nobody
knows the answer to — *how many bouncy balls are there in the world?* — by
shouting at each other and pressing **MINE** and **YOURS**. At the end the host
types in the real answer and everyone finds out how they did.

No accounts, no sign-up. One URL and a six-character code.

<!-- If you fork this or rename the repo, update the CI badge path below. -->
[![CI](https://github.com/TylerIllman/OpenOutcry/actions/workflows/ci.yml/badge.svg)](https://github.com/TylerIllman/OpenOutcry/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)
![Rust](https://img.shields.io/badge/rust-2024_edition-orange.svg)
![TypeScript](https://img.shields.io/badge/typescript-react-3178c6.svg)

### ▶ [openoutcry.tyleri.dev](https://openoutcry.tyleri.dev)

![The projected board](docs/images/board.png)

---

## What it looks like

The board goes on a screen the whole room can see. Everyone else gets four big
buttons on their phone.

| Your phone, mid-round | The reveal |
|---|---|
| ![Player view](docs/images/phone.png) | ![Player result](docs/images/phone-settled.png) |

At the end, every trade in the round plotted against the real answer:

![Settlement](docs/images/settled.png)

---

## The rules

- **One question per session.** The session *is* the market.
- **Every order is one lot.** No sizes — so a trade always fully consumes
  exactly one resting order, and there are no partial fills anywhere in the
  system.
- **A full limit order book**, price-then-time priority.
- **MINE** lifts the best offer. **YOURS** hits the best bid.
- A **crossing limit order** trades at the *resting* order's price. Bid 60 into
  an offer at 47 and you pay 47.
- **Position limits count resting orders**, not just filled position, so you can
  never breach the limit even if everything fills at once.
- **Self-trades are allowed** — and tagged, so they don't look like a bug.
- The **host doesn't trade.** They type the settlement value at the end, so
  giving them a position would be an open goal.

```
P&L = Σ(sells) − Σ(buys) + position × true value
```

---

## How it's built

One Rust binary serves the React bundle, the REST endpoints and the WebSocket on
a single origin. One deploy, no CORS, no cross-origin socket URL.

```
engine/    the matching engine — no I/O, no async, no clock
server/    axum, one actor per session, SQLite
web/       React + TypeScript
```

**One actor per session.** Each session owns its state on a single task, reached
only through a channel. Nothing else touches a `Market`, so there is no lock to
contend on and no way for two commands to interleave inside the engine. It is
also what makes the sequence numbers meaningful — the actor is the only thing
that increments them.

**Deltas, not snapshots.** The server publishes sequenced events —
`orderAdded`, `trade`, `orderCancelled` — and clients apply them locally. A gap
in the sequence triggers a resync. New joiners and reconnects both get a full
snapshot first, so there is only one code path.

**Types are generated, not written twice.** `ts-rs` derives the TypeScript in
[`web/src/generated/`](web/src/generated) from the Rust structs during
`cargo test`, and CI fails if the committed output is stale. A change to the
wire format breaks the build rather than the game.

## The engine

```rust
Market::apply(Command) -> Result<Vec<Event>, Reject>
```

That is the entire surface. It does no I/O, never reads the clock, and its
ordering comes from a counter rather than a timestamp — so the same commands
always produce the same market.

**Prices are scaled `i64` millionths, not floats.** Exact P&L, exact tick
checks, and they can key a `BTreeMap`, which `f64` cannot because it isn't
`Ord`.

**The book is `BTreeMap<Price, VecDeque<Order>>` on each side.** The map keeps
prices sorted so the best is the first or last key; the queue at each price is
arrival order. Price-then-time priority falls out of the data structure rather
than being something the code has to remember to enforce.

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

---

## Tests

```bash
cargo test --workspace     # 43 tests
```

- **20 rule tests** covering every rule above, including the awkward ones:
  priority preserved across a mid-queue cancel, a self-trade leaving position
  and cash untouched, the market being zero-sum.
- **4 property tests** throwing 1,600 random command sequences at the engine and
  asserting the invariants after *every single command* — the book is never
  crossed, no empty price levels are left behind, queues stay in sequence order,
  order ids are unique, cash and positions net to zero, nobody exceeds their
  limit, and the cached working-order counters agree with walking the book.
- **A replay test** asserting a logged session reproduces itself byte for byte.

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

Both of these were wrong before they were right, which is the more useful part.

**The benchmarks themselves were wrong first.** The obvious shape — build a book
in `iter_batched`, time one command against it — puts the destructor of a
20,000-order book inside the measured region. The numbers scale linearly with
depth and look like a `BTreeMap` that isn't logarithmic. Every benchmark here
now works against a book built once and performs a pair of operations that
leaves it in the same shape, so nothing is allocated or freed while the clock is
running.

**Then they found a real bug.** Even after that fix, `place + take` still scaled
linearly: 460 ns at depth 10 but **115 µs at depth 10,000**. The position-limit
check was counting a player's resting orders by walking the entire book, on
every single order. Caching those counts on `Position` made it flat — a 210×
improvement at depth 10,000. `Book::working()` still does the slow walk, and the
property tests assert the cached counters agree with it.

---

## Bots

The host can add bots so a small room still has someone to trade against. They
are deliberately stupid:

- **They never quote.** They only lift offers and hit bids that humans have
  made. The players make the market; the bots are the customers.
- **They have no view on price.** No fair value, no opinion, no cleverness —
  they buy and sell at random, at a rate the host sets. Giving them a view would
  quietly make them good at the thing the players are supposed to be competing
  at, and the game would become about guessing the bots' anchor.

The controls sit behind a gear icon, because the host screen is projected and
the room should not see that the flow is synthetic.

Bots are ordinary players to the engine — they hold positions and respect the
limit — but they are **left off the leaderboard**, which means the displayed P&L
no longer sums to zero. The bots are holding the other side of it.

**Their randomness never reaches the command log.** A bot acts by issuing an
ordinary `take` under its own player id, through the same path a human's command
takes, so what gets logged is the decision rather than the seed. A session full
of bots still replays exactly.

---

## Running it

**One process, the way production runs.** The Rust binary serves the built React
bundle and the socket together.

```bash
cd web && npm install && npm run build && cd ..
cargo run -p server
```

Then open <http://localhost:8080>.

**Front-end work, with hot reload.** Two terminals — Vite proxies `/api` and
`/ws` through to the server on 8080.

```bash
cargo run -p server
```
```bash
cd web && npm run dev
```

**Playing on real phones while it runs on your laptop.** The server binds
`0.0.0.0`, so open it on your machine's LAN address rather than `localhost` —
e.g. `http://192.168.1.14:8080`. The QR code encodes whatever origin the host
page was loaded from, so phones on the same Wi-Fi scan straight into the right
place. Some guest networks block device-to-device traffic; if the QR resolves
but nothing loads, that is why.

| | |
|---|---|
| `PORT` | default `8080` |
| `STATIC_DIR` | default `web/dist` |
| `DB_PATH` | default `open_outcry.db` |
| `SESSION_TTL_MS` | idle session sweep, default six hours |

## Deploying

One Fly.io app. The `Dockerfile` builds the front end and the server into a
single image — a 4.5 MB stripped binary on a slim Debian base.

```bash
fly apps create your-app-name      # must match `app` in fly.toml
fly volumes create open_outcry_data --size 1 --region syd
fly deploy
```

Set `primary_region` to whichever Fly region is closest to the room you're
playing in, and create the volume in that same region — a volume elsewhere
cannot be mounted. This is not a nicety: the whole game is people racing each
other to hit the same bid, so a server on the wrong continent adds a couple of
hundred milliseconds to every order and makes MINE feel broken.

**This app must run as exactly one machine.** All session state is in memory:

- `auto_stop_machines = 'off'` and `min_machines_running = 1` are already set in
  `fly.toml`. A machine that stops to save money takes every game in progress
  with it.
- Never `fly scale count 2`. A second machine holds a second, separate set of
  sessions, and half the room would join a market the other half cannot see.

A deploy restarts the machine and ends any game in progress, so deploy between
rounds rather than during one.

---

## Reading further

| | |
|---|---|
| [docs/SPEC.md](docs/SPEC.md) | what the thing is |
| [docs/DECISIONS.md](docs/DECISIONS.md) | all 29 design decisions, what was rejected and why, and the three that were reversed |
| [docs/BACKEND.md](docs/BACKEND.md) | endpoints, layout, what is wired to what |
| [docs/GUIDE.md](docs/GUIDE.md) | a staged walkthrough for building the engine yourself from the `pre-backend` tag |

`DECISIONS.md` is the one worth reading. It records the options that were
rejected, the reasoning at the time, and the decisions that turned out to be
wrong — including two separate attempts at clever bots that both had to be torn
out.

## Licence

[MIT](LICENSE)
