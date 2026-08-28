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
  const [anchor, setAnchor] = useState("");

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
                <li key={p.id} className={p.isBot ? "is-bot" : undefined}>
                  {p.name}
                  {p.isBot && " ·bot"}
                </li>
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
            <BotControls
              anchor={anchor}
              setAnchor={setAnchor}
              bots={players.filter((p) => p.isBot).length}
              unit={session?.unit ?? ""}
              onAdd={() => send({ t: "addBot", anchor: Number(anchor) || 0 })}
              onRemove={() => send({ t: "removeBots" })}
            />
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
          <BotControls
            anchor={anchor}
            setAnchor={setAnchor}
            bots={players.filter((p) => p.isBot).length}
            unit={session?.unit ?? ""}
            onAdd={() => send({ t: "addBot", anchor: Number(anchor) || 0 })}
            onRemove={() => send({ t: "removeBots" })}
          />
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

      <Toast message={reject?.message ?? state.error} />
    </main>
  );
}

interface BotControlsProps {
  anchor: string;
  setAnchor: (v: string) => void;
  bots: number;
  unit: string;
  onAdd: () => void;
  onRemove: () => void;
}

/**
 * Bots need somewhere to start. They quote around this number, each with its
 * own randomly offset private opinion of it, so they disagree with each other
 * and end up making a two-sided market rather than all leaning the same way.
 */
function BotControls({ anchor, setAnchor, bots, unit, onAdd, onRemove }: BotControlsProps) {
  return (
    <div className="bots">
      <label className="bots__field">
        <span className="field__label">Bots quote around</span>
        <input
          className="input"
          value={anchor}
          onChange={(e) => setAnchor(e.target.value)}
          placeholder={`rough guess${unit ? ` in ${unit}` : ""}`}
          inputMode="decimal"
        />
      </label>
      <button className="btn" onClick={onAdd} disabled={anchor.trim() === ""}>
        Add bot
      </button>
      {bots > 0 && (
        <>
          <span className="bots__count">{bots} trading</span>
          <button className="btn btn--danger" onClick={onRemove}>
            Remove bots
          </button>
        </>
      )}
    </div>
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
