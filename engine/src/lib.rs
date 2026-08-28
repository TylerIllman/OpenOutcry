//! The matching engine.
//!
//! This crate is deliberately pure: no I/O, no clock, no async, no networking.
//! Everything it does is `apply(&mut self, Command) -> Vec<Event>`. That is what
//! makes the whole session replayable — feed the persisted command log back in
//! and you must get byte-identical state.
//!
//! Design notes that follow from SPEC.md:
//!
//!  * **Every order is one lot.** A trade therefore always fully consumes exactly
//!    one resting order. There are no partial fills anywhere in this engine.
//!  * **Time priority is sequence priority.** Ordering never depends on wall-clock
//!    time, because that would make replay non-deterministic.
//!  * **Prices are scaled integers**, not floats. `f64` is not `Ord`, so it cannot
//!    key a `BTreeMap`, and float arithmetic would make P&L drift.
//!  * **Self-trades are allowed** (DECISIONS.md #19), but are tagged.

use std::collections::{BTreeMap, VecDeque};

pub mod ids;
pub use ids::{OrderId, PlayerId};

/// Prices are stored as micro-units: 46.25 is `Price(46_250_000)`.
///
/// This gives exact arithmetic for P&L and lets prices key a `BTreeMap`
/// directly, which `f64` cannot do because it is not `Ord`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Price(pub i64);

impl Price {
    pub const SCALE: i64 = 1_000_000;

    pub fn from_f64(v: f64) -> Self {
        Price((v * Self::SCALE as f64).round() as i64)
    }

    pub fn to_f64(self) -> f64 {
        self.0 as f64 / Self::SCALE as f64
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    Bid,
    Offer,
}

impl Side {
    pub fn opposite(self) -> Side {
        match self {
            Side::Bid => Side::Offer,
            Side::Offer => Side::Bid,
        }
    }
}

/// What an aggressor did. MINE => `Buy`, YOURS => `Sell`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    Buy,
    Sell,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Order {
    pub id: OrderId,
    pub player: PlayerId,
    pub side: Side,
    pub price: Price,
    /// Sequence at which this order entered the book. This is its time priority.
    pub seq: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Trade {
    pub seq: u64,
    pub price: Price,
    pub buyer: PlayerId,
    pub seller: PlayerId,
    pub aggressor: Direction,
    pub resting_order: OrderId,
    pub self_trade: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    Lobby,
    Open,
    Closed,
    Settled,
}

/// Commands are the only way to mutate the engine. They are what gets persisted
/// and replayed.
#[derive(Debug, Clone, PartialEq)]
pub enum Command {
    AddPlayer { player: PlayerId },
    PlaceOrder { player: PlayerId, side: Side, price: Price },
    CancelOrder { player: PlayerId, order: OrderId },
    Take { player: PlayerId, direction: Direction },
    SetPhase { phase: Phase },
    Settle { true_value: Price },
}

#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    PlayerAdded { player: PlayerId },
    OrderAdded { order: Order },
    OrderCancelled { order: OrderId, player: PlayerId },
    Traded { trade: Trade },
    PhaseChanged { phase: Phase },
    Settled { true_value: Price },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reject {
    PositionLimit,
    NotOpen,
    NoLiquidity,
    BadTick,
    BadPrice,
    UnknownOrder,
    NotYourOrder,
    UnknownPlayer,
}

/// Per-player book state. Position is derived, but cached for limit checks.
#[derive(Debug, Default, Clone)]
pub struct Position {
    /// Net lots. Positive is long.
    pub net: i64,
    /// Sum of sells minus sum of buys, in price micro-units.
    pub cash: i64,
    /// Resting bids, for working-order exposure in the limit check.
    pub working_bids: u32,
    /// Resting offers, for working-order exposure in the limit check.
    pub working_offers: u32,
}

#[derive(Debug, Clone)]
pub struct Config {
    /// `None` means any price is accepted.
    pub tick: Option<Price>,
    pub position_limit: i64,
}

/// The order book. Bids and offers each key a price to a FIFO queue, so
/// price-then-sequence priority falls out of the data structure.
#[derive(Debug, Default)]
pub struct Book {
    pub bids: BTreeMap<Price, VecDeque<Order>>,
    pub offers: BTreeMap<Price, VecDeque<Order>>,
}

impl Book {
    /// Highest bid. `BTreeMap` is ascending, so the best bid is the last key.
    pub fn best_bid(&self) -> Option<&Order> {
        self.bids.values().next_back()?.front()
    }

    /// Lowest offer, which is the first key.
    pub fn best_offer(&self) -> Option<&Order> {
        self.offers.values().next()?.front()
    }
}

#[derive(Debug)]
pub struct Market {
    pub config: Config,
    pub phase: Phase,
    pub book: Book,
    pub positions: BTreeMap<PlayerId, Position>,
    pub seq: u64,
    next_order_id: u64,
}

impl Market {
    pub fn new(config: Config) -> Self {
        Market {
            config,
            phase: Phase::Lobby,
            book: Book::default(),
            positions: BTreeMap::new(),
            seq: 0,
            next_order_id: 0,
        }
    }

    fn next_order_id(&mut self) -> OrderId {
        self.next_order_id += 1;
        OrderId(self.next_order_id)
    }

    /// The single entry point. Deterministic: same state plus same command must
    /// always produce the same events.
    ///
    /// ## Rules this must enforce
    ///
    /// 1. **Phase.** `PlaceOrder`, `CancelOrder` and `Take` are only valid while
    ///    `Phase::Open`; otherwise `Reject::NotOpen`.
    /// 2. **Tick.** If `config.tick` is `Some(t)`, a price not divisible by `t`
    ///    is `Reject::BadTick`. Non-positive prices are `Reject::BadPrice`.
    /// 3. **Crossing limit orders match immediately.** A bid at or above the best
    ///    offer trades at the *resting* order's price, not the incoming one.
    ///    Because every order is one lot, such an order matches exactly one
    ///    resting order and never rests. The book must never end up crossed.
    /// 4. **`Take` is a marketable order** against the best price on the far side.
    ///    Empty far side is `Reject::NoLiquidity`.
    /// 5. **Priority is price, then sequence.** Cancelling out of the middle of a
    ///    queue must not disturb the order of everyone else in it.
    /// 6. **Self-trades are allowed** and produce a `Trade` with `self_trade:
    ///    true`. Both legs land on the same player, so their net position is
    ///    unchanged and cash nets to zero.
    /// 7. **Position limit** is enforced on *potential* position, counting
    ///    working orders — `net + working_bids <= limit` and
    ///    `-net + working_offers <= limit`. This means a player can never breach
    ///    the limit even if every one of their resting orders fills at once.
    ///    Breach is `Reject::PositionLimit`.
    /// 8. **Cancel** only your own order; another player's is `Reject::NotYourOrder`
    ///    and a missing one is `Reject::UnknownOrder`.
    pub fn apply(&mut self, _cmd: Command) -> Result<Vec<Event>, Reject> {
        // TODO(tyler): this is yours. See tests/rules.rs for the spec as tests.
        todo!("matching engine")
    }

    /// Final P&L. `cash + net * true_value`, in micro-units.
    pub fn settle_pnl(&self, player: &PlayerId, true_value: Price) -> i64 {
        let p = self.positions.get(player).cloned().unwrap_or_default();
        p.cash + p.net * true_value.0
    }
}
