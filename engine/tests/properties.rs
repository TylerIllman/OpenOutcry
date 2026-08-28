//! Property-based tests.
//!
//! The tests in `rules.rs` check cases someone thought of. These throw
//! thousands of random command sequences at the engine and assert that the
//! things which must *always* be true still are, after every single command.
//!
//! An invariant failure here comes with the exact sequence that broke it,
//! shrunk to the shortest one that still fails.

use engine::*;
use proptest::prelude::*;

const PLAYERS: [&str; 3] = ["alice", "bob", "priya"];

#[derive(Debug, Clone)]
enum Action {
    Place { who: usize, side: bool, price: i64 },
    Take { who: usize, buy: bool },
    /// Cancel the nth live order belonging to this player, if they have one.
    Cancel { who: usize, nth: usize },
}

fn action() -> impl Strategy<Value = Action> {
    prop_oneof![
        // Weighted towards resting orders, so the book actually builds up
        // rather than being emptied as fast as it fills.
        3 => (0..PLAYERS.len(), any::<bool>(), 1i64..40).prop_map(|(who, side, price)| {
            Action::Place { who, side, price }
        }),
        1 => (0..PLAYERS.len(), any::<bool>()).prop_map(|(who, buy)| Action::Take { who, buy }),
        1 => (0..PLAYERS.len(), 0usize..4).prop_map(|(who, nth)| Action::Cancel { who, nth }),
    ]
}

fn live_orders(market: &Market, player: &PlayerId) -> Vec<OrderId> {
    market
        .book
        .bids
        .values()
        .chain(market.book.offers.values())
        .flatten()
        .filter(|o| &o.player == player)
        .map(|o| o.id)
        .collect()
}

/// Everything that must hold after any accepted command.
fn check_invariants(market: &Market, limit: i64) {
    // 1. The book is never crossed. A bid at or above the best offer trades
    //    instead of resting, so this must hold strictly.
    if let (Some(bid), Some(offer)) = (market.book.best_bid(), market.book.best_offer()) {
        prop_assert_crossed(bid.price, offer.price);
    }

    // 2. No empty price levels. One left behind would make best_bid/best_offer
    //    report an empty book.
    for (_, queue) in market.book.bids.iter().chain(market.book.offers.iter()) {
        assert!(!queue.is_empty(), "empty price level left in the book");
    }

    // 3. Queues are in sequence order, which is time priority.
    for (_, queue) in market.book.bids.iter().chain(market.book.offers.iter()) {
        let mut last = 0;
        for o in queue {
            assert!(o.seq > last, "queue out of sequence order");
            last = o.seq;
        }
    }

    // 4. Order ids are unique across the whole book.
    let mut ids: Vec<u64> = market
        .book
        .bids
        .values()
        .chain(market.book.offers.values())
        .flatten()
        .map(|o| o.id.0)
        .collect();
    let total = ids.len();
    ids.sort_unstable();
    ids.dedup();
    assert_eq!(ids.len(), total, "duplicate order id in the book");

    // 5. The market is zero-sum. Every lot has a buyer and a seller, so
    //    positions and cash must both net to nothing across all players.
    let net: i64 = market.positions.values().map(|p| p.net).sum();
    let cash: i64 = market.positions.values().map(|p| p.cash).sum();
    assert_eq!(net, 0, "positions do not net to zero");
    assert_eq!(cash, 0, "cash does not net to zero");

    // 6. The cached working-order counts agree with walking the book. The risk
    //    check reads the cached ones, so if these ever drift the limit is
    //    silently enforcing the wrong number.
    for name in PLAYERS {
        let id = PlayerId(name.to_string());
        let pos = market.positions.get(&id).cloned().unwrap_or_default();
        assert_eq!(
            pos.working_bids,
            market.book.working(&id, Side::Bid),
            "{name}: cached working_bids disagrees with the book"
        );
        assert_eq!(
            pos.working_offers,
            market.book.working(&id, Side::Offer),
            "{name}: cached working_offers disagrees with the book"
        );
    }

    // 7. Nobody is over their limit, counting orders that could still fill.
    for name in PLAYERS {
        let id = PlayerId(name.to_string());
        let pos = market.positions.get(&id).cloned().unwrap_or_default();
        let long = pos.net + market.book.working(&id, Side::Bid);
        let short = -pos.net + market.book.working(&id, Side::Offer);
        assert!(long <= limit, "{name} could end up long {long} with a limit of {limit}");
        assert!(short <= limit, "{name} could end up short {short} with a limit of {limit}");
    }
}

fn prop_assert_crossed(bid: Price, offer: Price) {
    assert!(
        bid.0 < offer.0,
        "book is crossed: bid {} >= offer {}",
        bid.to_f64(),
        offer.to_f64()
    );
}

fn run(actions: &[Action], limit: i64, tick: Option<Price>) {
    let mut market = Market::new(Config { tick, position_limit: limit });
    for name in PLAYERS {
        market.apply(Command::AddPlayer { player: PlayerId(name.to_string()) }).unwrap();
    }
    market.apply(Command::SetPhase { phase: Phase::Open }).unwrap();

    for action in actions {
        let cmd = match action {
            Action::Place { who, side, price } => Command::PlaceOrder {
                player: PlayerId(PLAYERS[*who].to_string()),
                side: if *side { Side::Bid } else { Side::Offer },
                price: Price(price * Price::SCALE),
            },
            Action::Take { who, buy } => Command::Take {
                player: PlayerId(PLAYERS[*who].to_string()),
                direction: if *buy { Direction::Buy } else { Direction::Sell },
            },
            Action::Cancel { who, nth } => {
                let player = PlayerId(PLAYERS[*who].to_string());
                let orders = live_orders(&market, &player);
                if orders.is_empty() {
                    continue;
                }
                Command::CancelOrder { player, order: orders[nth % orders.len()] }
            }
        };

        // A rejection is a valid outcome. What matters is that whether it was
        // accepted or refused, the market is still sane afterwards.
        let _ = market.apply(cmd);
        check_invariants(&market, limit);
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(400))]

    #[test]
    fn invariants_hold_through_any_sequence(actions in prop::collection::vec(action(), 0..120)) {
        run(&actions, 5, None);
    }

    #[test]
    fn invariants_hold_with_a_tight_limit(actions in prop::collection::vec(action(), 0..120)) {
        run(&actions, 1, None);
    }

    #[test]
    fn invariants_hold_with_a_tick_size(actions in prop::collection::vec(action(), 0..120)) {
        run(&actions, 5, Some(Price(2 * Price::SCALE)));
    }

    /// The same commands must always produce the same market. If this fails,
    /// something in the engine is reading state it should not — and replay
    /// from the command log would be worthless.
    #[test]
    fn the_engine_is_deterministic(actions in prop::collection::vec(action(), 0..120)) {
        let fingerprint = |actions: &[Action]| {
            let mut m = Market::new(Config { tick: None, position_limit: 5 });
            for name in PLAYERS {
                m.apply(Command::AddPlayer { player: PlayerId(name.to_string()) }).unwrap();
            }
            m.apply(Command::SetPhase { phase: Phase::Open }).unwrap();
            for action in actions {
                let cmd = match action {
                    Action::Place { who, side, price } => Command::PlaceOrder {
                        player: PlayerId(PLAYERS[*who].to_string()),
                        side: if *side { Side::Bid } else { Side::Offer },
                        price: Price(price * Price::SCALE),
                    },
                    Action::Take { who, buy } => Command::Take {
                        player: PlayerId(PLAYERS[*who].to_string()),
                        direction: if *buy { Direction::Buy } else { Direction::Sell },
                    },
                    Action::Cancel { who, nth } => {
                        let player = PlayerId(PLAYERS[*who].to_string());
                        let orders = live_orders(&m, &player);
                        if orders.is_empty() { continue; }
                        Command::CancelOrder { player, order: orders[nth % orders.len()] }
                    }
                };
                let _ = m.apply(cmd);
            }
            m.fingerprint()
        };

        prop_assert_eq!(fingerprint(&actions), fingerprint(&actions));
    }
}
