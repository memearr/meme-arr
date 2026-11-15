// Small browser helpers the editor uses: storage, remembered volume, and which keys are typing.

/** localStorage, or null where the browser won't give it out. */
export function browserStorage() {
  try {
    return window.localStorage;
  } catch (_) {
    return null;
  }
}

export function loadVolume(store) {
  try {
    const v = JSON.parse(store?.getItem("memearr.volume") || "null");
    if (v && typeof v.level === "number") return { level: Math.min(1, Math.max(0, v.level)), muted: !!v.muted };
  } catch (_) {
    /* fall through */
  }
  return { level: 0.7, muted: false };
}
export function saveVolume(store, v) {
  try {
    store?.setItem("memearr.volume", JSON.stringify(v));
  } catch (_) {
    /* a convenience only */
  }
}

/** True when a key press is meant for a text box, not for the editor's shortcuts. */
export function typingInto(el) {
  if (!el) return false;
  const tag = el.tagName;
  return tag === "INPUT" || tag === "TEXTAREA" || tag === "SELECT" || el.isContentEditable;
}

