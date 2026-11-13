import { useState } from "react";
import { api } from "./api.js";
import { episodeLabel, fmtTime } from "./format.js";
import { MAX_TOTAL_MS, lengthOf, totalMs } from "./sequenceModel.js";

/**
 * The whole sequence as one strip: every section as a block, as wide as it is
 * long, in the order they play. Click a block to pick it; drag blocks (or use
 * the arrows) to reorder. `progressMs` draws a marker for where playback is.
 */
export function SequenceTimeline({
  sequence,
  selectedId,
  progressMs,
  onSelect,
  onMove,
  onDuplicate,
  onRemove,
}) {
  const [dragFrom, setDragFrom] = useState(null);
  const [over, setOver] = useState(null);
  const total = totalMs(sequence);
  const selected = sequence.findIndex((c) => c.id === selectedId);
  const pct = total > 0 ? Math.min(100, (Math.max(0, progressMs || 0) / total) * 100) : 0;
  const used = Math.min(100, (total / MAX_TOTAL_MS) * 100);

  return (
    <div className="seq-tl">
      <div className="seq-tl-bar">
        <span className="tl-label">Sequence</span>
        <span className={"tl-range" + (total > MAX_TOTAL_MS ? " error" : "")}>
          {fmtTime(total)} of {fmtTime(MAX_TOTAL_MS)} · {sequence.length} section
          {sequence.length === 1 ? "" : "s"}
        </span>
        <span className="tl-spacer" />
        <span className="seq-tl-meter" aria-hidden="true">
          <span className={total > MAX_TOTAL_MS ? "over" : ""} style={{ width: `${used}%` }} />
        </span>
      </div>

      <div className="seq-tl-strip" role="list" aria-label="Sections in play order">
        {sequence.map((c, i) => (
          <button
            key={c.id}
            type="button"
            role="listitem"
            className={
              "seq-block" +
              (c.id === selectedId ? " selected" : "") +
              (over === i && dragFrom !== null && dragFrom !== i ? " drop" : "")
            }
            style={{ flexGrow: Math.max(lengthOf(c), 600), flexBasis: 0 }}
            draggable
            title={`${i + 1}. ${episodeLabel(c)}\n${fmtTime(c.start_ms)} → ${fmtTime(c.end_ms)}${c.line ? `\n“${c.line}”` : ""}`}
            aria-pressed={c.id === selectedId}
            onClick={() => onSelect(c.id)}
            onDragStart={(e) => {
              setDragFrom(i);
              e.dataTransfer?.setData("text/plain", String(i));
              if (e.dataTransfer) e.dataTransfer.effectAllowed = "move";
            }}
            onDragOver={(e) => {
              e.preventDefault();
              setOver(i);
            }}
            onDrop={(e) => {
              e.preventDefault();
              if (dragFrom !== null && dragFrom !== i) onMove(dragFrom, i);
              setDragFrom(null);
              setOver(null);
            }}
            onDragEnd={() => {
              setDragFrom(null);
              setOver(null);
            }}
          >
            <img
              className="seq-block-pic"
              src={api.frameUrl(c.rating_key, c.start_ms, 200)}
              alt=""
              loading="lazy"
              draggable={false}
            />
            <span className="seq-block-num">{i + 1}</span>
            <span className="seq-block-text">
              <span className="seq-block-title">{episodeLabel(c)}</span>
              <span className="seq-block-len">{(lengthOf(c) / 1000).toFixed(1)} s</span>
            </span>
          </button>
        ))}
        {total > 0 && <span className="seq-tl-marker" style={{ left: `${pct}%` }} aria-hidden="true" />}
      </div>

      <div className="seq-tl-tools" role="group" aria-label="Selected section">
        <button
          type="button"
          className="secondary small"
          disabled={selected <= 0}
          onClick={() => onMove(selected, selected - 1)}
          title="Play this section earlier"
        >
          ◀ Earlier
        </button>
        <button
          type="button"
          className="secondary small"
          disabled={selected < 0 || selected >= sequence.length - 1}
          onClick={() => onMove(selected, selected + 1)}
          title="Play this section later"
        >
          Later ▶
        </button>
        <button type="button" className="secondary small" disabled={selected < 0} onClick={() => onDuplicate(selectedId)}>
          ⧉ Duplicate
        </button>
        <button type="button" className="danger small" disabled={selected < 0} onClick={() => onRemove(selectedId)}>
          ✕ Remove
        </button>
        <span className="hint tiny">Drag blocks to reorder. A block is as wide as its section is long.</span>
      </div>
    </div>
  );
}
