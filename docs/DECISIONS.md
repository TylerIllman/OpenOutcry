# Decision log

Every decision taken on this project, in the order it was taken, with the options
that were rejected and why.

Three of them were reversed after the fact, and those are recorded rather than
tidied away — where it runs (#7), what a player sees mid-round (#20), and how
the bots behave (#29). The rejected options are the useful part: most of them
were reasonable, and a couple were built before being torn out.

Conclusions live in [SPEC.md](SPEC.md). This file is the reasoning behind them.

---

### 1. Core concept
**In-person open-outcry game.** Phones plus one projected host screen. The shouting
happens in the room; the app is the book and the tape.
*Rejected:* remote players; anything involving real audio.

### 2. What is traded
**An estimation question with a hidden answer** — "how many bouncy balls in the world" —
settling to a true value.
*Rejected:* a stock with a live price and no settlement (never ends, no right answer);
several contracts trading at once (unreadable on one projector); a dice/cards hidden-
information game (a different game — asymmetry rather than estimation).

### 3. Book model
**Full limit order book, price-time priority.**
*Rejected:* top-of-book only, one quote per player (no depth, filled quotes just vanish);
forced two-sided quoting (stricter market-making drill, less free-form).

### 4. Order size
**None. Every order and trade is one lot.**
*Rejected:* a per-player "my size" setting; a size typed per order (two inputs per action
is too slow on a phone when the room is moving).

### 5. Position limit
**Host-set.** Breaching orders rejected with a visible message.
*Rejected:* no limits (one fast player takes the whole book and it becomes a reflex
game); capping resting orders instead of position.

### 6. Round flow
**One question per session.** Play again = new session.
*Rejected:* multi-round session with a cumulative leaderboard; a continuous market the
host settles ad hoc.

### 7. Where it runs — **REVERSED**
Originally: local-first, host runs it on their laptop, phones join over LAN.
**Revised to: hosted only, a public website.** "Open a website" is far lower friction for
the host, and phones have mobile data if the venue Wi-Fi is bad.
*Rejected on revisit:* keeping a local fallback (two deployment paths to maintain);
reverting to local-only.

**Consequence discovered:** Vercel cannot hold a WebSocket open — its functions are
serverless. The real-time backend has to live on a persistent process regardless of
language. This shaped decisions 15 and 24.

### 8. Joining
**Open join by code/QR plus a name. Late joins allowed.**
*Rejected:* locking the lobby when trading opens (fairer P&L, but unforgiving in a pub);
host approving each joiner (friction at exactly the wrong moment).
Accepted cost: a late joiner trades with more information, and their P&L is not strictly
comparable.

### 9. Host screen
**Book + tape + last, with positions and P&L hidden until settle.**
*Rejected:* public live positions and mark-to-market P&L (turns an estimation game into a
squeeze game); book and tape only, no names (loses the social thread).

### 10. A player's own orders
**Multiple resting orders per side, with a cancel list.**
*Rejected:* one live bid and one live offer, new replaces old. That would have kept the
phone to exactly four controls, but rules out building a ladder.

### 11. Persistence
**SQLite on the server, plus CSV export at settlement.** Survives a crash mid-round.
*Rejected:* in-memory with an export at the end (a crash loses the round);
a post-game replay/scrub UI — deferred, not refused. The event log supports it later.

### 12. Settlement timing
**Host enters the true value at the end.** Allows questions whose answer isn't knowable
until later — "how many pints does the group drink tonight".
*Rejected:* committing the answer at session creation; making it the host's choice.
This is what forced decision 22.

### 13. Prices
**Floats, with a host-defined unit label.** Host may optionally set a tick size.
*Rejected:* integers only; unconstrained decimals with no tick (someone quotes 45.001 to
jump the queue and the projector fills with near-identical levels).
*Implementation:* stored as scaled `i64` micro-units internally — exact P&L, trivial
`BTreeMap` ordering, and it sidesteps `f64` not being `Ord`.

### 14. Anonymity
**Names on the book and on the tape.** This is what open outcry is — you know who's
shouting, and you can lean on someone who keeps improving their bid.
*Rejected:* anonymous book with names on the tape; anonymous everywhere.

### 15. Backend language
**Rust**, chosen deliberately as a vehicle for learning it, and because it reads well to
a trading firm.
*Rejected:* Python (fastest to ship, but a Python matching engine is a weak artefact for
this specific pitch); C++ (most authentic, worst fit for a web-backed side project,
and mediocre C++ signals worse than good Rust); Go; prototyping in Python and rewriting.

Noted at the time: **language is a distant fourth** in what actually signals. Ahead of it,
in order — a deterministic event-sourced engine with replay; property-based tests over
the nasty cases; benchmarks with real numbers.

### 16. Front end
**TypeScript / React**, built with Vite. Never in question.

### 17. Wire protocol
**Incremental events with monotonic sequence numbers**; full snapshot on join, and on a
detected gap. This is how real exchanges publish market data, and the event log doubles
as the persistence and replay source.
*Rejected:* broadcasting the full book on every change (fine at this scale, but the one
thing here a trading firm would raise an eyebrow at); deltas plus a periodic snapshot.

### 18. Disconnects
**Resting orders stay live.** Your book is your responsibility.
*Rejected:* cancel-on-disconnect — a real exchange feature that reads well, but iOS
suspends backgrounded tabs, so glancing at a text message would evaporate your book;
a grace period then pull (timers and reconnect edge cases).
Accepted cost: a genuinely dead phone can be picked off by the room.

### 19. Self-match
**Allowed — you can trade against your own resting order.**
*Rejected:* rejecting the aggressing order (standard self-match prevention);
cancelling the resting order and continuing, as CME does.

**Known consequence:** the last price is paintable. It cannot corrupt P&L, which settles
against the true value, but combined with decision 14 the room will watch someone trade
with themselves by name. Open suggestion: tag self-trades on the tape so it doesn't read
as a bug.

### 20. What a player sees — **REVERSED TWICE**
First: nothing at all, track it in your head.
Then: position, trades and a P&L counter.
**Final: position and your own fills, but no P&L counter during trading.**

The middle position collapsed once the question "marked against what?" was put — there is
no true value yet, so any live P&L must mark against the last trade or the book mid, and
decision 19 makes the last trade paintable.
*Rejected:* cash and position shown separately; mark-to-market against last; mark against
the book mid.

### 21. Position limit vs hidden position
Raised as a collision: a rejected order tells a player they are at the cap, leaking the
position that decision 20 was hiding. **Resolved by showing position** — see 20.

### 22. Host role
**Host operates and does not trade.**
Forced by decision 12: a host who trades and types the settlement value afterwards is
choosing the number after seeing their own book.
*Rejected:* host plays with the answer committed up front (rules out later-knowable
questions); host plays on trust (a real hole, and the one thing that would make a trading
firm wince); a second player confirming the answer (another role, and it slows down the
best moment of the game).

### 23. Session access
**Open creation, 6-character codes, idle sessions expire.** Host-token in localStorage
gates open/close/settle. Rate limit on creation.
*Rejected:* 4-character codes (easier to read off a projector, but ~1.6M combinations);
a shared secret to host (keeps it private, but you can't hand the link to a friend).

### 24. Deploy shape
**One Fly.io deploy** — the Rust binary serves the Vite bundle and `/ws` on one domain.
*Rejected:* front end on Vercel with the backend on Fly (two deploys in step, CORS on the
socket upgrade, a WS URL that will be wrong at least once); a hybrid serving assets
differently in dev and prod.
*Constraint:* `auto_stop_machines = false`, exactly one always-on machine. In-memory
state means never zero instances and never two.

### 25. Crossing limit orders match immediately
A bid at or above the best offer trades at the **resting** order's price, so the
aggressor gets the price improvement. Not decided during scoping; the alternative
lets the book sit crossed, which is nonsense.
*Consequence:* because every order is one lot, a crossing order matches exactly
one resting order and never rests.

### 26. No partial fills exist
Falls out of decision 4. One lot per order means a trade always fully consumes
exactly one resting order. Worth stating explicitly because it removes a whole
category of engine complexity — and because an interviewer will ask about it.

### 27. Position limit counts working orders
`net + working_bids <= limit` and `-net + working_offers <= limit`, so a player
can never breach the limit even if every resting order fills at once.
*Rejected:* checking only at fill time, which means rejecting a trade after the
fact — worse for both the engine and the player.

### 28. Self-trades are tagged in the protocol
`Trade.selfTrade` is on the wire and the tape renders a `self` chip. This is the
mitigation raised against decision 19, now taken.

### 29. Bots are dumb flow, not opponents — **REVERSED TWICE**
Bots **only take** — they never quote — and they have **no view on price**. The
host sets a rate and a buy/sell lean; everything else is random.
*Rejected:* market-making bots that quote both sides around an anchor (built
first, then removed — they did the players' job for them); bots with a private
fair value and a logistic willingness curve (built second, then removed — it
made the game about guessing the bots' anchor rather than estimating the answer).
*Also:* bots are kept off the leaderboard, so the displayed P&L no longer sums
to zero. The controls sit behind a gear icon because the host screen is
projected, and the roster and trader count deliberately do not distinguish bots.

---

## Still open

- **Division of labour.** Engine crate only, or the whole Rust backend? Not decided.
- **Self-trade tagging on the tape** — proposed, not yet accepted. See decision 19.
