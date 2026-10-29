import { useEffect, useRef, useState } from "react";
import { libraryFilterLabel, toggleLibrary } from "./libraryFilter.js";

/**
 * Pick which libraries a search or list covers. Nothing ticked means all of
 * them. A small checkbox popover, so any subset can be chosen, not just one.
 */
export function LibraryFilter({ all, selected, onChange, className = "" }) {
  const [open, setOpen] = useState(false);
  const box = useRef(null);

  useEffect(() => {
    if (!open) return undefined;
    const away = (e) => {
      if (box.current && !box.current.contains(e.target)) setOpen(false);
    };
    const esc = (e) => e.key === "Escape" && setOpen(false);
    // Close on a click elsewhere, not on mouse-down: closing between mouse-down
    // and mouse-up can move the page under the pointer and swallow that click
    // (e.g. on a "Show more" button further down).
    document.addEventListener("click", away);
    document.addEventListener("keydown", esc);
    return () => {
      document.removeEventListener("click", away);
      document.removeEventListener("keydown", esc);
    };
  }, [open]);

  if (!all || all.length < 2) return null; // nothing to choose between

  const chosen = selected.filter((l) => all.includes(l));
  return (
    <div className={`libfilter ${className}`} ref={box}>
      <button
        type="button"
        className="libfilter-btn filter"
        aria-haspopup="true"
        aria-expanded={open}
        onClick={() => setOpen((o) => !o)}
      >
        {libraryFilterLabel(selected, all)} <span aria-hidden="true">▾</span>
      </button>
      {open && (
        <div className="libfilter-pop" role="group" aria-label="Libraries">
          <label className="libfilter-row">
            <input type="checkbox" checked={chosen.length === 0} onChange={() => onChange([])} />
            All libraries
          </label>
          {all.map((l) => (
            <label key={l} className="libfilter-row">
              <input
                type="checkbox"
                checked={chosen.includes(l)}
                onChange={() => onChange(toggleLibrary(chosen, l, all))}
              />
              {l}
            </label>
          ))}
        </div>
      )}
    </div>
  );
}
