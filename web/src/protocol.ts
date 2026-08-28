/**
 * The wire contract between the TypeScript front end and the Rust backend.
 *
 * This file is hand-written for now. Once the Rust side exists, it should be
 * REPLACED by output from `ts-rs` (`#[derive(TS)]` on the Rust types, generated
 * during `cargo test`) so that drift between the two fails CI rather than
 * showing up as a blank order book in the pub.
 *
 * Conventions:
 *  - Every field is camelCase on the wire. On the Rust side that means
 *    `#[serde(rename_all = "camelCase")]` on each struct.
 *  - Enums are internally tagged on `t`, i.e. `#[serde(tag = "t", rename_all = "camelCase")]`.
 *  - `price` is a float at the UI edge. The engine stores scaled i64 micro-units
 *    internally; conversion happens at the serde boundary, not here.
 *  - Timestamps are epoch milliseconds.
 *
 * Note on sizes: every order is one lot, so a trade always fully consumes exactly
 * one resting order. There are no partial fills anywhere in this protocol.
 */

export type Phase = "lobby" | "open" | "closed" | "settled";

/** Which side of the book an order rests on. */
export type Side = "bid" | "offer";

/** What an aggressor did. MINE => "buy", YOURS => "sell". */
export type Direction = "buy" | "sell";

export interface SessionMeta {
  code: string;
  question: string;
  /** Host-defined unit label, e.g. "millions of balls". Display only. */
  unit: string;
  /** null means any price is accepted. */
  tickSize: number | null;
  positionLimit: number;
  phase: Phase;
}

export interface Player {
  id: string;
  name: string;
  joinedAt: number;
}

export interface Order {
  id: string;
  playerId: string;
  /** Denormalised so the projector can render names without a player lookup. */
  playerName: string;
  side: Side;
  price: number;
  /** The sequence number at which this order entered the book. This IS its time
   *  priority — the engine must not rely on wall-clock time for ordering. */
  seq: number;
}

export interface Trade {
  id: string;
  seq: number;
  price: number;
  buyerId: string;
  buyerName: string;
  sellerId: string;
  sellerName: string;
  /** Which side crossed the spread. */
  aggressor: Direction;
  /** True when buyer and seller are the same player. Allowed by design — see
   *  DECISIONS.md #19 — but tagged so the tape does not look like a bug. */
  selfTrade: boolean;
  ts: number;
}

export interface BookState {
  /** Best first: highest price, then lowest seq. */
  bids: Order[];
  /** Best first: lowest price, then lowest seq. */
  offers: Order[];
}

/** Per-connection private state. Never contains another player's position. */
export interface YouState {
  playerId: string;
  name: string;
  position: number;
}

export interface Result {
  playerId: string;
  name: string;
  position: number;
  /** Sum of sell prices minus sum of buy prices. */
  cash: number;
  /** cash + position * trueValue */
  pnl: number;
}

export type RejectReason =
  | "position_limit"
  | "not_open"
  | "no_liquidity"
  | "bad_tick"
  | "bad_price"
  | "unknown_order"
  | "not_your_order"
  | "not_host"
  | "unknown_player";

/* ------------------------------------------------------------------ */
/* Server -> client                                                    */
/* ------------------------------------------------------------------ */

/**
 * Every event except `rejected` and `error` carries a monotonic `seq` and is
 * broadcast to every connection in the session. A client that sees a gap in
 * `seq` must send `resync` and will receive a fresh `snapshot`.
 *
 * `rejected` and `error` are addressed to a single connection and carry no seq,
 * because they are not part of the shared event log.
 */
export type ServerEvent =
  | {
      t: "snapshot";
      seq: number;
      session: SessionMeta;
      players: Player[];
      book: BookState;
      /** Full tape, oldest first. */
      trades: Trade[];
      /** null for a host connection — the host does not trade. */
      you: YouState | null;
      /** Present only once phase is "settled". */
      settlement: { trueValue: number; results: Result[] } | null;
    }
  | { t: "playerJoined"; seq: number; player: Player }
  | { t: "phaseChanged"; seq: number; phase: Phase }
  | { t: "orderAdded"; seq: number; order: Order }
  | { t: "orderCancelled"; seq: number; orderId: string; playerId: string }
  | {
      t: "trade";
      seq: number;
      trade: Trade;
      /** The resting order consumed by this trade. Clients remove it from the
       *  book. Always fully consumed, since every order is one lot. */
      restingOrderId: string;
    }
  | { t: "settled"; seq: number; trueValue: number; results: Result[] }
  | { t: "rejected"; reason: RejectReason; message: string }
  | { t: "error"; message: string };

/* ------------------------------------------------------------------ */
/* Client -> server                                                    */
/* ------------------------------------------------------------------ */

export type ClientCommand =
  /** Rest a new order. Players may hold any number per side. */
  | { t: "placeOrder"; side: Side; price: number }
  | { t: "cancelOrder"; orderId: string }
  /** MINE (`buy`) lifts the best offer. YOURS (`sell`) hits the best bid.
   *  Rejected with `no_liquidity` if that side of the book is empty. */
  | { t: "take"; direction: Direction }
  /** Sent after a detected seq gap, or on reconnect. */
  | { t: "resync"; fromSeq: number }
  /* Host-only. Rejected with `not_host` from a player connection. */
  | { t: "openTrading" }
  | { t: "closeTrading" }
  | { t: "settle"; trueValue: number };

/* ------------------------------------------------------------------ */
/* HTTP payloads                                                       */
/* ------------------------------------------------------------------ */

export interface CreateSessionRequest {
  question: string;
  unit: string;
  tickSize: number | null;
  positionLimit: number;
}

export interface CreateSessionResponse {
  code: string;
  /** Secret. Stored in the host's localStorage; gates open/close/settle. */
  hostToken: string;
}

export interface JoinSessionRequest {
  name: string;
}

export interface JoinSessionResponse {
  playerId: string;
  /** Secret. Stored in the player's localStorage so a screen-lock or refresh
   *  restores the same seat rather than creating a new trader. */
  playerToken: string;
}

/* ------------------------------------------------------------------ */
/* Helpers                                                             */
/* ------------------------------------------------------------------ */

export const bestBid = (book: BookState): Order | undefined => book.bids[0];
export const bestOffer = (book: BookState): Order | undefined => book.offers[0];

export function spread(book: BookState): number | null {
  const b = bestBid(book);
  const o = bestOffer(book);
  return b && o ? o.price - b.price : null;
}

/** Position is derivable from the public tape, since trades carry names. */
export function positionFrom(trades: Trade[], playerId: string): number {
  let pos = 0;
  for (const tr of trades) {
    if (tr.buyerId === playerId) pos += 1;
    if (tr.sellerId === playerId) pos -= 1;
  }
  return pos;
}

export function tradesInvolving(trades: Trade[], playerId: string): Trade[] {
  return trades.filter((t) => t.buyerId === playerId || t.sellerId === playerId);
}

export function formatPrice(price: number, tickSize: number | null): string {
  if (tickSize && tickSize < 1) {
    const dp = Math.min(6, (String(tickSize).split(".")[1] ?? "").length);
    return price.toFixed(dp);
  }
  return Number.isInteger(price) ? String(price) : String(Number(price.toFixed(4)));
}
