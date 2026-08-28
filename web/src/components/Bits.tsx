import { useState } from "react";
import type { Phase } from "../protocol";
import { isMuted, setMuted, sfx } from "../sfx";

export function PhasePill({ phase }: { phase: Phase }) {
  const label = {
    lobby: "Lobby",
    open: "Trading open",
    closed: "Trading closed",
    settled: "Settled",
  }[phase];
  return <span className={`pill pill--${phase}`}>{label}</span>;
}

export function ConnectionDot({ connection }: { connection: string }) {
  return (
    <span className={`dot dot--${connection}`} title={connection}>
      <i />
      {connection === "live" ? "live" : connection}
    </span>
  );
}

export function MuteButton() {
  const [muted, set] = useState(isMuted);
  return (
    <button
      className="mute"
      aria-pressed={muted}
      aria-label={muted ? "Unmute sound" : "Mute sound"}
      title={muted ? "Sound off" : "Sound on"}
      onClick={() => {
        const next = !muted;
        setMuted(next);
        set(next);
        // Confirm audibly that sound is back, which also satisfies the
        // browser's "audio needs a user gesture" rule.
        if (!next) sfx("tick");
      }}
    >
      {muted ? "🔇" : "🔊"}
    </button>
  );
}

export function Toast({ message }: { message: string | null }) {
  if (!message) return null;
  return <div className="toast">{message}</div>;
}
