import { useEffect, useRef, useState } from "react";
import { useNavigate, useParams } from "react-router-dom";
import { OrderBook } from "../components/OrderBook";
import { TradeTape } from "../components/TradeTape";
import { PriceChart } from "../components/PriceChart";
import { ConnectionDot, MuteButton, Toast } from "../components/Bits";
import { useSession, useTransientReject } from "../useSession";
import { loadPlayerIdentity } from "../storage";
import { formatPrice } from "../protocol";
import { sfx } from "../sfx";

export function PlayerSession() {
  const { code = "" } = useParams();
  const nav = useNavigate();
  const identity = loadPlayerIdentity(code);

  useEffect(() => {
    if (!identity) nav(`/join/${code}`, { replace: true });
  }, [identity, code, nav]);

  const { state, send } = useSession({ code, playerToken: identity?.playerToken });
  const reject = useTransientReject(state.reject);

  const [bidPrice, setBidPrice] = useState("");
  const [offerPrice, setOfferPrice] = useState("");

  const { session, book, trades, you } = state;
  const phase = session?.phase ?? "lobby";
  const myId = identity?.playerId;

  // Your own fills, so you know you were hit without watching the screen.
  const heard = useRef<number | null>(null);
  useEffect(() => {
    if (!myId) return;
    const mine = trades.filter((t) => t.buyerId === myId || t.sellerId === myId);
    if (heard.current !== null && mine.length > heard.current) {
      const latest = mine[mine.length - 1];
      if (latest) sfx(latest.buyerId === myId ? "buy" : "sell");
    }
    heard.current = mine.length;
  }, [trades, myId]);

  useEffect(() => {
    if (state.reject) sfx("reject");
  }, [state.reject]);

  useEffect(() => {
    if (phase === "open") sfx("open");
    if (phase === "closed") sfx("close");
    if (phase === "settled") sfx("settle");
  }, [phase]);

  if (!identity) return null;

  const tick = session?.tickSize ?? null;
  const myOrders = [...book.bids, ...book.offers].filter((o) => o.playerId === identity.playerId);
  const bestBid = book.bids[0];
  const bestOffer = book.offers[0];
  const position = you?.position ?? 0;
  const mine = state.settlement?.results.find((r) => r.playerId === identity.playerId);

  const cancelAll = () => myOrders.forEach((o) => send({ t: "cancelOrder", orderId: o.id }));

  return (
    <main className="player">
      <header className="player__head">
        <div>
          <p className="player__question">{session?.question ?? "…"}</p>
          {session && <p className="player__unit">in {session.unit}</p>}
        </div>
        <div className="player__head-right">
          <ConnectionDot connection={state.connection} />
          <MuteButton />
        </div>
      </header>

      {phase === "lobby" && (
        <section className="player__waiting">
          <h2>You're in, {identity.name}.</h2>
          <p>Waiting for the host to open trading.</p>
        </section>
      )}

      {phase === "settled" && state.settlement && (
        <section className="player__result">
          <p className="results__eyebrow">The answer was</p>
          <p className="results__value">{formatPrice(state.settlement.trueValue, tick)}</p>
          {mine && (
            <>
              <p className={`player__pnl ${mine.pnl >= 0 ? "buy" : "sell"}`}>
                {mine.pnl >= 0 ? "+" : ""}
                {formatPrice(mine.pnl, tick)}
              </p>
              <p className="player__pnl-sub">
                {mine.position === 0
                  ? "finished flat"
                  : mine.position > 0
                    ? `finished long ${mine.position}`
                    : `finished short ${-mine.position}`}
                {" · "}cash {mine.cash >= 0 ? "+" : ""}
                {formatPrice(mine.cash, tick)}
              </p>
            </>
          )}
          <PriceChart trades={trades} trueValue={state.settlement.trueValue} tickSize={tick} />
          <TradeTape
            trades={trades}
            tickSize={tick}
            variant="compact"
            onlyPlayerId={identity.playerId}
            limit={20}
          />
        </section>
      )}

      {(phase === "open" || phase === "closed") && (
        <>
          <OrderBook
            book={book}
            tickSize={tick}
            variant="compact"
            youId={identity.playerId}
            depth={4}
          />

          <div className="position">
            <span className="position__label">Position</span>
            <span className="position__value">
              {position === 0
                ? "flat"
                : position > 0
                  ? `long ${position}`
                  : `short ${-position}`}
            </span>
            <span className="position__limit">limit ±{session?.positionLimit ?? "–"}</span>
          </div>

          <fieldset className="controls" disabled={phase !== "open"}>
            <div className="controls__take">
              <button
                className="btn btn--yours"
                onClick={() => send({ t: "take", direction: "sell" })}
                disabled={!bestBid}
              >
                <span className="btn__word">YOURS</span>
                <span className="btn__sub">
                  sell {bestBid ? formatPrice(bestBid.price, tick) : "–"}
                </span>
              </button>
              <button
                className="btn btn--mine"
                onClick={() => send({ t: "take", direction: "buy" })}
                disabled={!bestOffer}
              >
                <span className="btn__word">MINE</span>
                <span className="btn__sub">
                  buy {bestOffer ? formatPrice(bestOffer.price, tick) : "–"}
                </span>
              </button>
            </div>

            <div className="controls__quote">
              <form
                className="quote"
                onSubmit={(e) => {
                  e.preventDefault();
                  const p = Number(bidPrice);
                  if (Number.isFinite(p) && bidPrice !== "") {
                    send({ t: "placeOrder", side: "bid", price: p });
                    setBidPrice("");
                  }
                }}
              >
                <input
                  className="input input--quote"
                  value={bidPrice}
                  onChange={(e) => setBidPrice(e.target.value)}
                  placeholder="price"
                  inputMode="decimal"
                  aria-label="Bid price"
                />
                <button className="btn btn--bid" type="submit">
                  BID
                </button>
              </form>

              <form
                className="quote"
                onSubmit={(e) => {
                  e.preventDefault();
                  const p = Number(offerPrice);
                  if (Number.isFinite(p) && offerPrice !== "") {
                    send({ t: "placeOrder", side: "offer", price: p });
                    setOfferPrice("");
                  }
                }}
              >
                <input
                  className="input input--quote"
                  value={offerPrice}
                  onChange={(e) => setOfferPrice(e.target.value)}
                  placeholder="price"
                  inputMode="decimal"
                  aria-label="Offer price"
                />
                <button className="btn btn--offer" type="submit">
                  OFFER
                </button>
              </form>
            </div>
          </fieldset>

          <section className="my-orders">
            <h2 className="panel__title">
              Working <span className="count">{myOrders.length}</span>
              {myOrders.length > 1 && (
                <button className="btn btn--tiny my-orders__all" onClick={cancelAll}>
                  Cancel all
                </button>
              )}
            </h2>
            {myOrders.length === 0 && <p className="tape__empty">No resting orders.</p>}
            <ul className="my-orders__list">
              {myOrders.map((o) => (
                <li key={o.id} className={`my-orders__row my-orders__row--${o.side}`}>
                  <span className="my-orders__side">{o.side === "bid" ? "Bid" : "Offer"}</span>
                  <span className="my-orders__price">{formatPrice(o.price, tick)}</span>
                  <button
                    className="btn btn--tiny"
                    onClick={() => send({ t: "cancelOrder", orderId: o.id })}
                  >
                    Cancel
                  </button>
                </li>
              ))}
            </ul>
          </section>

          <TradeTape
            trades={trades}
            tickSize={tick}
            variant="compact"
            onlyPlayerId={identity.playerId}
            limit={12}
          />
        </>
      )}

      <Toast message={reject?.message ?? state.error} />
    </main>
  );
}
