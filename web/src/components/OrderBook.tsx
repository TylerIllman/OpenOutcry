import type { BookState, Order } from "../protocol";
import { formatPrice } from "../protocol";

interface Props {
  book: BookState;
  tickSize: number | null;
  /** "board" is the projector; "compact" is the top of a phone screen. */
  variant: "board" | "compact";
  /** Highlights your own resting orders. */
  youId?: string | undefined;
  depth?: number;
}

function Side({
  orders,
  side,
  tickSize,
  youId,
  depth,
}: {
  orders: Order[];
  side: "bid" | "offer";
  tickSize: number | null;
  youId?: string | undefined;
  depth: number;
}) {
  const rows = orders.slice(0, depth);
  return (
    <ol className={`ladder ladder--${side}`}>
      {rows.map((o, i) => (
        <li
          key={o.id}
          className={[
            "ladder__row",
            i === 0 ? "ladder__row--best" : "",
            o.playerId === youId ? "ladder__row--mine" : "",
          ].join(" ")}
        >
          <span className="ladder__name">{o.playerName}</span>
          <span className="ladder__price">{formatPrice(o.price, tickSize)}</span>
        </li>
      ))}
      {rows.length === 0 && <li className="ladder__row ladder__row--empty">—</li>}
    </ol>
  );
}

export function OrderBook({ book, tickSize, variant, youId, depth = 6 }: Props) {
  const bid = book.bids[0];
  const offer = book.offers[0];
  const spread = bid && offer ? offer.price - bid.price : null;

  return (
    <section className={`book book--${variant}`}>
      <header className="book__head">
        <span className="book__label book__label--bid">Bid</span>
        <span className="book__spread">
          {spread === null ? "no market" : `${formatPrice(spread, tickSize)} wide`}
        </span>
        <span className="book__label book__label--offer">Offer</span>
      </header>

      <div className="book__body">
        <Side orders={book.bids} side="bid" tickSize={tickSize} youId={youId} depth={depth} />
        <Side orders={book.offers} side="offer" tickSize={tickSize} youId={youId} depth={depth} />
      </div>

      <footer className="book__touch">
        <span className="touch touch--bid">{bid ? formatPrice(bid.price, tickSize) : "–"}</span>
        <span className="touch__sep">/</span>
        <span className="touch touch--offer">
          {offer ? formatPrice(offer.price, tickSize) : "–"}
        </span>
      </footer>
    </section>
  );
}
