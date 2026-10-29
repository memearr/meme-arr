import { useEffect, useRef, useState } from "react";
import { api } from "./api.js";
import { fileSize, fontName, uploadProblem } from "./fonts.js";

/** Settings: the fonts captions can use. Built-in ones are listed; uploaded ones can be added and removed. */
export function FontsPanel() {
  const [data, setData] = useState(null);
  const [msg, setMsg] = useState(null);
  const [busy, setBusy] = useState(false);
  const input = useRef(null);

  const load = () =>
    api
      .fonts()
      .then(setData)
      .catch((e) => setMsg({ ok: false, text: e.message }));
  useEffect(() => {
    load();
  }, []);

  async function onPick(e) {
    const file = e.target.files?.[0];
    e.target.value = ""; // so picking the same file again works
    if (!file) return;
    const problem = uploadProblem(file, data?.max_bytes);
    if (problem) {
      setMsg({ ok: false, text: problem });
      return;
    }
    setBusy(true);
    setMsg(null);
    try {
      const { font } = await api.uploadFont(file);
      setMsg({ ok: true, text: `Added ${fontName(font)}.` });
      await load();
    } catch (err) {
      setMsg({ ok: false, text: err.message });
    } finally {
      setBusy(false);
    }
  }

  async function remove(f) {
    if (
      !window.confirm(
        `Delete ${fontName(f)}? Anything that uses it (including clips already queued in a sequence) needs another font.`
      )
    ) {
      return;
    }
    setBusy(true);
    setMsg(null);
    try {
      await api.deleteFont(f.id);
      await load();
    } catch (err) {
      setMsg({ ok: false, text: err.message });
    } finally {
      setBusy(false);
    }
  }

  const fonts = data?.fonts || [];
  const full = data && fonts.filter((f) => !f.builtin).length >= data.max_fonts;

  return (
    <div className="panel fonts-panel">
      <h3>Caption fonts</h3>
      <p className="hint tiny">
        Choose any of these for burned subtitles or meme text when you make a clip. Add your own{" "}
        <code>.ttf</code> or <code>.otf</code> files (up to {data ? Math.round(data.max_bytes / 1024 / 1024) : 16} MB
        each); they are kept in your data folder. Only upload fonts you are allowed to use.
      </p>
      {!data && !msg && <p className="hint">Loading…</p>}
      {fonts.length > 0 && (
        <ul className="font-list">
          {fonts.map((f) => (
            <li key={f.id}>
              <span className="font-name">{fontName(f)}</span>
              <span className="muted">
                {f.id} · {fileSize(f.bytes)}
              </span>
              {f.builtin ? (
                <span className="pill queued">built in</span>
              ) : (
                <button type="button" className="danger small" onClick={() => remove(f)} disabled={busy}>
                  Delete
                </button>
              )}
            </li>
          ))}
        </ul>
      )}
      <input
        ref={input}
        type="file"
        accept=".ttf,.otf,font/ttf,font/otf"
        onChange={onPick}
        hidden
        aria-label="Font file"
      />
      <div className="btn-row">
        <button type="button" className="secondary" onClick={() => input.current?.click()} disabled={busy || full}>
          {busy ? "Uploading…" : "Upload a font"}
        </button>
      </div>
      {full && <p className="hint tiny">The limit of {data.max_fonts} uploaded fonts is reached; delete one to add another.</p>}
      {msg && <p className={msg.ok ? "test ok" : "error"}>{msg.text}</p>}
    </div>
  );
}
