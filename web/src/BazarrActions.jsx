import { useEffect, useState } from "react";
import { api } from "./api.js";
import { missingLabel, useBazarrConfigured } from "./bazarr.js";

/**
 * What Bazarr can do for one item: search for the subtitles it is missing, and
 * re-time the subtitle file beside the video. Shows nothing unless Bazarr is
 * set up. `onChanged` is called after a re-index, so the caller can reload
 * the item's subtitle lines.
 */
export default function BazarrActions({ itemId, compact = false, onChanged }) {
  const configured = useBazarrConfigured();
  const [info, setInfo] = useState(null); // answer of GET .../bazarr
  const [err, setErr] = useState(null);
  const [busy, setBusy] = useState(null); // "search" | "sync" | "reindex"
  const [note, setNote] = useState(null);

  // The compact (list) form doesn't look the item up first: that would ask
  // Bazarr about every row. The search itself reports "not in Bazarr".
  useEffect(() => {
    if (!configured || compact) return undefined;
    let alive = true;
    setInfo(null);
    setErr(null);
    api
      .itemBazarr(itemId)
      .then((d) => alive && setInfo(d))
      .catch((e) => alive && setErr(e.message));
    return () => {
      alive = false;
    };
  }, [configured, compact, itemId]);

  if (!configured) return null;

  async function run(kind, call) {
    setBusy(kind);
    setNote(null);
    try {
      const r = await call();
      setNote({ ok: true, text: r.message || "Done." });
    } catch (e) {
      setNote({ ok: false, text: e.message });
    } finally {
      setBusy(null);
    }
  }

  async function reread() {
    await run("reindex", async () => {
      const r = await api.reindexItem(itemId);
      onChanged?.(r);
      return {
        message:
          r.status === "unchanged"
            ? "Nothing new beside the video yet. Try again once Bazarr has written the file."
            : "Subtitles re-read.",
      };
    });
  }

  const found = compact ? true : info?.found;
  const missing = info?.target?.missing || [];

  // In a list row: just the button, in the row's own line of buttons, and its
  // answer under them (a fragment, so it lines up with Find subtitles and Open).
  if (compact) {
    return (
      <>
        <button
          type="button"
          className="small secondary"
          disabled={busy != null}
          onClick={() => run("search", () => api.bazarrSearch(itemId))}
          title="Ask Bazarr to search its providers for the missing subtitles"
        >
          {busy === "search" ? "Asking…" : "Search with Bazarr"}
        </button>
        {note && (
          <span className={"bazarr-note " + (note.ok ? "hint tiny" : "error")} role="status">
            {note.text}
          </span>
        )}
      </>
    );
  }

  return (
    <div className="bazarr">
      <h4>Bazarr</h4>
      {err && <p className="hint tiny">Bazarr: {err}</p>}
      {info && !found && (
        <p className="hint tiny">
          Bazarr doesn't have this file, or couldn't tell it apart from another.
        </p>
      )}
      {found && (
        <p className="hint tiny">
          {info.target.title}
          {missing.length > 0 ? ` — missing: ${missingLabel(missing)}` : " — nothing missing in Bazarr"}
        </p>
      )}
      <div className="btn-row">
        <button
          type="button"
          className="small secondary"
          disabled={!found || busy != null}
          onClick={() => run("search", () => api.bazarrSearch(itemId))}
          title="Ask Bazarr to search its providers for the missing subtitles"
        >
          {busy === "search" ? "Asking…" : "Search with Bazarr"}
        </button>
        <>
            <button
              type="button"
              className="small secondary"
              disabled={!found || !info?.can_sync || busy != null}
              onClick={() => run("sync", () => api.bazarrSync(itemId))}
              title={
                info && !info.can_sync
                  ? "There is no subtitle file beside the video to re-time"
                  : "Ask Bazarr to re-time the subtitle against the audio"
              }
            >
              {busy === "sync" ? "Asking…" : "Fix timing with Bazarr"}
            </button>
            <button type="button" className="small secondary" disabled={busy != null} onClick={reread}>
              {busy === "reindex" ? "Reading…" : "Re-read subtitles"}
            </button>
        </>
      </div>
      {note && (
        <p className={note.ok ? "hint tiny" : "error"} role="status">
          {note.text}
        </p>
      )}
    </div>
  );
}
