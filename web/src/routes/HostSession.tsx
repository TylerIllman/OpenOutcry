import { useEffect, useRef, useState } from "react";
import { useParams } from "react-router-dom";
import { OrderBook } from "../components/OrderBook";
import { TradeTape } from "../components/TradeTape";
import { JoinPanel } from "../components/JoinPanel";
import { PriceChart } from "../components/PriceChart";
import { ConnectionDot, MuteButton, PhasePill, Toast } from "../components/Bits";
import { useSession, useTransientReject } from "../useSession";
import { loadHostToken } from "../storage";
import { exportUrl } from "../api";
import { formatPrice } from "../protocol";
import { sfx } from "../sfx";

export function HostSession() {
  const { code = "" } = useParams();
  const hostToken = loadHostToken(code);
  const { state, send } = useSession({ code, hostToken });
  const reject = useTransientReject(state.reject);
  const [trueValue, setTrueValue] = useState("");
  const [showBots, setShowBots] = useState(false);
  const [botRate, setBotRate] = useState(6);
  const [buyBias, setBuyBias] = useState(50);

  const heard = useRef<number | null>(null);
  useEffect(() => {
    if (heard.current !== null && state.trades.length > heard.current) sfx("tick");
    heard.current = state.trades.length;
  }, [state.trades.length]);

  const lastPhase = state.session?.phase;
  useEffect(() => {
    if (lastPhase === "open") sfx("open");
    if (lastPhase === "closed") sfx("close");
    if (lastPhase === "settled") sfx("settle");
  }, [lastPhase]);

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
  const worst = Math.max(1, ...(settlement?.results ?? []).map((r) => Math.abs(r.pnl)));

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
          <MuteButton />
          <button
            className="gear"
            onClick={() => setShowBots((v) => !v)}
            aria-label="Bot controls"
            aria-expanded={showBots}
            title="Bot controls"
          >
            ⚙
          </button>
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

          <PriceChart trades={trades} trueValue={settlement.trueValue} tickSize={tick} />

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
                  <span className="leaderboard__bar" aria-hidden="true">
                    <i
                      className={r.pnl >= 0 ? "buy" : "sell"}
                      style={{
                        width: `${Math.min(100, (Math.abs(r.pnl) / worst) * 100)}%`,
                        marginLeft: r.pnl >= 0 ? "50%" : undefined,
                        marginRight: r.pnl < 0 ? "50%" : undefined,
                      }}
                    />
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

      {showBots && (
        <BotControls
          rate={botRate}
          setRate={(v) => {
            setBotRate(v);
            send({ t: "setBotFlow", ordersPerMinute: v, buyBias: buyBias / 100 });
          }}
          bias={buyBias}
          setBias={(v) => {
            setBuyBias(v);
            send({ t: "setBotFlow", ordersPerMinute: botRate, buyBias: v / 100 });
          }}
          bots={players.filter((p) => p.isBot).length}
          onAdd={() => send({ t: "addBot" })}
          onRemove={() => send({ t: "removeBots" })}
          onClose={() => setShowBots(false)}
        />
      )}

      <Toast message={reject?.message ?? state.error} />
    </main>
  );
}

interface BotControlsProps {
  rate: number;
  setRate: (v: number) => void;
  bias: number;
  setBias: (v: number) => void;
  bots: number;
  onAdd: () => void;
  onRemove: () => void;
  onClose: () => void;
}

/**
 * Hidden behind the gear, and floating rather than inline, so the projected
 * board never shows that the flow is synthetic. The room should not be able to
 * see how many bots are in, or that a dial exists at all.
 */
function BotControls({
  rate,
  setRate,
  bias,
  setBias,
  bots,
  onAdd,
  onRemove,
  onClose,
}: BotControlsProps) {
  return (
    <aside className="bots" role="dialog" aria-label="Bot controls">
      <header className="bots__head">
        <h2 className="panel__title">
          Bots <span className="count">{bots}</span>
        </h2>
        <button className="gear" onClick={onClose} aria-label="Close">
          ✕
        </button>
      </header>

      <p className="bots__lead">
        Bots only <b>take prices you make</b>. They never quote and have no view
        on value — they buy and sell at random, at the rate you set.
      </p>

      <label className="bots__field">
        <span className="field__label">
          Rate <b className="bots__value">{rate}</b> / min each
        </span>
        <input className="slider" type="range" min={0} max={30} step={1}
               value={rate} onChange={(e) => setRate(Number(e.target.value))} />
        <span className="field__hint">
          {rate === 0 ? "Idle" : `About ${(rate * bots).toFixed(0)} orders a minute in total`}
        </span>
      </label>

      <label className="bots__field">
        <span className="field__label">
          Direction <b className="bots__value">{bias}% buy</b>
        </span>
        <input className="slider" type="range" min={0} max={100} step={5}
               value={bias} onChange={(e) => setBias(Number(e.target.value))} />
        <span className="field__hint">
          {bias === 50
            ? "Even both ways"
            : bias > 50
              ? "Leans towards buying — lifts your offers"
              : "Leans towards selling — hits your bids"}
        </span>
      </label>

      <div className="bots__actions">
        <button className="btn" onClick={onAdd}>
          Add bot
        </button>
        {bots > 0 && (
          <button className="btn btn--danger" onClick={onRemove}>
            Remove all
          </button>
        )}
      </div>
    </aside>
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
