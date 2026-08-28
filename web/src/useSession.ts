import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import type {
  BookState,
  ClientCommand,
  Order,
  Player,
  Result,
  ServerEvent,
  SessionMeta,
  Trade,
  YouState,
} from "./protocol";

export interface RejectNotice {
  message: string;
  at: number;
}

export interface ClientState {
  connection: "connecting" | "live" | "reconnecting" | "failed";
  /** Highest sequence number applied. A gap triggers a resync. */
  seq: number;
  session: SessionMeta | null;
  players: Player[];
  book: BookState;
  trades: Trade[];
  you: YouState | null;
  settlement: { trueValue: number; results: Result[] } | null;
  reject: RejectNotice | null;
  error: string | null;
}

const empty: ClientState = {
  connection: "connecting",
  seq: 0,
  session: null,
  players: [],
  book: { bids: [], offers: [] },
  trades: [],
  you: null,
  settlement: null,
  reject: null,
  error: null,
};

/** Bids descend by price; offers ascend. Ties break on seq, which is time priority. */
function insert(orders: Order[], order: Order, side: "bid" | "offer"): Order[] {
  const next = [...orders, order];
  next.sort((a, b) =>
    a.price !== b.price
      ? side === "bid"
        ? b.price - a.price
        : a.price - b.price
      : a.seq - b.seq,
  );
  return next;
}

const without = (orders: Order[], id: string) => orders.filter((o) => o.id !== id);

export function apply(state: ClientState, ev: ServerEvent): ClientState {
  switch (ev.t) {
    case "snapshot":
      return {
        ...state,
        connection: "live",
        seq: ev.seq,
        session: ev.session,
        players: ev.players,
        book: ev.book,
        trades: ev.trades,
        you: ev.you,
        settlement: ev.settlement,
        error: null,
      };

    case "playerJoined":
      return {
        ...state,
        seq: ev.seq,
        players: state.players.some((p) => p.id === ev.player.id)
          ? state.players
          : [...state.players, ev.player],
      };

    case "phaseChanged":
      return {
        ...state,
        seq: ev.seq,
        session: state.session ? { ...state.session, phase: ev.phase } : null,
      };

    case "orderAdded":
      return {
        ...state,
        seq: ev.seq,
        book:
          ev.order.side === "bid"
            ? { ...state.book, bids: insert(state.book.bids, ev.order, "bid") }
            : { ...state.book, offers: insert(state.book.offers, ev.order, "offer") },
      };

    case "orderCancelled":
      return {
        ...state,
        seq: ev.seq,
        book: {
          bids: without(state.book.bids, ev.orderId),
          offers: without(state.book.offers, ev.orderId),
        },
      };

    case "trade": {
      // Every order is one lot, so the resting order is always fully consumed.
      const you = state.you;
      const delta =
        you === null
          ? 0
          : (ev.trade.buyerId === you.playerId ? 1 : 0) -
            (ev.trade.sellerId === you.playerId ? 1 : 0);
      return {
        ...state,
        seq: ev.seq,
        trades: [...state.trades, ev.trade],
        book: {
          bids: without(state.book.bids, ev.restingOrderId),
          offers: without(state.book.offers, ev.restingOrderId),
        },
        you: you ? { ...you, position: you.position + delta } : null,
      };
    }

    case "settled":
      return {
        ...state,
        seq: ev.seq,
        session: state.session ? { ...state.session, phase: "settled" } : null,
        settlement: { trueValue: ev.trueValue, results: ev.results },
      };

    // Addressed to this connection only, and outside the sequence.
    case "rejected":
      return { ...state, reject: { message: ev.message, at: Date.now() } };

    case "error":
      return { ...state, error: ev.message };
  }
}

export interface UseSessionArgs {
  code: string;
  hostToken?: string | null;
  playerToken?: string | null;
}

function socketUrl({ code, hostToken, playerToken }: UseSessionArgs): string {
  const proto = location.protocol === "https:" ? "wss:" : "ws:";
  const params = new URLSearchParams({ code });
  if (hostToken) params.set("hostToken", hostToken);
  if (playerToken) params.set("playerToken", playerToken);
  return `${proto}//${location.host}/ws?${params}`;
}

export function useSession(args: UseSessionArgs) {
  const [state, setState] = useState<ClientState>(empty);
  const socket = useRef<WebSocket | null>(null);
  const attempt = useRef(0);
  const alive = useRef(true);
  const seq = useRef(0);

  const url = socketUrl(args);

  useEffect(() => {
    alive.current = true;

    const connect = () => {
      if (!alive.current) return;
      const ws = new WebSocket(url);
      socket.current = ws;

      ws.onopen = () => {
        attempt.current = 0;
        setState((s) => ({ ...s, connection: "live" }));
      };

      ws.onmessage = (msg) => {
        let ev: ServerEvent;
        try {
          ev = JSON.parse(msg.data as string) as ServerEvent;
        } catch {
          return;
        }

        // A gap means we missed an event — ask for a fresh snapshot rather than
        // rendering a book we know is wrong.
        if ("seq" in ev && ev.t !== "snapshot" && ev.seq > seq.current + 1) {
          ws.send(JSON.stringify({ t: "resync", fromSeq: seq.current } satisfies ClientCommand));
          return;
        }
        if ("seq" in ev) seq.current = ev.seq;

        setState((s) => apply(s, ev));
      };

      ws.onclose = () => {
        // Only the current socket may trigger a reconnect. Without this, a
        // socket closed by an effect cleanup races the new one and both end up
        // live, with socket.current pointing at whichever won.
        if (!alive.current || socket.current !== ws) return;
        setState((s) => ({ ...s, connection: "reconnecting" }));
        // Backoff, capped. Phones drop the socket constantly on screen-lock, so
        // this path is the normal case, not the exceptional one.
        const delay = Math.min(500 * 2 ** attempt.current, 5000);
        attempt.current += 1;
        setTimeout(connect, delay);
      };

      ws.onerror = () => ws.close();
    };

    connect();
    return () => {
      alive.current = false;
      socket.current?.close();
    };
  }, [url]);

  const send = useCallback((cmd: ClientCommand) => {
    const ws = socket.current;
    if (ws && ws.readyState === WebSocket.OPEN) ws.send(JSON.stringify(cmd));
  }, []);

  return useMemo(() => ({ state, send }), [state, send]);
}

/** Clears a rejection banner a couple of seconds after it lands. */
export function useTransientReject(reject: RejectNotice | null) {
  const [shown, setShown] = useState<RejectNotice | null>(null);
  useEffect(() => {
    if (!reject) return;
    setShown(reject);
    const t = setTimeout(() => setShown(null), 2600);
    return () => clearTimeout(t);
  }, [reject]);
  return shown;
}
