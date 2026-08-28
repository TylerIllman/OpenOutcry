import { useState } from "react";
import { useParams } from "react-router-dom";
import { OrderBook } from "../components/OrderBook";
import { TradeTape } from "../components/TradeTape";
import { JoinPanel } from "../components/JoinPanel";
import { ConnectionDot, PhasePill, Toast } from "../components/Bits";
import { useSession, useTransientReject } from "../useSession";
import { loadHostToken } from "../storage";
import { exportUrl } from "../api";
import { formatPrice } from "../protocol";

export function HostSession() {
  const { code = "" } = useParams();
  const hostToken = loadHostToken(code);
  const { state, send } = useSession({ code, hostToken });
  const reject = useTransientReject(state.reject);
  const [trueValue, setTrueValue] = useState("");

  if (!hostToken) {
    return (
      <main className="page page--narrow">
        <h1 className="page__title">Not your session</h1>
        <p className="page__lead">
          The host token for <b>{code}</b> is not on this device. Host controls live
          in the browser that created the session.
        </p>
      </main>
    );
  }

  const { session, players, book, trades, settlement } = state;
  const phase = session?.phase ?? "lobby";
  const tick = session?.tickSize ?? null;
  const last = trades[trades.length - 1];

  return (
    <main className="host">
      <header className="host__head">
        <div className="host__question">
          <h1>{session?.question ?? "…"}</h1>
          {session && <p className="host__unit">priced in {session.unit}</p>}
        </div>
        <div className="host__meta">
          <PhasePill phase={phase} />
          <ConnectionDot connection={state.connection} />
          <span className="host__code">{code}</span>
        </div>
      </header>

      {phase === "lobby" && (
        <section className="host__lobby">
          <JoinPanel code={code} />
          <div className="host__roster">
            <h2 className="panel__title">
              In the room <span className="count">{players.length}</span>
            </h2>
            <ul className="roster">
              {players.map((p) => (
                <li key={p.id}>{p.name}</li>
              ))}
            </ul>
            {players.length === 0 && <p className="tape__empty">Nobody yet.</p>}
            <button
              className="btn btn--primary btn--lg"
              onClick={() => send({ t: "openTrading" })}
              disabled={players.length === 0}
            >
              Open trading
            </button>
            <p className="field__hint">
              Late joiners are allowed, so you do not have to wait for stragglers.
            </p>
          </div>
        </section>
      )}

      {(phase === "open" || phase === "closed") && (
        <section className="host__floor">
          <div className="host__book">
            <OrderBook book={book} tickSize={tick} variant="board" depth={7} />
            <div className="host__stats">
              <Stat label="Last" value={last ? formatPrice(last.price, tick) : "–"} />
              <Stat label="Trades" value={String(trades.length)} />
              <Stat label="Traders" value={String(players.length)} />
              <Stat label="Limit" value={`±${session?.positionLimit ?? "–"}`} />
            </div>
          </div>
          <div className="host__tape">
            <TradeTape trades={trades} tickSize={tick} variant="board" limit={18} />
          </div>
        </section>
      )}

      {phase === "open" && (
        <footer className="host__controls">
          <button className="btn btn--danger btn--lg" onClick={() => send({ t: "closeTrading" })}>
            Close trading
          </button>
        </footer>
      )}

      {phase === "closed" && (
        <footer className="host__controls host__controls--settle">
          <form
            className="settle"
            onSubmit={(e) => {
              e.preventDefault();
              const v = Number(trueValue);
              if (Number.isFinite(v)) send({ t: "settle", trueValue: v });
            }}
          >
            <label className="field">
              <span className="field__label">
                True value {session ? `(${session.unit})` : ""}
              </span>
              <input
                className="input input--lg"
                value={trueValue}
                onChange={(e) => setTrueValue(e.target.value)}
                placeholder="47.5"
                inputMode="decimal"
                autoFocus
              />
            </label>
            <button className="btn btn--primary btn--lg" type="submit" disabled={trueValue === ""}>
              Reveal &amp; settle
            </button>
          </form>
        </footer>
      )}

      {phase === "settled" && settlement && (
        <section className="results">
          <p className="results__eyebrow">The answer was</p>
          <p className="results__value">
            {formatPrice(settlement.trueValue, tick)}
            <span className="results__unit"> {session?.unit}</span>
          </p>

          <ol className="leaderboard">
            {[...settlement.results]
              .sort((a, b) => b.pnl - a.pnl)
              .map((r, i) => (
                <li key={r.playerId} className="leaderboard__row">
                  <span className="leaderboard__rank">{i + 1}</span>
                  <span className="leaderboard__name">{r.name}</span>
                  <span className="leaderboard__pos">
                    {r.position === 0
                      ? "flat"
                      : r.position > 0
                        ? `long ${r.position}`
                        : `short ${-r.position}`}
                  </span>
                  <span className={`leaderboard__pnl ${r.pnl >= 0 ? "buy" : "sell"}`}>
                    {r.pnl >= 0 ? "+" : ""}
                    {formatPrice(r.pnl, tick)}
                  </span>
                </li>
              ))}
          </ol>

          <a className="btn" href={exportUrl(code, hostToken)} download>
            Download the tape (CSV)
          </a>
        </section>
      )}

      <Toast message={reject?.message ?? state.error} />
    </main>
  );
}

function Stat({ label, value }: { label: string; value: string }) {
  return (
    <div className="stat">
      <span className="stat__label">{label}</span>
      <span className="stat__value">{value}</span>
    </div>
  );
}
