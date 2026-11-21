// @vitest-environment jsdom
import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { useState } from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { api } from "./api.js";
import { ADVANCED_KEY, useAdvanced } from "./advanced.js";
import { SectionEditor } from "./SectionEditor.jsx";
import { SequenceView } from "./SequenceView.jsx";
import {
  DEFAULT_MS,
  KINDS,
  MAX_MS,
  adjustTransition,
  overlapMs,
  transitionLimit,
  transitionsForRequest,
} from "./transitionModel.js";

beforeEach(() => {
  window.localStorage.clear();
  vi.spyOn(api, "fetchBlobUrl").mockResolvedValue("blob:section");
  vi.spyOn(api, "fonts").mockResolvedValue({ fonts: [], default: null });
  vi.spyOn(api, "videoInfo").mockResolvedValue({});
  vi.spyOn(window.HTMLMediaElement.prototype, "play").mockImplementation(() => Promise.resolve());
  vi.spyOn(window.HTMLMediaElement.prototype, "pause").mockImplementation(() => {});
  URL.revokeObjectURL = vi.fn();
});
afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
});

const sec = (id, ms, extra = {}) => ({
  id,
  rating_key: `r-${id}`,
  title: id,
  start_ms: 0,
  end_ms: ms,
  caption: { mode: "none" },
  subtitle_offset_ms: 0,
  ...extra,
});
const fade = (ms) => ({ kind: "fade", ms });

describe("what a transition may be", () => {
  it("offers the server's kinds by their names", () => {
    expect(KINDS.map((k) => k.id)).toEqual([
      "fade", "dissolve", "fade_black", "fade_white", "wipe_left", "wipe_right", "wipe_up", "wipe_down",
      "slide_left", "slide_right", "slide_up", "slide_down", "circle_open", "circle_close", "radial", "pixelize",
    ]);
  });

  it("has none on the first section, and is as long as the sections around it leave room for", () => {
    const seq = [sec("a", 5000), sec("b", 5000), sec("c", 5000)];
    expect(transitionLimit(seq, 0)).toBe(0);
    expect(transitionLimit(seq, 1)).toBe(MAX_MS);
    expect(transitionLimit(seq, 5)).toBe(0);
    // the room in a short section is shared with the transition out of it
    const short = [sec("a", 5000), sec("b", 2000), sec("c", 5000, { transition: fade(1500) })];
    expect(transitionLimit(short, 1)).toBe(500);
    // and with the one into the section before it
    const tight = [sec("a", 5000), sec("b", 1500, { transition: fade(1000) }), sec("c", 5000)];
    expect(transitionLimit(tight, 2)).toBe(500);
    // too little room for the shortest transition: none
    expect(transitionLimit([sec("a", 5000), sec("b", 50)], 1)).toBe(0);
  });

  it("keeps a transition inside that, and drops what can't be", () => {
    const seq = [sec("a", 5000), sec("b", 2000), sec("c", 5000)];
    expect(adjustTransition(seq, 1, fade(900))).toEqual(fade(900));
    expect(adjustTransition(seq, 1, fade(9000))).toEqual(fade(MAX_MS > 2000 ? 2000 : MAX_MS));
    expect(adjustTransition(seq, 1, { kind: "fade", ms: 10 })).toEqual(fade(100));
    expect(adjustTransition(seq, 1, { kind: "spin", ms: 500 })).toBe(null);
    expect(adjustTransition(seq, 0, fade(500))).toBe(null);
    expect(adjustTransition(seq, 1, null)).toBe(null);
    expect(adjustTransition(seq, 1, { kind: "fade" }).ms).toBe(DEFAULT_MS);
  });

  it("sends none without Advanced, and every one inside what its neighbours allow", () => {
    const seq = [sec("a", 5000, { transition: fade(500) }), sec("b", 2000, { transition: fade(1500) }), sec("c", 5000, { transition: fade(1500) })];
    expect(transitionsForRequest(seq, false)).toEqual([undefined, undefined, undefined]);
    const t = transitionsForRequest(seq, true);
    expect(t[0]).toBeUndefined(); // the first has nothing to come in over
    // b keeps room for the transition out of it: 2 s - 1.5 s
    expect(t[1].ms + (t[2]?.ms ?? 0)).toBeLessThanOrEqual(2000);
    expect(overlapMs(t)).toBe(t.reduce((n, x) => n + (x?.ms ?? 0), 0));
    expect(overlapMs([undefined, undefined])).toBe(0);
  });
});

describe("the transition box of a section", () => {
  function Harness({ limit = 2000, seen }) {
    const [clip, setClip] = useState(sec("b", 4000));
    seen?.(clip);
    return <SectionEditor clip={clip} index={1} count={3} onChange={setClip} onDetailed={() => {}} transitionLimit={limit} />;
  }

  it("is there only with room for a transition", () => {
    render(<Harness limit={0} />);
    expect(screen.queryByLabelText("Transition")).toBeNull();
    cleanup();
    render(<Harness />);
    expect(screen.getByLabelText("Transition")).toBeTruthy();
  });

  it("picks a kind, sets its length within the limit, and goes back to a cut", async () => {
    let seen;
    render(<Harness limit={800} seen={(c) => (seen = c)} />);
    expect(screen.queryByLabelText("Transition length")).toBeNull();
    await act(async () => {
      fireEvent.change(screen.getByLabelText("Transition"), { target: { value: "wipe_left" } });
    });
    expect(seen.transition).toEqual({ kind: "wipe_left", ms: 800 }); // the default, kept inside the limit
    await act(async () => {
      fireEvent.change(screen.getByLabelText("Transition length"), { target: { value: "300" } });
    });
    expect(seen.transition).toEqual({ kind: "wipe_left", ms: 300 });
    await act(async () => {
      fireEvent.change(screen.getByLabelText("Transition"), { target: { value: "" } });
    });
    expect(seen.transition).toBeUndefined();
    expect(screen.queryByLabelText("Transition length")).toBeNull();
  });
});

describe("the sequence's export", () => {
  const sequence = [sec("a", 4000), sec("b", 4000, { transition: fade(1000) }), sec("c", 4000)];

  async function exportIt() {
    const start = vi.spyOn(api, "startSequenceExport").mockResolvedValue({ job_id: "j1" });
    render(<SequenceView sequence={sequence} setSequence={() => {}} onJob={() => {}} />);
    await act(async () => {
      fireEvent.click(await screen.findByText(/Export GIF \(3 clips\)/));
    });
    return start.mock.calls[0][0];
  }

  it("leaves the transitions out until Advanced is on", async () => {
    const sent = await exportIt();
    expect(sent.clips.map((c) => c.transition)).toEqual([undefined, undefined, undefined]);
  });

  it("sends them with Advanced on, only where a section has one", async () => {
    window.localStorage.setItem(ADVANCED_KEY, "1");
    const sent = await exportIt();
    expect(sent.clips.map((c) => c.transition)).toEqual([undefined, fade(1000), undefined]);
  });

  it("has its own Advanced tick box, and the first section has no transition box", async () => {
    render(<SequenceView sequence={sequence} setSequence={() => {}} onJob={() => {}} />);
    await screen.findByText(/Export GIF/);
    expect(screen.queryByLabelText("Transition")).toBeNull();
    fireEvent.click(screen.getByLabelText("Advanced"));
    // the open section is the first, which has nothing before it to come in over
    expect(screen.queryByLabelText("Transition")).toBeNull();
    expect(screen.getAllByLabelText("Advanced")).toHaveLength(1);
    expect(window.localStorage.getItem(ADVANCED_KEY)).toBe("1");
  });
});

describe("the Advanced switch in two places", () => {
  it("follows itself: ticking one box ticks the other", () => {
    function Box({ name }) {
      const [on, set] = useAdvanced();
      return (
        <label>
          <input type="checkbox" checked={on} onChange={(e) => set(e.target.checked)} aria-label={name} />
        </label>
      );
    }
    render(
      <>
        <Box name="one" />
        <Box name="two" />
      </>
    );
    expect(screen.getByLabelText("one").checked).toBe(false);
    fireEvent.click(screen.getByLabelText("one"));
    expect(screen.getByLabelText("one").checked).toBe(true);
    expect(screen.getByLabelText("two").checked).toBe(true);
    fireEvent.click(screen.getByLabelText("two"));
    expect(screen.getByLabelText("one").checked).toBe(false);
    expect(window.localStorage.getItem(ADVANCED_KEY)).toBe("0");
  });
});
