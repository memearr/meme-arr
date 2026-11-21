// Transitions between the sections of a sequence: how a section comes in over the one before it.
// Pure logic, no React; the limits are the server's (see src/export/transition.rs).

/** The kinds on offer, with the server's names. */
export const KINDS = [
  { id: "fade", label: "Cross-fade" },
  { id: "dissolve", label: "Dissolve" },
  { id: "fade_black", label: "Through black" },
  { id: "fade_white", label: "Through white" },
  { id: "wipe_left", label: "Wipe left" },
  { id: "wipe_right", label: "Wipe right" },
  { id: "wipe_up", label: "Wipe up" },
  { id: "wipe_down", label: "Wipe down" },
  { id: "slide_left", label: "Slide left" },
  { id: "slide_right", label: "Slide right" },
  { id: "slide_up", label: "Slide up" },
  { id: "slide_down", label: "Slide down" },
  { id: "circle_open", label: "Circle open" },
  { id: "circle_close", label: "Circle close" },
  { id: "radial", label: "Radial" },
  { id: "pixelize", label: "Pixelate" },
];

export const MIN_MS = 100;
export const MAX_MS = 3000;
export const DEFAULT_MS = 1000;

const lengthOf = (c) => Math.max(0, (c?.end_ms ?? 0) - (c?.start_ms ?? 0));
const inMs = (c) => (c?.transition ? c.transition.ms : 0);

/**
 * The longest the transition into section `i` of `sequence` may be: the server's limit, and short enough
 * that this section still has room for the transition out of it, and the section before it for the
 * transition into that. Under the shortest transition (including the first section) there can be none.
 */
export function transitionLimit(sequence, i) {
  if (i <= 0 || i >= sequence.length) return 0;
  const room = (c, other) => lengthOf(c) - other;
  const limit = Math.min(
    MAX_MS,
    room(sequence[i], inMs(sequence[i + 1])),
    room(sequence[i - 1], i - 1 > 0 ? inMs(sequence[i - 1]) : 0)
  );
  return limit >= MIN_MS ? limit : 0;
}

/** A transition kept inside what section `i` may have, or `null` when it can't have one. */
export function adjustTransition(sequence, i, transition) {
  const limit = transitionLimit(sequence, i);
  if (!transition || !KINDS.some((k) => k.id === transition.kind) || limit === 0) return null;
  const ms = Math.round(Number.isFinite(transition.ms) ? transition.ms : DEFAULT_MS);
  return { kind: transition.kind, ms: Math.min(limit, Math.max(MIN_MS, ms)) };
}

/**
 * The transition of each section as the export takes it: none unless `advanced`, and each kept inside what
 * the sections around it leave room for (the one before it first, so a short section can't be asked twice).
 */
export function transitionsForRequest(sequence, advanced) {
  if (!advanced) return sequence.map(() => undefined);
  const settled = sequence.map((c) => ({ ...c }));
  return settled.map((c, i) => {
    const t = adjustTransition(settled, i, c.transition);
    settled[i].transition = t || undefined;
    return t || undefined;
  });
}

/** How much shorter the whole is for its transitions, in ms. */
export function overlapMs(transitions) {
  return transitions.reduce((n, t) => n + (t ? t.ms : 0), 0);
}
