import { useState } from "react";
import { useNavigate } from "react-router-dom";
import { createSession } from "../api";
import { saveHostToken } from "../storage";

export function CreateSession() {
  const nav = useNavigate();
  const [question, setQuestion] = useState("");
  const [unit, setUnit] = useState("");
  const [tick, setTick] = useState("");
  const [limit, setLimit] = useState("10");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  async function submit(e: React.FormEvent) {
    e.preventDefault();
    setBusy(true);
    setError(null);
    try {
      const res = await createSession({
        question: question.trim(),
        unit: unit.trim(),
        tickSize: tick.trim() === "" ? null : Number(tick),
        positionLimit: Number(limit),
      });
      saveHostToken(res.code, res.hostToken);
      nav(`/host/${res.code}`);
    } catch (err) {
      setError(err instanceof Error ? err.message : "Could not create the session");
      setBusy(false);
    }
  }

  return (
    <main className="page page--narrow">
      <h1 className="page__title">New session</h1>
      <p className="page__lead">
        You host and run the screen. You do not trade — you type the answer at the
        end, so you cannot have a position.
      </p>

      <form className="form" onSubmit={submit}>
        <label className="field">
          <span className="field__label">Question</span>
          <input
            className="input"
            value={question}
            onChange={(e) => setQuestion(e.target.value)}
            placeholder="How many bouncy balls are there in the world?"
            required
          />
        </label>

        <label className="field">
          <span className="field__label">Unit</span>
          <input
            className="input"
            value={unit}
            onChange={(e) => setUnit(e.target.value)}
            placeholder="millions of balls"
            required
          />
          <span className="field__hint">
            Make the question tradeable. Nobody wants to type 1,300,000,000,000 into
            a phone — pick a unit that puts sensible prices in the tens.
          </span>
        </label>

        <div className="form__row">
          <label className="field">
            <span className="field__label">Tick size</span>
            <input
              className="input"
              value={tick}
              onChange={(e) => setTick(e.target.value)}
              placeholder="any"
              inputMode="decimal"
            />
            <span className="field__hint">Blank means any price.</span>
          </label>

          <label className="field">
            <span className="field__label">Position limit</span>
            <input
              className="input"
              value={limit}
              onChange={(e) => setLimit(e.target.value)}
              inputMode="numeric"
              required
            />
            <span className="field__hint">Counts resting orders too.</span>
          </label>
        </div>

        {error && <p className="error">{error}</p>}

        <button className="btn btn--primary btn--lg" type="submit" disabled={busy}>
          {busy ? "Creating…" : "Create session"}
        </button>
      </form>
    </main>
  );
}
