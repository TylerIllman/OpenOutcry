//! The matching engine.
//!
//! Everything happens through one function: `Market::apply`. Give it a Command,
//! get back a list of Events. It does no I/O, no networking, and never looks at
//! the clock — which is what makes a whole session replayable.
//!
//! Work through GUIDE.md in the repo root. It builds this up one step at a time.

use std::collections::{BTreeMap, HashMap, VecDeque};

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
    /// Live bids this player has resting. Maintained incrementally, because the
    /// risk check runs on every order and counting them by walking the book
    /// makes order entry O(depth). `Book::working` is the same number computed
    /// the slow way, and the property tests check the two agree.
    pub working_bids: i64,
    /// Live offers this player has resting. See `working_bids`.
    pub working_offers: i64,
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

    /// How many orders this player has resting on one side, counted by walking
    /// the book.
    ///
    /// This is O(depth) and deliberately not used on the hot path — the risk
    /// check reads the counters cached on `Position` instead. This exists as
    /// the slow, obviously-correct version those counters are tested against.
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
    /// Where each live order sits, so cancel is a lookup rather than a scan of
    /// every price level on both sides.
    index: HashMap<OrderId, (Side, Price)>,
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
            index: HashMap::new(),
        }
    }

    /// Hand out the next order id. Call this once per new order.
    pub fn next_order_id(&mut self) -> OrderId {
        self.next_order_id += 1;
        OrderId(self.next_order_id)
    }

    /// The one entry point.
    ///
    /// Deterministic: the same state plus the same command always produces the
    /// same events. Nothing in here reads the clock or does I/O, which is what
    /// makes a session replayable from its command log.
    pub fn apply(&mut self, cmd: Command) -> Result<Vec<Event>, Reject> {
        match cmd {
            Command::AddPlayer { player } => {
                self.positions.entry(player.clone()).or_default();
                Ok(vec![Event::PlayerAdded { player }])
            }
            Command::SetPhase { phase } => {
                self.phase = phase;
                Ok(vec![Event::PhaseChanged { phase }])
            }
            Command::Settle { true_value } => {
                self.phase = Phase::Settled;
                Ok(vec![Event::Settled { true_value }])
            }
            Command::PlaceOrder { player, side, price } => self.place_order(player, side, price),
            Command::CancelOrder { player, order } => self.cancel_order(player, order),
            Command::Take { player, direction } => self.take(player, direction),
        }
    }

    fn require_open(&self) -> Result<(), Reject> {
        if self.phase == Phase::Open { Ok(()) } else { Err(Reject::NotOpen) }
    }

    fn require_player(&self, player: &PlayerId) -> Result<(), Reject> {
        if self.positions.contains_key(player) { Ok(()) } else { Err(Reject::UnknownPlayer) }
    }

    /// The position limit counts orders that could still fill, not just the
    /// position already taken on. `add_long` / `add_short` describe what the
    /// command about to be applied would add to each side of that exposure.
    ///
    /// A resting bid and a buy both add one to potential length, so the same
    /// check covers resting an order and crossing the spread.
    fn would_breach(&self, player: &PlayerId, add_long: i64, add_short: i64) -> bool {
        let pos = self.positions.get(player).cloned().unwrap_or_default();
        let limit = self.config.position_limit;
        let potential_long = pos.net + pos.working_bids + add_long;
        let potential_short = -pos.net + pos.working_offers + add_short;
        potential_long > limit || potential_short > limit
    }

    fn level_mut(&mut self, side: Side, price: Price) -> &mut VecDeque<Order> {
        let levels = match side {
            Side::Bid => &mut self.book.bids,
            Side::Offer => &mut self.book.offers,
        };
        levels.entry(price).or_default()
    }

    fn place_order(
        &mut self,
        player: PlayerId,
        side: Side,
        price: Price,
    ) -> Result<Vec<Event>, Reject> {
        self.require_open()?;
        self.require_player(&player)?;

        if price.0 <= 0 {
            return Err(Reject::BadPrice);
        }
        if let Some(tick) = self.config.tick {
            if tick.0 <= 0 || price.0 % tick.0 != 0 {
                return Err(Reject::BadTick);
            }
        }

        let (add_long, add_short) = match side {
            Side::Bid => (1, 0),
            Side::Offer => (0, 1),
        };
        if self.would_breach(&player, add_long, add_short) {
            return Err(Reject::PositionLimit);
        }

        // A limit order that crosses trades instead of resting, at the resting
        // order's price — so the aggressor gets the price improvement. Because
        // every order is one lot it consumes exactly one resting order and is
        // then used up, so it never rests and there is no partial fill.
        let crosses = match side {
            Side::Bid => self.book.best_offer().is_some_and(|o| price >= o.price),
            Side::Offer => self.book.best_bid().is_some_and(|o| price <= o.price),
        };
        if crosses {
            let direction = match side {
                Side::Bid => Direction::Buy,
                Side::Offer => Direction::Sell,
            };
            let trade = self.execute(&player, direction).ok_or(Reject::NoLiquidity)?;
            return Ok(vec![Event::Traded { trade }]);
        }

        self.seq += 1;
        let order = Order {
            id: self.next_order_id(),
            player,
            side,
            price,
            seq: self.seq,
        };
        self.index.insert(order.id, (side, price));
        self.bump_working(&order.player, side, 1);

        // Clone for the event, because pushing into the book moves the order.
        let event = Event::OrderAdded { order: order.clone() };
        self.level_mut(side, price).push_back(order);
        Ok(vec![event])
    }

    fn take(&mut self, player: PlayerId, direction: Direction) -> Result<Vec<Event>, Reject> {
        self.require_open()?;
        self.require_player(&player)?;

        let available = match direction {
            Direction::Buy => self.book.best_offer().is_some(),
            Direction::Sell => self.book.best_bid().is_some(),
        };
        if !available {
            return Err(Reject::NoLiquidity);
        }

        let (add_long, add_short) = match direction {
            Direction::Buy => (1, 0),
            Direction::Sell => (0, 1),
        };
        if self.would_breach(&player, add_long, add_short) {
            return Err(Reject::PositionLimit);
        }

        let trade = self.execute(&player, direction).ok_or(Reject::NoLiquidity)?;
        Ok(vec![Event::Traded { trade }])
    }

    /// Consume the best order on the far side and book the trade.
    ///
    /// Shared by `take` and by a crossing limit order, so the two can never
    /// drift apart.
    /// Adjust a player's cached count of resting orders on one side.
    fn bump_working(&mut self, player: &PlayerId, side: Side, delta: i64) {
        let pos = self.positions.entry(player.clone()).or_default();
        match side {
            Side::Bid => pos.working_bids += delta,
            Side::Offer => pos.working_offers += delta,
        }
    }

    fn execute(&mut self, taker: &PlayerId, direction: Direction) -> Option<Trade> {
        let (side, price) = match direction {
            Direction::Buy => (Side::Offer, self.book.best_offer()?.price),
            Direction::Sell => (Side::Bid, self.book.best_bid()?.price),
        };

        let levels = match side {
            Side::Bid => &mut self.book.bids,
            Side::Offer => &mut self.book.offers,
        };
        let queue = levels.get_mut(&price)?;
        let resting = queue.pop_front()?;
        // A price level left holding an empty queue would make best_bid() and
        // best_offer() report an empty book, because they ask that level for a
        // front order and get nothing.
        if queue.is_empty() {
            levels.remove(&price);
        }
        self.index.remove(&resting.id);
        self.bump_working(&resting.player, side, -1);

        let (buyer, seller) = match direction {
            Direction::Buy => (taker.clone(), resting.player.clone()),
            Direction::Sell => (resting.player.clone(), taker.clone()),
        };
        let self_trade = buyer == seller;

        // Both legs are applied even when they are the same player, so a
        // self-trade nets to no position and no cash rather than being skipped.
        let b = self.positions.entry(buyer.clone()).or_default();
        b.net += 1;
        b.cash -= price.0;
        let s = self.positions.entry(seller.clone()).or_default();
        s.net -= 1;
        s.cash += price.0;

        self.seq += 1;
        Some(Trade {
            seq: self.seq,
            price,
            buyer,
            seller,
            aggressor: direction,
            resting_order: resting.id,
            self_trade,
        })
    }

    fn cancel_order(&mut self, player: PlayerId, order: OrderId) -> Result<Vec<Event>, Reject> {
        self.require_open()?;

        let (side, price) = *self.index.get(&order).ok_or(Reject::UnknownOrder)?;
        let levels = match side {
            Side::Bid => &mut self.book.bids,
            Side::Offer => &mut self.book.offers,
        };
        let queue = levels.get_mut(&price).ok_or(Reject::UnknownOrder)?;
        let at = queue.iter().position(|o| o.id == order).ok_or(Reject::UnknownOrder)?;

        if queue[at].player != player {
            return Err(Reject::NotYourOrder);
        }

        // `remove` shifts the rest along and keeps their order, so everyone
        // behind the cancelled order keeps their place in the queue.
        queue.remove(at);
        if queue.is_empty() {
            levels.remove(&price);
        }
        self.index.remove(&order);
        self.bump_working(&player, side, -1);

        Ok(vec![Event::OrderCancelled { order, player }])
    }

    /// A stable string describing the entire market state.
    ///
    /// Two markets with the same fingerprint hold the same book, in the same
    /// queue order, with the same positions. Used to check that a session
    /// replayed from its command log matches the one that was played.
    ///
    /// Deterministic because both `BTreeMap`s iterate in key order and each
    /// price level is already in arrival order.
    pub fn fingerprint(&self) -> String {
        use std::fmt::Write;
        let mut out = String::new();
        let _ = write!(out, "phase={:?};", self.phase);
        for (price, queue) in &self.book.bids {
            for o in queue {
                let _ = write!(out, "B{}@{}~{};", o.id.0, price.0, o.player.0);
            }
        }
        for (price, queue) in &self.book.offers {
            for o in queue {
                let _ = write!(out, "O{}@{}~{};", o.id.0, price.0, o.player.0);
            }
        }
        for (player, pos) in &self.positions {
            let _ = write!(
                out,
                "P{}={}/{}/{}/{};",
                player.0, pos.net, pos.cash, pos.working_bids, pos.working_offers
            );
        }
        out
    }

    /// Final score for one player: the cash they took in, plus whatever their
    /// position turned out to be worth.
    pub fn settle_pnl(&self, player: &PlayerId, true_value: Price) -> i64 {
        let p = self.positions.get(player).cloned().unwrap_or_default();
        p.cash + p.net * true_value.0
    }
}
