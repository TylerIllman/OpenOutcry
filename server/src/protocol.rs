//! Wire types. These mirror `web/src/protocol.ts` exactly.
//!
//! Right now the two are kept in step by hand, which will not last. The fix is
//! `ts-rs`: derive `TS` on everything here and let `cargo test` generate the
//! TypeScript, so a mismatch breaks the build rather than the game.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, TS)]
#[ts(export, export_to = "../../web/src/generated/")]
#[serde(rename_all = "camelCase")]
pub enum Phase {
    Lobby,
    Open,
    Closed,
    Settled,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, TS)]
#[ts(export, export_to = "../../web/src/generated/")]
#[serde(rename_all = "camelCase")]
pub enum Side {
    Bid,
    Offer,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, TS)]
#[ts(export, export_to = "../../web/src/generated/")]
#[serde(rename_all = "camelCase")]
pub enum Direction {
    Buy,
    Sell,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../web/src/generated/")]
#[serde(rename_all = "camelCase")]
pub struct SessionMeta {
    pub code: String,
    pub question: String,
    pub unit: String,
    pub tick_size: Option<f64>,
    #[ts(type = "number")]
    pub position_limit: i64,
    pub phase: Phase,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../web/src/generated/")]
#[serde(rename_all = "camelCase")]
pub struct Player {
    pub id: String,
    pub name: String,
    #[ts(type = "number")]
    pub joined_at: i64,
    /// Bots are ordinary players to the engine — they hold positions, respect
    /// the limit and settle like anyone else. This only tells the UI to mark
    /// them so the room knows who it is trading against.
    pub is_bot: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../web/src/generated/")]
#[serde(rename_all = "camelCase")]
pub struct Order {
    pub id: String,
    pub player_id: String,
    pub player_name: String,
    pub side: Side,
    pub price: f64,
    #[ts(type = "number")]
    pub seq: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../web/src/generated/")]
#[serde(rename_all = "camelCase")]
pub struct Trade {
    pub id: String,
    #[ts(type = "number")]
    pub seq: u64,
    pub price: f64,
    pub buyer_id: String,
    pub buyer_name: String,
    pub seller_id: String,
    pub seller_name: String,
    pub aggressor: Direction,
    pub self_trade: bool,
    #[ts(type = "number")]
    pub ts: i64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../web/src/generated/")]
#[serde(rename_all = "camelCase")]
pub struct BookState {
    pub bids: Vec<Order>,
    pub offers: Vec<Order>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../web/src/generated/")]
#[serde(rename_all = "camelCase")]
pub struct YouState {
    pub player_id: String,
    pub name: String,
    #[ts(type = "number")]
    pub position: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../web/src/generated/")]
#[serde(rename_all = "camelCase")]
pub struct Result {
    pub player_id: String,
    pub name: String,
    #[ts(type = "number")]
    pub position: i64,
    pub cash: f64,
    pub pnl: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../web/src/generated/")]
#[serde(rename_all = "camelCase")]
pub struct Settlement {
    pub true_value: f64,
    pub results: Vec<Result>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../web/src/generated/")]
#[serde(rename_all = "snake_case")]
pub enum RejectReason {
    PositionLimit,
    NotOpen,
    NoLiquidity,
    BadTick,
    BadPrice,
    UnknownOrder,
    NotYourOrder,
    NotHost,
    UnknownPlayer,
}

/// Server -> client. Internally tagged on `t` to match the TS discriminated union.
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export, export_to = "../../web/src/generated/")]
#[serde(tag = "t", rename_all = "camelCase")]
pub enum ServerEvent {
    #[serde(rename_all = "camelCase")]
    Snapshot {
        #[ts(type = "number")]
        seq: u64,
        session: SessionMeta,
        players: Vec<Player>,
        book: BookState,
        trades: Vec<Trade>,
        you: Option<YouState>,
        settlement: Option<Settlement>,
    },
    #[serde(rename_all = "camelCase")]
    PlayerJoined {
        #[ts(type = "number")]
        seq: u64,
        player: Player,
    },
    #[serde(rename_all = "camelCase")]
    PhaseChanged {
        #[ts(type = "number")]
        seq: u64,
        phase: Phase,
    },
    #[serde(rename_all = "camelCase")]
    OrderAdded {
        #[ts(type = "number")]
        seq: u64,
        order: Order,
    },
    #[serde(rename_all = "camelCase")]
    OrderCancelled {
        #[ts(type = "number")]
        seq: u64,
        order_id: String,
        player_id: String,
    },
    #[serde(rename_all = "camelCase")]
    Trade {
        #[ts(type = "number")]
        seq: u64,
        trade: Trade,
        resting_order_id: String,
    },
    #[serde(rename_all = "camelCase")]
    Settled {
        #[ts(type = "number")]
        seq: u64,
        true_value: f64,
        results: Vec<Result>,
    },
    #[serde(rename_all = "camelCase")]
    Rejected {
        reason: RejectReason,
        message: String,
    },
    #[serde(rename_all = "camelCase")]
    Error { message: String },
}

/// Client -> server.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../web/src/generated/")]
#[serde(tag = "t", rename_all = "camelCase")]
pub enum ClientCommand {
    #[serde(rename_all = "camelCase")]
    PlaceOrder {
        side: Side,
        price: f64,
    },
    #[serde(rename_all = "camelCase")]
    CancelOrder {
        order_id: String,
    },
    #[serde(rename_all = "camelCase")]
    Take {
        direction: Direction,
    },
    #[serde(rename_all = "camelCase")]
    Resync {
        #[ts(type = "number")]
        from_seq: u64,
    },
    OpenTrading,
    CloseTrading,
    #[serde(rename_all = "camelCase")]
    Settle {
        true_value: f64,
    },
    /// Host-only. Adds one bot.
    ///
    /// Bots have no view on price. They buy and sell at random, at a rate the
    /// host sets — they are order flow, not opinion.
    AddBot,
    /// Host-only. How the bots behave.
    ///
    /// `orders_per_minute` is per bot. `buy_bias` is 0..1: 0.5 is even, higher
    /// buys more than it sells, so the host can lean the flow one way.
    #[serde(rename_all = "camelCase")]
    SetBotFlow {
        orders_per_minute: f64,
        buy_bias: f64,
    },
    /// Host-only. Stops the bots trading.
    RemoveBots,
}

/* -- HTTP payloads -------------------------------------------------------- */

#[derive(Debug, Deserialize, TS)]
#[ts(export, export_to = "../../web/src/generated/")]
#[serde(rename_all = "camelCase")]
pub struct CreateSessionRequest {
    pub question: String,
    pub unit: String,
    pub tick_size: Option<f64>,
    #[ts(type = "number")]
    pub position_limit: i64,
}

#[derive(Debug, Serialize, TS)]
#[ts(export, export_to = "../../web/src/generated/")]
#[serde(rename_all = "camelCase")]
pub struct CreateSessionResponse {
    pub code: String,
    pub host_token: String,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export, export_to = "../../web/src/generated/")]
#[serde(rename_all = "camelCase")]
pub struct JoinSessionRequest {
    pub name: String,
}

#[derive(Debug, Serialize, TS)]
#[ts(export, export_to = "../../web/src/generated/")]
#[serde(rename_all = "camelCase")]
pub struct JoinSessionResponse {
    pub player_id: String,
    pub player_token: String,
}
