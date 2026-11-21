// The Advanced switch: one tick box that shows the advanced tools. The browser remembers it.
import { useCallback, useEffect, useState } from "react";
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

// Every box on the page that shows the switch, so turning it on in one turns it on in all.
const listeners = new Set();

/** `[advanced, setAdvanced]`: starts as it was left, keeps what it is set to, and follows other boxes. */
export function useAdvanced() {
  const [on, setOn] = useState(() => loadAdvanced(browserStorage()));
  useEffect(() => {
    listeners.add(setOn);
    // it may have changed between the first render and now
    setOn(loadAdvanced(browserStorage()));
    return () => {
      listeners.delete(setOn);
    };
  }, []);
  const set = useCallback((v) => {
    saveAdvanced(browserStorage(), !!v);
    listeners.forEach((fn) => fn(!!v));
  }, []);
  return [on, set];
}
