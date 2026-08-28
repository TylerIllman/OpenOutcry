import type { Phase } from "../protocol";

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

export function Toast({ message }: { message: string | null }) {
  if (!message) return null;
  return <div className="toast">{message}</div>;
}
