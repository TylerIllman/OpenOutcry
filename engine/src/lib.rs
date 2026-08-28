//! The matching engine.
//!
//! Everything happens through one function: `Market::apply`. Give it a Command,
//! get back a list of Events. It does no I/O, no networking, and never looks at
//! the clock — which is what makes a whole session replayable.
//!
//! Work through GUIDE.md in the repo root. It builds this up one step at a time.

use std::collections::{BTreeMap, VecDeque};

pub mod ids;
pub use ids::{OrderId, PlayerId};

/// A price, stored as a whole number of millionths. 46.25 is `Price(46_250_000)`.
///
/// Why not just use `f64`? Two reasons, and both bite in practice:
///   * 0.1 + 0.2 != 0.3 in floating point, so P&L would slowly drift.
///   * Checking "is this price a multiple of the tick size" is exact with
///     integers (`price.0 % tick.0 == 0`) and a mess with floats.
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

/// Which side of the book an order sits on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    Bid,
    Offer,
}

/// What an aggressor did. MINE is `Buy`, YOURS is `Sell`.
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
    /// Position in the queue. Lower means earlier, so this is time priority.
    /// It is a counter, never a timestamp — timestamps would make replay
    /// non-deterministic.
    pub seq: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Trade {
    pub seq: u64,
    pub price: Price,
    pub buyer: PlayerId,
    pub seller: PlayerId,
    pub aggressor: Direction,
    /// The order that was sitting in the book and got consumed.
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

/// The only way to change the market. These get persisted and replayed.
#[derive(Debug, Clone, PartialEq)]
pub enum Command {
    AddPlayer { player: PlayerId },
    PlaceOrder { player: PlayerId, side: Side, price: Price },
    CancelOrder { player: PlayerId, order: OrderId },
    Take { player: PlayerId, direction: Direction },
    SetPhase { phase: Phase },
    Settle { true_value: Price },
}

/// What actually happened. The server turns these into messages for the browser.
#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    PlayerAdded { player: PlayerId },
    OrderAdded { order: Order },
    OrderCancelled { order: OrderId, player: PlayerId },
    Traded { trade: Trade },
    PhaseChanged { phase: Phase },
    Settled { true_value: Price },
}

/// Why a command was refused.
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

/// What a player is carrying.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Position {
    /// Net lots. Positive is long, negative is short.
    pub net: i64,
    /// Money in: sum of what they sold for, minus what they paid. In millionths.
    pub cash: i64,
}

#[derive(Debug, Clone)]
pub struct Config {
    /// `None` means any price is allowed.
    pub tick: Option<Price>,
    pub position_limit: i64,
}

/// The order book.
///
/// Each side maps a price to a queue of orders sitting at that price.
///
///   * `BTreeMap` keeps prices **sorted**, so the best price is just the first
///     or last key. A `HashMap` would not — that is the whole reason for using
///     it here.
///   * `VecDeque` is a queue you can push onto the back and pop from the front,
///     which is exactly time priority: first in, first filled.
///
/// So price-then-time priority falls out of the data structure rather than
/// being something you have to remember to enforce.
#[derive(Debug, Default, Clone)]
pub struct Book {
    pub bids: BTreeMap<Price, VecDeque<Order>>,
    pub offers: BTreeMap<Price, VecDeque<Order>>,
}

impl Book {
    /// Best bid is the HIGHEST price, and `BTreeMap` sorts ascending, so it is
    /// the last key. `next_back()` walks the iterator from the end.
    pub fn best_bid(&self) -> Option<&Order> {
        self.bids.values().next_back()?.front()
    }

    /// Best offer is the LOWEST price, so it is the first key.
    pub fn best_offer(&self) -> Option<&Order> {
        self.offers.values().next()?.front()
    }

    /// How many orders this player has resting on one side. The position limit
    /// uses this, because an order that could still fill is exposure.
    pub fn working(&self, player: &PlayerId, side: Side) -> i64 {
        let levels = match side {
            Side::Bid => &self.bids,
            Side::Offer => &self.offers,
        };
        levels.values().flatten().filter(|o| &o.player == player).count() as i64
    }
}

#[derive(Debug)]
pub struct Market {
    pub config: Config,
    pub phase: Phase,
    pub book: Book,
    pub positions: BTreeMap<PlayerId, Position>,
    /// Counter behind both order sequence numbers and trade sequence numbers.
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

    /// Hand out the next order id. Call this once per new order.
    pub fn next_order_id(&mut self) -> OrderId {
        self.next_order_id += 1;
        OrderId(self.next_order_id)
    }

    /// The one entry point.
    ///
    /// The rules live in GUIDE.md, one stage at a time. Start at Stage 1.
    pub fn apply(&mut self, _cmd: Command) -> Result<Vec<Event>, Reject> {
        todo!("GUIDE.md, Stage 1, Step 1")
    }

    /// Final score for one player: the cash they took in, plus whatever their
    /// position turned out to be worth.
    pub fn settle_pnl(&self, player: &PlayerId, true_value: Price) -> i64 {
        let p = self.positions.get(player).cloned().unwrap_or_default();
        p.cash + p.net * true_value.0
    }
}
