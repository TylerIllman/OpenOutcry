import { useEffect, useState } from "react";
import { useNavigate, useParams } from "react-router-dom";
import { getSession, joinSession } from "../api";
import { loadPlayerIdentity, savePlayerIdentity } from "../storage";
import type { SessionMeta } from "../protocol";

export function Join() {
  const { code = "" } = useParams();
  const nav = useNavigate();
  const [meta, setMeta] = useState<SessionMeta | null>(null);
  const [name, setName] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  // Already have a seat in this session? Go straight back to it. This is what
  // makes a refresh or a screen-lock harmless.
  useEffect(() => {
    if (loadPlayerIdentity(code)) nav(`/play/${code}`, { replace: true });
  }, [code, nav]);

  useEffect(() => {
    getSession(code)
      .then(setMeta)
      .catch(() => setError("No session with that code."));
  }, [code]);

  async function submit(e: React.FormEvent) {
    e.preventDefault();
    setBusy(true);
    setError(null);
    try {
      const res = await joinSession(code, { name: name.trim() });
      savePlayerIdentity(code, {
        playerId: res.playerId,
        playerToken: res.playerToken,
        name: name.trim(),
      });
      nav(`/play/${code}`);
    } catch {
      setError("Could not join. Check the code and try again.");
      setBusy(false);
    }
  }

  return (
    <main className="page page--narrow">
      <p className="page__eyebrow">{code}</p>
      <h1 className="page__title">{meta ? meta.question : "Joining…"}</h1>
      {meta && <p className="page__lead">Priced in {meta.unit}.</p>}

      <form className="form" onSubmit={submit}>
        <label className="field">
          <span className="field__label">Your name</span>
          <input
            className="input input--lg"
            value={name}
            onChange={(e) => setName(e.target.value)}
            placeholder="Tyler"
            maxLength={16}
            required
            autoFocus
          />
          <span className="field__hint">Everyone sees this on the board.</span>
        </label>

        {error && <p className="error">{error}</p>}

        <button
          className="btn btn--primary btn--lg"
          type="submit"
          disabled={busy || name.trim() === "" || !meta}
        >
          {busy ? "Joining…" : "Take a seat"}
        </button>
      </form>
    </main>
  );
}
