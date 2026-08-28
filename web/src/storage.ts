/**
 * Tokens live in localStorage so a screen-lock or a refresh restores the same
 * seat rather than creating a new trader. Scoped per session code.
 */
const key = (kind: "host" | "player", code: string) => `oo:${kind}:${code.toUpperCase()}`;

export interface PlayerIdentity {
  playerId: string;
  playerToken: string;
  name: string;
}

export const saveHostToken = (code: string, token: string) =>
  localStorage.setItem(key("host", code), token);

export const loadHostToken = (code: string): string | null =>
  localStorage.getItem(key("host", code));

export const savePlayerIdentity = (code: string, id: PlayerIdentity) =>
  localStorage.setItem(key("player", code), JSON.stringify(id));

export function loadPlayerIdentity(code: string): PlayerIdentity | null {
  const raw = localStorage.getItem(key("player", code));
  if (!raw) return null;
  try {
    return JSON.parse(raw) as PlayerIdentity;
  } catch {
    return null;
  }
}

export const clearPlayerIdentity = (code: string) =>
  localStorage.removeItem(key("player", code));
