/**
 * Sound effects, synthesised with the Web Audio API.
 *
 * No audio files: a handful of oscillator blips are a few hundred bytes of code
 * rather than a few hundred kilobytes of assets, and they load instantly on a
 * pub's wifi.
 *
 * Browsers refuse to start audio until the user has interacted with the page,
 * so the context is created lazily on the first sound and resumed if the
 * browser suspended it. Every player screen starts with a tap, so by the time
 * anything needs to make a noise the gesture has happened.
 */

export type Sound = "buy" | "sell" | "reject" | "open" | "close" | "settle" | "tick";

const STORAGE_KEY = "oo:muted";

let ctx: AudioContext | null = null;
let muted = false;

try {
  muted = localStorage.getItem(STORAGE_KEY) === "1";
} catch {
  // Private mode, or storage disabled. Default to audible.
}

export function isMuted(): boolean {
  return muted;
}

export function setMuted(value: boolean): void {
  muted = value;
  try {
    localStorage.setItem(STORAGE_KEY, value ? "1" : "0");
  } catch {
    // Not being able to remember the preference is not worth failing over.
  }
}

function audio(): AudioContext | null {
  if (typeof window === "undefined") return null;
  try {
    if (!ctx) {
      const Ctor = window.AudioContext ?? (window as never as { webkitAudioContext: typeof AudioContext }).webkitAudioContext;
      if (!Ctor) return null;
      ctx = new Ctor();
    }
    if (ctx.state === "suspended") void ctx.resume();
    return ctx;
  } catch {
    return null;
  }
}

interface Note {
  freq: number;
  /** Seconds from now. */
  at: number;
  /** Seconds. */
  length: number;
  type?: OscillatorType;
  gain?: number;
}

function play(notes: Note[]): void {
  const c = audio();
  if (!c) return;

  for (const note of notes) {
    const osc = c.createOscillator();
    const amp = c.createGain();
    osc.type = note.type ?? "sine";
    osc.frequency.value = note.freq;

    const start = c.currentTime + note.at;
    const peak = note.gain ?? 0.18;

    // A quick attack and an exponential tail. A square edge on either end
    // produces an audible click.
    amp.gain.setValueAtTime(0.0001, start);
    amp.gain.exponentialRampToValueAtTime(peak, start + 0.012);
    amp.gain.exponentialRampToValueAtTime(0.0001, start + note.length);

    osc.connect(amp).connect(c.destination);
    osc.start(start);
    osc.stop(start + note.length + 0.02);
  }
}

const SOUNDS: Record<Sound, Note[]> = {
  // You bought: two notes going up.
  buy: [
    { freq: 587.33, at: 0, length: 0.09 },
    { freq: 880.0, at: 0.07, length: 0.13 },
  ],
  // You sold: the same shape going down, so you can tell them apart without
  // looking at the screen.
  sell: [
    { freq: 587.33, at: 0, length: 0.09 },
    { freq: 392.0, at: 0.07, length: 0.13 },
  ],
  reject: [{ freq: 155.56, at: 0, length: 0.16, type: "square", gain: 0.1 }],
  open: [
    { freq: 659.25, at: 0, length: 0.16, type: "triangle" },
    { freq: 987.77, at: 0.1, length: 0.28, type: "triangle" },
  ],
  close: [
    { freq: 493.88, at: 0, length: 0.16, type: "triangle" },
    { freq: 329.63, at: 0.1, length: 0.3, type: "triangle" },
  ],
  // The reveal. A major chord, held.
  settle: [
    { freq: 523.25, at: 0, length: 0.7, gain: 0.14 },
    { freq: 659.25, at: 0.06, length: 0.7, gain: 0.14 },
    { freq: 783.99, at: 0.12, length: 0.8, gain: 0.14 },
  ],
  // Someone else traded. Quiet, because on the host screen this fires often.
  tick: [{ freq: 1200, at: 0, length: 0.035, gain: 0.05 }],
};

export function sfx(sound: Sound): void {
  if (muted) return;
  play(SOUNDS[sound]);
}
