import { PathControls } from "./PathControls.jsx";
import { ALIGNS, ANIMS, PRESETS, lookPlace, newLook, resolveLook } from "./textLook.js";

/**
 * How burned subtitles look: a preset, the colour, where they sit in the picture, whether
 * the words appear one by one as they are said, and how a line comes in. `look` is null
 * for the plain subtitle look.
 */
export function LookControls({ look, setLook, relMs = 0, clipMs = 0, onSeek }) {
  const patch = (changes) => setLook((l) => ({ ...(l || newLook()), ...changes }));
  const following = !!look?.path?.length;
  const placed = look && !following && Number.isFinite(look.x) && Number.isFinite(look.y);
  const resolved = look ? resolveLook(look) : null;
  return (
    <div className="look-controls">
      <label>
        Look
        <select
          value={look ? look.preset || "classic" : ""}
          onChange={(e) => {
            const id = e.target.value;
            // Switching preset keeps the placement and motion but drops a colour picked for the old one.
            if (!id) setLook(null);
            else setLook((l) => ({ ...(l || newLook(id)), preset: id, color: undefined, outline_color: undefined }));
          }}
        >
          <option value="">Plain subtitles</option>
          {PRESETS.map((p) => (
            <option key={p.id} value={p.id}>
              {p.label}
            </option>
          ))}
        </select>
      </label>
      {look && (
        <>
          <label>
            Colour
            <input type="color" value={resolved.color} onChange={(e) => patch({ color: e.target.value })} />
            {look.color && (
              <button type="button" className="link" onClick={() => patch({ color: undefined })}>
                preset colour
              </button>
            )}
          </label>
          <label>
            Align
            <select value={look.align || "center"} onChange={(e) => patch({ align: e.target.value })}>
              {ALIGNS.map((a) => (
                <option key={a.id} value={a.id}>
                  {a.label}
                </option>
              ))}
            </select>
          </label>
          {!following && (
            <label>
              <input
                type="checkbox"
                checked={!!placed}
                onChange={(e) =>
                  patch(e.target.checked ? { x: look.x ?? 0.5, y: look.y ?? 0.8 } : { x: undefined, y: undefined })
                }
              />
              Place the text myself
            </label>
          )}
          {placed && (
            <>
              <label>
                Across <b>{Math.round(look.x * 100)}%</b>
                <input
                  type="range"
                  min={0}
                  max={100}
                  value={Math.round(look.x * 100)}
                  onChange={(e) => patch({ x: Number(e.target.value) / 100 })}
                />
              </label>
              <label>
                Down <b>{Math.round(look.y * 100)}%</b>
                <input
                  type="range"
                  min={0}
                  max={100}
                  value={Math.round(look.y * 100)}
                  onChange={(e) => patch({ y: Number(e.target.value) / 100 })}
                />
              </label>
            </>
          )}
          <PathControls
            path={look.path}
            onChange={(path) => patch({ path })}
            onSeek={onSeek}
            place={lookPlace(look, relMs) ?? { x: 0.5, y: 0.8 }}
            relMs={relMs}
            clipMs={clipMs}
            what="the text"
          />
          <label>
            Comes in
            <select value={look.anim || "none"} onChange={(e) => patch({ anim: e.target.value })}>
              {ANIMS.map((a) => (
                <option key={a.id} value={a.id}>
                  {a.label}
                </option>
              ))}
            </select>
          </label>
          <label>
            <input
              type="checkbox"
              checked={look.reveal === "word"}
              onChange={(e) => patch({ reveal: e.target.checked ? "word" : "none" })}
            />
            Words appear as they are said
          </label>
          <p className="hint tiny">
            Put *stars* round a word in the line editor to make it bigger. The player shows the colour,
            place and words appearing; the entrance animation shows in the exported file.
          </p>
        </>
      )}
    </div>
  );
}
