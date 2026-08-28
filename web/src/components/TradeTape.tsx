import type { Trade } from "../protocol";
import { formatPrice } from "../protocol";

interface Props {
  trades: Trade[];
  tickSize: number | null;
  variant: "board" | "compact";
  /** When set, only trades involving this player are shown. */
  onlyPlayerId?: string | undefined;
  limit?: number;
}

const time = (ts: number) =>
  new Date(ts).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit", second: "2-digit" });

export function TradeTape({ trades, tickSize, variant, onlyPlayerId, limit = 40 }: Props) {
  const filtered = onlyPlayerId
    ? trades.filter((t) => t.buyerId === onlyPlayerId || t.sellerId === onlyPlayerId)
    : trades;
  const rows = filtered.slice(-limit).reverse();

  return (
    <section className={`tape tape--${variant}`}>
      <h2 className="panel__title">{onlyPlayerId ? "Your fills" : "Trades"}</h2>
      {rows.length === 0 && <p className="tape__empty">Nothing has traded yet.</p>}
      <ol className="tape__list">
        {rows.map((t) => {
          const youBought = onlyPlayerId && t.buyerId === onlyPlayerId;
          const youSold = onlyPlayerId && t.sellerId === onlyPlayerId;
          return (
            <li key={t.id} className="tape__row">
              <span className="tape__time">{time(t.ts)}</span>
              <span className="tape__text">
                {onlyPlayerId ? (
                  <>
                    <b className={youBought ? "buy" : "sell"}>
                      {youBought && youSold ? "Self" : youBought ? "Bought" : "Sold"}
                    </b>{" "}
                    at
                  </>
                ) : (
                  <>
                    <b>{t.buyerName}</b> bought from <b>{t.sellerName}</b> at
                  </>
                )}
              </span>
              <span className={`tape__price ${t.aggressor === "buy" ? "buy" : "sell"}`}>
                {formatPrice(t.price, tickSize)}
              </span>
              {t.selfTrade && (
                <span className="tag tag--self" title="Buyer and seller are the same player">
                  self
                </span>
              )}
            </li>
          );
        })}
      </ol>
    </section>
  );
}
