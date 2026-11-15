import { findCueMatches, stepMatch } from "./cueFind.js";
import { fmtTime } from "./format.js";
import { useEffect, useRef, useState } from "react";

/** Every subtitle line of the item, with a find box; picking one moves the clip to it. */
export function CueBrowser({ allCues, onJump }) {
  const [showCues, setShowCues] = useState(false);
  const [cueFind, setCueFind] = useState("");
  const [findIndex, setFindIndex] = useState(-1); // position within cueMatches, not allCues
  const cueItemRefs = useRef([]);

  const cueMatches = findCueMatches(allCues, cueFind);
  const currentMatchCueIndex = findIndex >= 0 ? cueMatches[findIndex] : -1;

  useEffect(() => {
    setFindIndex(cueMatches.length > 0 ? 0 : -1);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [cueFind, allCues]);

  useEffect(() => {
    if (currentMatchCueIndex < 0) return;
    cueItemRefs.current[currentMatchCueIndex]?.scrollIntoView({
      block: "center",
      behavior: "smooth",
    });
  }, [currentMatchCueIndex]);

  function goToMatch(delta) {
    if (cueMatches.length === 0) return;
    setFindIndex((i) => stepMatch(i, delta, cueMatches.length));
  }

  function handleFindKeyDown(e) {
    if (e.key !== "Enter") return;
    e.preventDefault();
    goToMatch(e.shiftKey ? -1 : 1);
  }

  if (allCues.length === 0) return null;
  return (
    <div className="cue-browser">
      <button type="button" className="secondary" onClick={() => setShowCues((v) => !v)}>
        {showCues ? "▾" : "▸"} Browse subtitles ({allCues.length} lines)
      </button>
      {showCues && (
        <div className="cue-list-wrap">
          <div className="cue-find-row">
            <input
              className="cue-filter"
              placeholder="Find in subtitles… (Enter for next)"
              value={cueFind}
              onChange={(e) => setCueFind(e.target.value)}
              onKeyDown={handleFindKeyDown}
            />
            <button
              type="button"
              className="cue-find-btn"
              title="Previous match (Shift+Enter)"
              disabled={cueMatches.length === 0}
              onClick={() => goToMatch(-1)}
            >
              ▲
            </button>
            <button
              type="button"
              className="cue-find-btn"
              title="Next match (Enter)"
              disabled={cueMatches.length === 0}
              onClick={() => goToMatch(1)}
            >
              ▼
            </button>
            {cueFind.trim() && (
              <span className="cue-find-status">
                {cueMatches.length > 0 ? `${findIndex + 1} of ${cueMatches.length}` : "No matches"}
              </span>
            )}
          </div>
          <ul className="cue-list">
            {allCues.map((c, i) => (
              <li key={i}>
                <button
                  type="button"
                  ref={(el) => {
                    // A block, not an expression: React 19 treats a returned
                    // function as this ref's cleanup, so return nothing.
                    cueItemRefs.current[i] = el;
                  }}
                  className={"cue-item" + (i === currentMatchCueIndex ? " current-match" : "")}
                  onClick={() => {
                    onJump(c);
                    setShowCues(false);
                  }}
                >
                  <span className="cue-ts">{fmtTime(c.start_ms)}</span>
                  <span className="cue-text">{c.text}</span>
                </button>
              </li>
            ))}
          </ul>
        </div>
      )}
    </div>
  );
}
