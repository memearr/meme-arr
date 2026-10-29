import { useEffect, useRef, useState } from "react";
import { LibraryFilter } from "./LibraryFilter.jsx";
import { FILTER_FIELDS, describeFilter, makeFilter } from "./searchScope.js";

/** "+ Filter": a small form to add a filter (show, episode, movie, season, only movies/episodes). */
function AddFilter({ onAdd }) {
  const [open, setOpen] = useState(false);
  const [field, setField] = useState("show");
  const [value, setValue] = useState("");
  const [negate, setNegate] = useState(false);
  const box = useRef(null);
  const meta = FILTER_FIELDS.find((f) => f.id === field);

  useEffect(() => {
    if (!open) return undefined;
    const away = (e) => {
      if (box.current && !box.current.contains(e.target)) setOpen(false);
    };
    const esc = (e) => e.key === "Escape" && setOpen(false);
    document.addEventListener("click", away);
    document.addEventListener("keydown", esc);
    return () => {
      document.removeEventListener("click", away);
      document.removeEventListener("keydown", esc);
    };
  }, [open]);

  const filter = makeFilter(field, value, negate);

  function submit(e) {
    e.preventDefault();
    if (!filter) return;
    onAdd(filter);
    setValue("");
    setNegate(false);
    setOpen(false);
  }

  return (
    <div className="addfilter" ref={box}>
      <button
        type="button"
        className="secondary small addfilter-btn"
        aria-haspopup="true"
        aria-expanded={open}
        onClick={() => setOpen((o) => !o)}
      >
        + Filter
      </button>
      {open && (
        <form className="addfilter-pop" onSubmit={submit} aria-label="Add a filter">
          <label>
            <span className="muted">Narrow to</span>
            <select value={field} onChange={(e) => setField(e.target.value)}>
              {FILTER_FIELDS.map((f) => (
                <option key={f.id} value={f.id}>
                  {f.label}
                </option>
              ))}
            </select>
          </label>
          {!meta.bare && (
            <label>
              <span className="muted">Text</span>
              <input
                autoFocus
                value={value}
                placeholder={meta.placeholder}
                inputMode={field === "season" ? "numeric" : undefined}
                onChange={(e) => setValue(e.target.value)}
              />
            </label>
          )}
          <label className="addfilter-not">
            <input type="checkbox" checked={negate} onChange={(e) => setNegate(e.target.checked)} />
            Exclude these instead
          </label>
          <div className="addfilter-actions">
            <button type="submit" disabled={!filter}>
              Add filter
            </button>
          </div>
          <p className="hint tiny">
            Then search for dialogue in the box: it is searched inside what's left. You can also type these
            into the search box (<code>show:"Orbital Diner"</code>).
          </p>
        </form>
      )}
    </div>
  );
}

/**
 * What a search is narrowed to, kept apart from the text: the libraries to
 * search in, and filter chips (show, episode, movie, season...) that stay put
 * while you search the dialogue inside them.
 */
export function SearchScope({ libraryNames, searchLibraries, onPickLibraries, scope, onAddFilter, onRemoveFilter, onClear }) {
  return (
    <div className="search-scope" aria-label="Narrow the search">
      {libraryNames.length > 1 && (
        <>
          <span className="muted">Search in</span>
          <LibraryFilter all={libraryNames} selected={searchLibraries} onChange={onPickLibraries} />
        </>
      )}
      {scope.map((f, i) => (
        <span key={describeFilter(f)} className={"scope-chip" + (f.negate ? " not" : "")}>
          {describeFilter(f)}
          <button
            type="button"
            className="scope-chip-x"
            aria-label={`Remove filter ${describeFilter(f)}`}
            title="Remove this filter"
            onClick={() => onRemoveFilter(i)}
          >
            ×
          </button>
        </span>
      ))}
      <AddFilter onAdd={onAddFilter} />
      {scope.length > 1 && (
        <button type="button" className="link tiny" onClick={onClear}>
          clear filters
        </button>
      )}
    </div>
  );
}
