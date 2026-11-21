// The Advanced switch: one tick box that shows the editor's advanced tools. The browser remembers it.
import { useCallback, useState } from "react";
import { browserStorage } from "./editorStorage.js";

export const ADVANCED_KEY = "memearr.advanced";

/** Whether Advanced was left on (anything unreadable counts as off). */
export function loadAdvanced(store) {
  try {
    return store?.getItem(ADVANCED_KEY) === "1";
  } catch (_) {
    return false;
  }
}

export function saveAdvanced(store, on) {
  try {
    store?.setItem(ADVANCED_KEY, on ? "1" : "0");
  } catch (_) {
    /* a convenience only */
  }
}

/** `[advanced, setAdvanced]`: starts as it was left, and keeps what it is set to. */
export function useAdvanced() {
  const [on, setOn] = useState(() => loadAdvanced(browserStorage()));
  const set = useCallback((v) => {
    setOn(!!v);
    saveAdvanced(browserStorage(), !!v);
  }, []);
  return [on, set];
}
