//! Benchmarks.
//!
//! **Methodology note, because it changes how you read these numbers.**
//!
//! The obvious way to write these — build a fresh book in `iter_batched`, then
//! time one command against it — is wrong. Criterion includes the destructor of
//! the batched input inside the measured region, so the timing is dominated by
//! tearing down a book with tens of thousands of orders in it. It produces
//! numbers that scale linearly with depth and look like a `BTreeMap` lookup
//! that isn't logarithmic.
//!
//! So each benchmark below works against a book built **once**, and performs a
//! **pair** of operations that leaves the book in the same shape it started in.
//! Nothing is allocated or freed inside the timed section. The trade-off is that
//! every number here is the cost of two commands, not one.
//!
//! Run with `cargo bench -p engine`; criterion writes an HTML report to
//! `target/criterion/report/index.html`.

use std::hint::black_box;

use criterion::{Criterion, criterion_group, criterion_main};
use engine::*;

const TRADERS: usize = 20;
/// High enough that nothing is refused for risk during a run.
const NO_LIMIT: i64 = i64::MAX / 4;

fn open_market() -> Market {
    let mut m = Market::new(Config {
        tick: None,
        position_limit: NO_LIMIT,
    });
    for i in 0..TRADERS {
        m.apply(Command::AddPlayer {
            player: PlayerId(format!("p{i}")),
        })
        .unwrap();
    }
    m.apply(Command::SetPhase { phase: Phase::Open }).unwrap();
    m
}

/// `depth` orders resting on each side, bids at 501..=1000 and offers at
/// 1001..=1500, spread across all the traders.
fn book_with_depth(depth: i64) -> Market {
    let mut m = open_market();
    for i in 0..depth {
        let who = PlayerId(format!("p{}", (i as usize) % TRADERS));
        m.apply(Command::PlaceOrder {
            player: who.clone(),
            side: Side::Bid,
            price: Price((1000 - i % 500) * Price::SCALE),
        })
        .unwrap();
        m.apply(Command::PlaceOrder {
            player: who,
            side: Side::Offer,
            price: Price((1001 + i % 500) * Price::SCALE),
        })
        .unwrap();
    }
    m
}

const BEST_OFFER: i64 = 1001;

fn benches(c: &mut Criterion) {
    // Rest an order at the best offer, then lift the best offer. The order
    // placed joins the back of that price level and the take consumes the
    // front, so depth and price distribution are identical afterwards.
    let mut group = c.benchmark_group("place_then_take");
    for depth in [10i64, 1_000, 10_000] {
        let mut m = book_with_depth(depth);
        group.bench_function(format!("depth_{depth}"), |b| {
            b.iter(|| {
                black_box(m.apply(Command::PlaceOrder {
                    player: PlayerId("p0".into()),
                    side: Side::Offer,
                    price: Price(BEST_OFFER * Price::SCALE),
                }))
                .ok();
                black_box(m.apply(Command::Take {
                    player: PlayerId("p1".into()),
                    direction: Direction::Buy,
                }))
                .ok();
            })
        });
    }
    group.finish();

    // Rest an order and pull it again. This is what the OrderId index exists
    // for: without it, finding the order means walking every price level on
    // both sides.
    let mut group = c.benchmark_group("place_then_cancel");
    for depth in [10i64, 1_000, 10_000] {
        let mut m = book_with_depth(depth);
        let mut next_id = m.book.bids.values().flatten().count() as u64
            + m.book.offers.values().flatten().count() as u64;
        group.bench_function(format!("depth_{depth}"), |b| {
            b.iter(|| {
                next_id += 1;
                black_box(m.apply(Command::PlaceOrder {
                    player: PlayerId("p0".into()),
                    side: Side::Bid,
                    price: Price(600 * Price::SCALE),
                }))
                .ok();
                black_box(m.apply(Command::CancelOrder {
                    player: PlayerId("p0".into()),
                    order: OrderId(next_id),
                }))
                .ok();
            })
        });
    }
    group.finish();
}

criterion_group!(matching, benches);
criterion_main!(matching);
