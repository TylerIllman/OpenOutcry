# Building the engine — a step at a time

This is the order to build things in, assuming Rust is new to you.

> **The engine is now implemented on `main`.** This guide still works — check out
> the `pre-backend` tag and build it yourself, with the finished version as a
> reference you can diff against when you get stuck:
>
> ```bash
> git checkout pre-backend -b my-attempt
> ```

Each step is small enough to finish in one sitting and ends with a test going
green. Don't skip ahead: the tests build on each other, and a later one will
fail for reasons that have nothing to do with what you just wrote.

**The rule for this whole guide: get one test green, then stop and look at it.**
Twenty green tests you rushed teach you nothing. Six you fought for teach you
Rust.

---

## Rust survival kit

Just the bits you need. Come back here when something looks alien.

### `Option<T>` — "maybe there's a value"

There is no `null` in Rust. Something that might be missing is `Option<T>`,
which is either `Some(value)` or `None`. `best_bid()` returns
`Option<&Order>` because the book might be empty.

```rust
match self.book.best_offer() {
    Some(order) => { /* there is one, `order` is it */ }
    None => return Err(Reject::NoLiquidity),
}
```

The `?` shortcut means "if this is `None`, stop and return `None`":

```rust
self.bids.values().next_back()?.front()
```

### `Result<T, E>` — "this might fail"

Either `Ok(value)` or `Err(problem)`. Our `apply` returns
`Result<Vec<Event>, Reject>`: on success a list of what happened, on failure a
reason. Returning an error is just `return Err(Reject::NotOpen);`.

### `match` — the workhorse

Like a switch, but it forces you to handle every case, and it pulls values out
of the thing you're matching on:

```rust
match cmd {
    Command::PlaceOrder { player, side, price } => {
        // player, side and price are now local variables
    }
    Command::SetPhase { phase } => { /* ... */ }
    _ => todo!(),   // `_` catches everything else
}
```

That `_ => todo!()` is how you leave the rest unbuilt while you work on one arm.

### Ownership, and why the compiler shouts at you

Every value has exactly one owner. Passing it somewhere **moves** it, and you
can't use it afterwards. You will hit this within ten minutes:

```rust
queue.push_back(order);                          // `order` moved into the queue
Ok(vec![Event::OrderAdded { order }])            // ERROR: it's gone
```

The fix is `.clone()` — make a copy for the event, move the original into the
book:

```rust
let event = Event::OrderAdded { order: order.clone() };
queue.push_back(order);
Ok(vec![event])
```

Don't feel bad about cloning. A five-field struct is nothing, and the rule the
compiler is enforcing is a real one. Reach for references and lifetimes later,
if ever.

### `&mut self`

`fn apply(&mut self, ...)` means "this can change the market". Inside it,
`self.phase = Phase::Open` works. A plain `&self` would be read-only.

### The `entry` API

Getting a queue at a price, creating an empty one if the price is new:

```rust
self.book.bids.entry(price).or_default().push_back(order);
```

`entry(price)` finds or reserves the slot, `or_default()` gives you an empty
`VecDeque` if nothing was there, `push_back` adds to the end of the queue.

---

## Stage 0 — look before you write

```bash
cargo test -p engine
```

Everything fails with `not yet implemented`. That's `todo!()` in
`Market::apply` panicking. Good — that's the starting line.

Now open two files side by side:

* [engine/src/lib.rs](engine/src/lib.rs) — the types. Read it top to bottom.
  It's about 200 lines and mostly comments. You don't need to understand every
  line, but you should recognise `Command`, `Event`, `Order`, `Book`, `Market`.
* [engine/tests/rules.rs](engine/tests/rules.rs) — twenty tests. This is the
  spec. Every rule the game has is in here.

Read `resting_orders_do_not_cross`. It's six lines. That's your first target.

---

## Stage 1 — make the book hold an order

### Step 1: players and phases

**Green:** `orders_rejected_before_trading_opens`

Replace `todo!()` with a `match cmd { ... }` and build two arms:

* `Command::AddPlayer` — put a `Position::default()` into `self.positions`,
  return `Ok(vec![Event::PlayerAdded { player }])`.
* `Command::SetPhase` — set `self.phase`, return the matching event.

Then a third arm for `Command::PlaceOrder` that, for now, only checks the phase:
if `self.phase` isn't `Phase::Open`, return `Err(Reject::NotOpen)`. Leave the
rest as `todo!()`.

End every other arm with `_ => todo!()`.

**Why this first:** the `market()` helper at the top of the test file calls
`AddPlayer` and `SetPhase` with `.unwrap()`. Nothing else can run until these
work.

**Gotcha:** `.unwrap()` on a `Result` means "give me the value, and panic if it
was an error". When a test panics on a line you didn't expect, look for an
`.unwrap()` above it.

### Step 2: reject nonsense prices

**Green:** `orders_rejected_after_trading_closes`, `non_positive_price_is_rejected`,
`price_off_tick_is_rejected`

Add two guards to `PlaceOrder`, after the phase check:

* `price.0 <= 0` → `Err(Reject::BadPrice)`
* if `self.config.tick` is `Some(t)` and `price.0 % t.0 != 0` → `Err(Reject::BadTick)`

`price.0` is the raw `i64` inside `Price`. This is the payoff for storing
prices as whole millionths instead of `f64`: the tick check is an exact
remainder, with no rounding argument.

**The Rust:** `if let Some(t) = self.config.tick { ... }` runs the block only
when there is a tick size.

### Step 3: actually rest the order

**Green:** `resting_orders_do_not_cross`

Now finish `PlaceOrder`:

1. `self.seq += 1` — do this **before** building the order. The `seq` is the
   order's place in the queue, so it has to be stamped at entry.
2. Build the `Order`, using `self.next_order_id()` for the id.
3. Pick the side: `Side::Bid` goes in `self.book.bids`, `Side::Offer` in
   `self.book.offers`.
4. Push it in with the `entry` pattern from the survival kit.
5. Return `Ok(vec![Event::OrderAdded { .. }])`.

**This is where ownership will bite you.** Re-read the ownership section. Clone
for the event, move the original into the book.

**Stop here and look at what you have.** The book now holds orders, sorted by
price, queued by arrival. That is a real order book. Everything else is rules on
top of it.

---

## Stage 2 — make it trade

### Step 4: MINE and YOURS

**Green:** `take_on_an_empty_book_is_rejected`, `mine_lifts_the_best_offer`

`Command::Take` is a market order.

* `Direction::Buy` (MINE) hits `best_offer()`. `Direction::Sell` (YOURS) hits
  `best_bid()`.
* Nothing there → `Err(Reject::NoLiquidity)`.
* Otherwise: remove that order from the front of its queue, build a `Trade`,
  update both players' `Position`, return `Ok(vec![Event::Traded { .. }])`.

For the position update: the buyer's `net` goes up by 1 and their `cash` goes
**down** by the price. The seller is the mirror. Cash is in millionths, same as
price, so it's `p.cash -= price.0`.

**The big gotcha:** when you pop the last order out of a price level, you're
left with an empty `VecDeque` still sitting in the `BTreeMap`. `best_offer()`
does `.values().next()?.front()` — it finds that empty level, asks for its front
order, gets `None`, and reports the book as empty when it isn't.

**Remove the price level when its queue becomes empty.** This will cost you an
hour if you don't do it now.

### Step 5: crossing limit orders

**Green:** `crossing_bid_trades_at_the_resting_price_not_its_own`,
`a_crossing_order_never_rests_because_every_order_is_one_lot`

Go back to `PlaceOrder`. Before resting the order, check whether it crosses:

* a bid at or above the best offer, or an offer at or below the best bid

If it does, it trades instead of resting — at the **resting order's** price, not
its own. Someone bidding 50 into an offer at 47 pays 47. That's price
improvement, and it's how real venues work.

Because every order is one lot, a crossing order matches exactly one resting
order and is then completely used up. **It never rests.** No partial fills exist
anywhere in this engine — that falls out of the no-size decision and it removes
a whole category of complexity you'd otherwise be dealing with.

**Refactor moment:** Steps 4 and 5 both "consume the best order on one side and
make a trade". Pull that into one private helper — something like
`fn execute(&mut self, taker: &PlayerId, direction: Direction) -> Option<Trade>`
— and call it from both. Doing this now will save you from fixing the same bug
twice later.

### Step 6: self-trades

**Green:** `self_trades_are_allowed_and_tagged`,
`a_self_trade_leaves_position_and_cash_unchanged`

You decided people can trade with themselves. Set `self_trade: true` on the
`Trade` when buyer and seller are the same player.

The second test should already pass if your position maths is right: +1 and -1
net to zero, and paying yourself nets to zero cash. If it doesn't, you're
probably applying the buyer update and then overwriting it with the seller
update instead of applying both.

**The Rust:** you can't hold two `&mut` references into `self.positions` at
once. Read both positions out, do the arithmetic, write both back. The
borrow checker is stopping you from aliasing, which is exactly the bug you'd
otherwise write here.

---

## Stage 3 — the remaining rules

### Step 7: cancelling

**Green:** `you_cannot_cancel_someone_elses_order`,
`cancelling_an_unknown_order_is_rejected`,
`cancelling_mid_queue_preserves_everyone_elses_priority`

Find the order by id, check it belongs to the player, remove it.

The first two are easy. The third is the interesting one: when you pull an order
out of the middle of a queue, everyone behind it must keep their place. Use
`VecDeque::retain` or find the index and `remove` it — do not rebuild the queue
in a different order.

**You'll notice the problem here:** finding an order by id means scanning every
price level on both sides. That's the moment to add an index — a
`HashMap<OrderId, (Side, Price)>` on `Market`, updated whenever an order enters
or leaves the book.

Add it *now that you've felt why it's needed*, not before. Being able to explain
why it exists is worth more than having it there from the start.

### Step 8: the position limit

**Green:** `position_limit_counts_working_orders_not_just_fills`,
`cancelling_frees_up_limit_headroom`

The limit counts orders that *could* fill, not just ones that did:

```
net + working_bids   <= limit
-net + working_offers <= limit
```

`Book::working()` is already written for you. So a player who is flat with two
resting bids and a limit of 2 cannot bid again — if all three filled they'd be
long 3.

Check this **before** resting the order, and return `Err(Reject::PositionLimit)`.

### Step 9: settlement

**Green:** `pnl_is_cash_plus_position_times_true_value`,
`the_whole_market_is_zero_sum`,
`replaying_the_same_commands_gives_the_same_book`

`settle_pnl` is already written. If your cash and position bookkeeping is right,
these three pass without new code — which is exactly why they're last. The
zero-sum test is a genuine check on the whole engine: every lot has a buyer and
a seller, so all the P&L must cancel out. If it doesn't, you have a real bug.

Handle `Command::Settle` by setting the phase and returning `Event::Settled`.

```bash
cargo test -p engine
```

Twenty green. **You have written a matching engine.**

---

## Stage 4 — see it on a screen

The engine works but nothing is plugged in. Everything up to now has been pure
logic with instant tests; this stage is async plumbing, which fails in different
and more annoying ways. That's why it's separate.

### Step 10: wire the actor

In [server/src/state.rs](server/src/state.rs), the arm handling `PlaceOrder |
CancelOrder | Take` currently just returns "Matching engine not implemented
yet". Replace it with:

1. Give `SessionActor` a `market: Market` field.
2. Translate the wire command into an `engine::Command` — mostly renaming, plus
   `Price::from_f64`.
3. Call `self.market.apply(cmd)`.
4. On `Ok(events)`, turn each `engine::Event` into a `ServerEvent`, stamping
   each with a fresh `self.seq += 1`, and broadcast them.
5. On `Err(reject)`, map it onto a `RejectReason` and send it down `env.reply`
   — the private channel, so only that one player sees it.

**Why two sequence counters:** the engine has its own `seq` for queue priority.
The actor has a separate `seq` for the browser's event stream. Don't merge them;
they're answering different questions.

### Step 11: play it

```bash
cd web && npm run build && cd ..
cargo run -p server
```

Open `http://localhost:8080`, host a game, join from your phone on the same
Wi-Fi, and put a bid in. If it appears on the host screen, you're done.

Get someone else to play it with you before doing anything in Stage 5.

---

## Stage 5 — make it a portfolio piece

Only start this once the game is genuinely playable. In rough order of how much
a trading firm will care:

1. **Property tests.** Add `proptest`. Throw thousands of random command
   sequences at the engine and assert the invariants hold every time: the book is
   never crossed, P&L is always zero-sum, priority is never violated. This finds
   bugs fifty hand-written tests won't, and it is the single most trading-firm-
   legible thing you can add.
2. **Persistence and replay.** The schema is already in
   [server/src/db.rs](server/src/db.rs). Append every accepted command, then
   write a test that replays a whole session from the log and asserts the final
   state is identical. Deterministic replay is the thing this design was built
   for — make it visible.
3. **Benchmarks.** Add `criterion`. Measure orders/sec and p50/p99 match latency.
   Put the numbers in the README.
4. **Then swap the data structure and measure again.** You'll have real numbers
   for both, and "I measured before I optimised" is a better answer than any
   structure you picked on day one.
5. **`ts-rs`.** Generate `web/src/protocol.ts` from the Rust types so the two
   can't drift.

---

## When you get stuck

Paste me the compiler error. Rust's errors are long but they nearly always name
the fix — the useful bit is usually the `help:` line near the bottom, not the
first line.

Two things worth saying out loud:

* **A borrow checker error is not you being bad at this.** It is the compiler
  pointing at a real aliasing problem. Read what it says you're doing twice.
* **`todo!()` is a tool, not a placeholder you forgot.** Leaving whole arms
  unbuilt while you get one right is the correct way to work through this.
