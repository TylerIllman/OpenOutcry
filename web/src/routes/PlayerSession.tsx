import { useEffect, useState } from "react";
import { useNavigate, useParams } from "react-router-dom";
import { OrderBook } from "../components/OrderBook";
import { TradeTape } from "../components/TradeTape";
import { ConnectionDot, Toast } from "../components/Bits";
import { useSession, useTransientReject } from "../useSession";
import { loadPlayerIdentity } from "../storage";
import { formatPrice } from "../protocol";

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

  if (!identity) return null;

  const { session, book, trades, you } = state;
  const phase = session?.phase ?? "lobby";
  const tick = session?.tickSize ?? null;
  const myOrders = [...book.bids, ...book.offers].filter(
    (o) => o.playerId === identity.playerId,
  );
  const bestBid = book.bids[0];
  const bestOffer = book.offers[0];
  const position = you?.position ?? 0;

  return (
    <main className="player">
      <header className="player__head">
        <div>
          <p className="player__question">{session?.question ?? "…"}</p>
          {session && <p className="player__unit">in {session.unit}</p>}
        </div>
        <ConnectionDot connection={state.connection} />
      </header>

      {phase === "lobby" && (
        <section className="player__waiting">
          <h2>You're in, {identity.name}.</h2>
          <p>Waiting for the host to open trading.</p>
        </section>
      )}

      {phase === "settled" && state.settlement && (
        <section className="player__waiting">
          <p className="results__eyebrow">The answer was</p>
          <p className="results__value">{formatPrice(state.settlement.trueValue, tick)}</p>
          {(() => {
            const mine = state.settlement.results.find((r) => r.playerId === identity.playerId);
            if (!mine) return null;
            return (
              <p className={`player__pnl ${mine.pnl >= 0 ? "buy" : "sell"}`}>
                {mine.pnl >= 0 ? "+" : ""}
                {formatPrice(mine.pnl, tick)}
              </p>
            );
          })()}
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
                className="quote quote--bid"
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
                className="quote quote--offer"
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
                    Pull
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
