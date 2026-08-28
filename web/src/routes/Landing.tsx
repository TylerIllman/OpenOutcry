import { useState } from "react";
import { Link, useNavigate } from "react-router-dom";

export function Landing() {
  const nav = useNavigate();
  const [code, setCode] = useState("");

  return (
    <main className="landing">
      <div className="landing__inner">
        <h1 className="landing__title">
          Open<span>Outcry</span>
        </h1>
        <p className="landing__blurb">
          A trading floor for a room full of people. One question, one order book,
          no accounts.
        </p>

        <div className="landing__actions">
          <Link className="btn btn--primary btn--lg" to="/host/new">
            Host a game
          </Link>

          <form
            className="landing__join"
            onSubmit={(e) => {
              e.preventDefault();
              const c = code.trim().toUpperCase();
              if (c) nav(`/join/${c}`);
            }}
          >
            <input
              className="input input--code"
              value={code}
              onChange={(e) => setCode(e.target.value.toUpperCase())}
              placeholder="CODE"
              maxLength={6}
              autoCapitalize="characters"
              autoCorrect="off"
              spellCheck={false}
              aria-label="Session code"
            />
            <button className="btn btn--lg" type="submit" disabled={code.trim().length === 0}>
              Join
            </button>
          </form>
        </div>

        <p className="landing__foot">
          MINE lifts the offer. YOURS hits the bid. Everything is one lot.
        </p>
      </div>
    </main>
  );
}
