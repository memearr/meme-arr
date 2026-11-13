import { useRef, useState } from "react";
import { copyText, selectContents } from "./clipboard.js";

/**
 * A Copy button. If the browser won't let a page copy (it can happen),
 * it selects the text instead and says to press Ctrl+C, rather than doing nothing.
 * `getTarget` returns the element holding the text, for that fallback.
 */
export default function CopyButton({ text, getTarget, className = "secondary", small = false }) {
  const [state, setState] = useState(null); // null | "copied" | "manual"
  const timer = useRef(null);

  async function copy() {
    clearTimeout(timer.current);
    const ok = await copyText(text);
    if (ok) {
      setState("copied");
    } else {
      selectContents(getTarget?.());
      setState("manual");
    }
    timer.current = setTimeout(() => setState(null), 4000);
  }

  return (
    <>
      <button type="button" className={className + (small ? " small" : "")} onClick={copy}>
        {state === "copied" ? "Copied" : "Copy"}
      </button>
      {state === "manual" && (
        <span className="hint tiny" role="status">
          Selected: press Ctrl+C (⌘C on a Mac) to copy.
        </span>
      )}
    </>
  );
}
