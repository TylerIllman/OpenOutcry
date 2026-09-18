//! The rules from SPEC.md, written as tests.
//!
//! These all fail right now because `Market::apply` is `todo!()`. Making them
//! green is the job. They are ordered roughly easiest-first.

use engine::*;

fn market() -> Market {
    let mut m = Market::new(Config {
        tick: None,
        position_limit: 10,
    });
    m.apply(Command::AddPlayer {
        player: "alice".into(),
    })
    .unwrap();
    m.apply(Command::AddPlayer {
        player: "bob".into(),
    })
    .unwrap();
    m.apply(Command::SetPhase { phase: Phase::Open }).unwrap();
    m
}

fn bid(m: &mut Market, who: &str, px: f64) -> Result<Vec<Event>, Reject> {
    m.apply(Command::PlaceOrder {
        player: who.into(),
        side: Side::Bid,
        price: Price::from_f64(px),
    })
}

fn offer(m: &mut Market, who: &str, px: f64) -> Result<Vec<Event>, Reject> {
    m.apply(Command::PlaceOrder {
        player: who.into(),
        side: Side::Offer,
        price: Price::from_f64(px),
    })
}

fn take(m: &mut Market, who: &str, dir: Direction) -> Result<Vec<Event>, Reject> {
    m.apply(Command::Take {
        player: who.into(),
        direction: dir,
    })
}

fn trades(events: &[Event]) -> Vec<&Trade> {
    events
        .iter()
        .filter_map(|e| match e {
            Event::Traded { trade } => Some(trade),
            _ => None,
        })
        .collect()
}

// -- Rule 1: phase ---------------------------------------------------------

#[test]
fn orders_rejected_before_trading_opens() {
    let mut m = Market::new(Config {
        tick: None,
        position_limit: 10,
    });
    m.apply(Command::AddPlayer {
        player: "alice".into(),
    })
    .unwrap();
    assert_eq!(bid(&mut m, "alice", 45.0), Err(Reject::NotOpen));
}

#[test]
fn orders_rejected_after_trading_closes() {
    let mut m = market();
    m.apply(Command::SetPhase {
        phase: Phase::Closed,
    })
    .unwrap();
    assert_eq!(bid(&mut m, "alice", 45.0), Err(Reject::NotOpen));
}

// -- Rule 2: tick and price validity ---------------------------------------

#[test]
fn price_off_tick_is_rejected() {
    let mut m = Market::new(Config {
        tick: Some(Price::from_f64(0.5)),
        position_limit: 10,
    });
    m.apply(Command::AddPlayer {
        player: "alice".into(),
    })
    .unwrap();
    m.apply(Command::SetPhase { phase: Phase::Open }).unwrap();
    assert_eq!(bid(&mut m, "alice", 45.25), Err(Reject::BadTick));
    assert!(bid(&mut m, "alice", 45.5).is_ok());
}

#[test]
fn non_positive_price_is_rejected() {
    let mut m = market();
    assert_eq!(bid(&mut m, "alice", 0.0), Err(Reject::BadPrice));
    assert_eq!(bid(&mut m, "alice", -1.0), Err(Reject::BadPrice));
}

// -- Rule 3: crossing limit orders -----------------------------------------

#[test]
fn resting_orders_do_not_cross() {
    let mut m = market();
    bid(&mut m, "alice", 45.0).unwrap();
    offer(&mut m, "bob", 47.0).unwrap();
    assert_eq!(m.book.best_bid().unwrap().price, Price::from_f64(45.0));
    assert_eq!(m.book.best_offer().unwrap().price, Price::from_f64(47.0));
}

#[test]
fn crossing_bid_trades_at_the_resting_price_not_its_own() {
    let mut m = market();
    offer(&mut m, "bob", 47.0).unwrap();

    // Alice bids 50 into an offer at 47. She gets price improvement: the trade
    // prints at 47, the resting order's price.
    let events = bid(&mut m, "alice", 50.0).unwrap();
    let t = trades(&events);
    assert_eq!(t.len(), 1);
    assert_eq!(t[0].price, Price::from_f64(47.0));
    assert_eq!(t[0].buyer, "alice".into());
    assert_eq!(t[0].aggressor, Direction::Buy);
}

#[test]
fn a_crossing_order_never_rests_because_every_order_is_one_lot() {
    let mut m = market();
    offer(&mut m, "bob", 47.0).unwrap();
    bid(&mut m, "alice", 50.0).unwrap();

    // The offer is consumed and Alice's bid is fully filled, so the book is empty.
    assert!(m.book.best_bid().is_none());
    assert!(m.book.best_offer().is_none());
}

// -- Rule 4: take ----------------------------------------------------------

#[test]
fn take_on_an_empty_book_is_rejected() {
    let mut m = market();
    assert_eq!(
        take(&mut m, "alice", Direction::Buy),
        Err(Reject::NoLiquidity)
    );
}

#[test]
fn mine_lifts_the_best_offer() {
    let mut m = market();
    offer(&mut m, "bob", 50.0).unwrap();
    offer(&mut m, "bob", 47.0).unwrap();

    let events = take(&mut m, "alice", Direction::Buy).unwrap();
    let t = trades(&events);
    assert_eq!(
        t[0].price,
        Price::from_f64(47.0),
        "must lift the best, not the first entered"
    );
    assert_eq!(m.book.best_offer().unwrap().price, Price::from_f64(50.0));
}

// -- Rule 5: priority ------------------------------------------------------

#[test]
fn equal_prices_fill_in_sequence_order() {
    let mut m = market();
    offer(&mut m, "alice", 47.0).unwrap();
    offer(&mut m, "bob", 47.0).unwrap();

    let events = take(&mut m, "alice", Direction::Buy).unwrap();
    assert_eq!(
        trades(&events)[0].seller,
        "alice".into(),
        "alice was first in the queue"
    );
}

#[test]
fn cancelling_mid_queue_preserves_everyone_elses_priority() {
    let mut m = market();
    let a = match &offer(&mut m, "alice", 47.0).unwrap()[0] {
        Event::OrderAdded { order } => order.id,
        _ => panic!("expected OrderAdded"),
    };
    let b = match &offer(&mut m, "bob", 47.0).unwrap()[0] {
        Event::OrderAdded { order } => order.id,
        _ => panic!("expected OrderAdded"),
    };
    offer(&mut m, "alice", 47.0).unwrap();

    m.apply(Command::CancelOrder {
        player: "bob".into(),
        order: b,
    })
    .unwrap();

    // Alice's first order still has priority over her second.
    let events = take(&mut m, "bob", Direction::Buy).unwrap();
    assert_eq!(trades(&events)[0].resting_order, a);
}

// -- Rule 6: self-trades ---------------------------------------------------

#[test]
fn self_trades_are_allowed_and_tagged() {
    let mut m = market();
    offer(&mut m, "alice", 47.0).unwrap();
    let events = take(&mut m, "alice", Direction::Buy).unwrap();

    let t = trades(&events);
    assert_eq!(t.len(), 1);
    assert!(t[0].self_trade);
    assert_eq!(t[0].buyer, t[0].seller);
}

#[test]
fn a_self_trade_leaves_position_and_cash_unchanged() {
    let mut m = market();
    offer(&mut m, "alice", 47.0).unwrap();
    take(&mut m, "alice", Direction::Buy).unwrap();

    let p = m
        .positions
        .get(&"alice".into())
        .cloned()
        .unwrap_or_default();
    assert_eq!(p.net, 0);
    assert_eq!(p.cash, 0);
}

// -- Rule 7: position limit ------------------------------------------------

#[test]
fn position_limit_counts_working_orders_not_just_fills() {
    let mut m = Market::new(Config {
        tick: None,
        position_limit: 2,
    });
    for p in ["alice", "bob"] {
        m.apply(Command::AddPlayer { player: p.into() }).unwrap();
    }
    m.apply(Command::SetPhase { phase: Phase::Open }).unwrap();

    bid(&mut m, "alice", 40.0).unwrap();
    bid(&mut m, "alice", 41.0).unwrap();
    // A third working bid could take her to +3 if all filled.
    assert_eq!(bid(&mut m, "alice", 42.0), Err(Reject::PositionLimit));

    // The offer side is unaffected — she is not short.
    assert!(offer(&mut m, "alice", 60.0).is_ok());
}

#[test]
fn cancelling_frees_up_limit_headroom() {
    let mut m = Market::new(Config {
        tick: None,
        position_limit: 1,
    });
    m.apply(Command::AddPlayer {
        player: "alice".into(),
    })
    .unwrap();
    m.apply(Command::SetPhase { phase: Phase::Open }).unwrap();

    let id = match &bid(&mut m, "alice", 40.0).unwrap()[0] {
        Event::OrderAdded { order } => order.id,
        _ => panic!("expected OrderAdded"),
    };
    assert_eq!(bid(&mut m, "alice", 41.0), Err(Reject::PositionLimit));

    m.apply(Command::CancelOrder {
        player: "alice".into(),
        order: id,
    })
    .unwrap();
    assert!(bid(&mut m, "alice", 41.0).is_ok());
}

// -- Rule 8: cancel ownership ----------------------------------------------

#[test]
fn you_cannot_cancel_someone_elses_order() {
    let mut m = market();
    let id = match &bid(&mut m, "alice", 45.0).unwrap()[0] {
        Event::OrderAdded { order } => order.id,
        _ => panic!("expected OrderAdded"),
    };
    assert_eq!(
        m.apply(Command::CancelOrder {
            player: "bob".into(),
            order: id
        }),
        Err(Reject::NotYourOrder)
    );
}

#[test]
fn cancelling_an_unknown_order_is_rejected() {
    let mut m = market();
    assert_eq!(
        m.apply(Command::CancelOrder {
            player: "alice".into(),
            order: OrderId(999)
        }),
        Err(Reject::UnknownOrder)
    );
}

// -- Settlement ------------------------------------------------------------

#[test]
fn pnl_is_cash_plus_position_times_true_value() {
    let mut m = market();
    offer(&mut m, "bob", 47.0).unwrap();
    take(&mut m, "alice", Direction::Buy).unwrap(); // alice long 1 @ 47

    let tv = Price::from_f64(50.0);
    // Alice paid 47 for something worth 50.
    assert_eq!(m.settle_pnl(&"alice".into(), tv), Price::from_f64(3.0).0);
    // Bob is the other side of exactly that.
    assert_eq!(m.settle_pnl(&"bob".into(), tv), Price::from_f64(-3.0).0);
}

#[test]
fn the_whole_market_is_zero_sum() {
    let mut m = market();
    offer(&mut m, "bob", 47.0).unwrap();
    take(&mut m, "alice", Direction::Buy).unwrap();
    bid(&mut m, "alice", 52.0).unwrap();
    take(&mut m, "bob", Direction::Sell).unwrap();

    let tv = Price::from_f64(50.0);
    let total: i64 = ["alice", "bob"]
        .iter()
        .map(|p| m.settle_pnl(&(*p).into(), tv))
        .sum();
    assert_eq!(total, 0, "every lot has a buyer and a seller");
}

// -- Determinism -----------------------------------------------------------

#[test]
fn replaying_the_same_commands_gives_the_same_book() {
    let script = |m: &mut Market| {
        offer(m, "bob", 47.0).unwrap();
        bid(m, "alice", 45.0).unwrap();
        offer(m, "alice", 48.0).unwrap();
        take(m, "alice", Direction::Buy).unwrap();
    };

    let mut a = market();
    let mut b = market();
    script(&mut a);
    script(&mut b);

    assert_eq!(a.seq, b.seq);
    assert_eq!(
        a.fingerprint(),
        b.fingerprint(),
        "same commands must give the same market"
    );
}
