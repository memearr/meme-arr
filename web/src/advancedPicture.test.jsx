// @vitest-environment jsdom
import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { useState } from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { api } from "./api.js";
import { AdvancedPicture } from "./AdvancedPicture.jsx";
import { ADVANCED_KEY } from "./advanced.js";
import {
  ASPECTS,
  WHOLE,
  cropBox,
  cropForRequest,
  cropWindow,
  movable,
  playbackRate,
  speedForRequest,
  speedText,
} from "./advancedPicture.js";
import { PlayerPanel } from "./PlayerPanel.jsx";
import { SequenceView } from "./SequenceView.jsx";
import { SongPreview } from "./SongPreview.jsx";
import { editorSeed, updatedClip } from "./sequenceClip.js";
import { PLAIN } from "./gradeModel.js";

beforeEach(() => {
  window.localStorage.clear();
  vi.spyOn(api, "fetchBlobUrl").mockResolvedValue("blob:section");
  vi.spyOn(api, "musicSources").mockResolvedValue({ navidrome: true, library: false, jellyfin: false, uploads: 0 });
  vi.spyOn(window.HTMLMediaElement.prototype, "play").mockImplementation(() => Promise.resolve());
  vi.spyOn(window.HTMLMediaElement.prototype, "pause").mockImplementation(() => {});
  URL.revokeObjectURL = vi.fn();
});
afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
});

describe("the reframe window", () => {
  const wide = 16 / 9;

  it("keeps the whole picture for the whole picture, and for a shape it already is", () => {
    expect(cropWindow(WHOLE, wide)).toBe(null);
    expect(cropWindow(undefined, wide)).toBe(null);
    expect(cropWindow({ aspect: "wide", cx: 0.5, cy: 0.5 }, wide)).toBe(null);
    expect(cropWindow({ aspect: "nonsense" }, wide)).toBe(null);
  });

  it("makes the largest window of the shape, centred", () => {
    const sq = cropWindow({ aspect: "square", cx: 0.5, cy: 0.5 }, wide);
    expect(sq.h).toBe(1);
    expect(sq.w).toBeCloseTo(9 / 16, 4);
    expect(sq.x).toBeCloseTo((1 - 9 / 16) / 2, 4);
    const tall = cropWindow({ aspect: "vertical", cx: 0.5, cy: 0.5 }, wide);
    expect((tall.w * wide) / tall.h).toBeCloseTo(9 / 16, 3);
    // a tall picture gets a short window
    const short = cropWindow({ aspect: "wide", cx: 0.5, cy: 0.5 }, 9 / 16);
    expect(short.w).toBe(1);
    expect(short.h).toBeCloseTo(((9 / 16) * 9) / 16, 4);
  });

  it("is moved by its centre and kept inside the picture", () => {
    const w = (cx, cy = 0.5) => cropWindow({ aspect: "vertical", cx, cy }, wide);
    expect(w(0).x).toBe(0);
    expect(w(1).x + w(1).w).toBeCloseTo(1, 6);
    expect(w(0.5).x).toBeGreaterThan(0.3);
    expect(w(NaN).x).toBeCloseTo(w(0.5).x, 6);
    expect(w(5).x + w(5).w).toBeLessThanOrEqual(1.0000001);
  });

  it("falls back to 16:9 when the picture's shape is unknown, and never goes under a fifth", () => {
    expect(cropWindow({ aspect: "square" }, NaN).w).toBeCloseTo(9 / 16, 4);
    expect(cropWindow({ aspect: "vertical" }, 10).w).toBeGreaterThanOrEqual(0.2);
  });

  it("is sent rounded, or as nothing", () => {
    expect(cropForRequest(null)).toBe(null);
    expect(cropForRequest({ x: 0.123456, y: 0, w: 0.5, h: 1 })).toEqual({ x: 0.1235, y: 0, w: 0.5, h: 1 });
    // rounding never pushes it past the edge
    const c = cropForRequest({ x: 0.99996, y: 0, w: 0.00004 + 0.2, h: 1 });
    expect(c.x + c.w).toBeLessThanOrEqual(1);
  });

  it("says which way it can be moved", () => {
    expect(movable(null)).toEqual({ across: false, down: false });
    expect(movable({ x: 0, y: 0, w: 0.5, h: 1 })).toEqual({ across: true, down: false });
    expect(movable({ x: 0, y: 0, w: 1, h: 0.5 })).toEqual({ across: false, down: true });
  });

  it("is drawn on the picture where it sits", () => {
    expect(cropBox(null, { left: 0, top: 0, width: 10, height: 10 })).toBe(null);
    expect(cropBox({ x: 0.5, y: 0.25, w: 0.25, h: 0.5 }, { left: 10, top: 20, width: 200, height: 100 })).toEqual({
      left: 110,
      top: 45,
      width: 50,
      height: 50,
    });
  });
});

describe("the speed", () => {
  it("says a speed and keeps one the export takes", () => {
    expect(speedText(1.5)).toBe("1.5×");
    expect(speedForRequest(2)).toBe(2);
    expect(speedForRequest(0.1)).toBe(1);
    expect(speedForRequest(9)).toBe(1);
    expect(speedForRequest(NaN)).toBe(1);
  });

  it("makes the player play at the export speed times its own", () => {
    expect(playbackRate(1, 1)).toBe(1);
    expect(playbackRate(0.5, 2)).toBe(1);
    expect(playbackRate(2, 4)).toBe(8);
    expect(playbackRate(undefined, undefined)).toBe(1);
    expect(playbackRate(16, 4)).toBe(16);
  });

  it("makes the song keep up with the player", () => {
    const m = { id: "s1", start_ms: 0, at_ms: 0, volume: 1, fade_in_ms: 0, fade_out_ms: 0, duck: 0, pan: 0, curve: [] };
    const { rerender } = render(<SongPreview music={m} relMs={500} clipMs={4000} playing rate={2} />);
    const audio = document.querySelector("audio");
    expect(audio.playbackRate).toBe(2);
    rerender(<SongPreview music={m} relMs={500} clipMs={4000} playing rate={0.5} />);
    expect(audio.playbackRate).toBe(0.5);
  });
});

function Harness({ seen, aspect = "full" }) {
  const [speed, setSpeed] = useState(1);
  const [view, setView] = useState({ ...WHOLE, aspect });
  seen?.({ speed, view });
  return <AdvancedPicture speed={speed} onSpeed={setSpeed} view={view} onView={setView} srcAspect={16 / 9} />;
}

describe("the advanced picture box", () => {
  it("picks a speed", () => {
    let seen;
    render(<Harness seen={(s) => (seen = s)} />);
    fireEvent.change(screen.getByLabelText("Clip speed"), { target: { value: "2" } });
    expect(seen.speed).toBe(2);
  });

  it("offers every shape, and moves the window only when there is one", () => {
    let seen;
    render(<Harness seen={(s) => (seen = s)} />);
    expect(screen.getByLabelText("Reframe").querySelectorAll("option")).toHaveLength(ASPECTS.length);
    expect(screen.queryByLabelText("Frame position across")).toBeNull();
    fireEvent.change(screen.getByLabelText("Reframe"), { target: { value: "vertical" } });
    expect(seen.view.aspect).toBe("vertical");
    const across = screen.getByLabelText("Frame position across");
    expect(screen.queryByLabelText("Frame position down")).toBeNull(); // a vertical window is full height
    fireEvent.change(across, { target: { value: "0.2" } });
    expect(seen.view.cx).toBeCloseTo(0.2, 2);
    fireEvent.doubleClick(screen.getByLabelText("Frame position across"));
    expect(seen.view.cx).toBe(0.5);
  });

  it("moves a wide window down instead of across", () => {
    const { unmount } = render(<AdvancedPicture speed={1} onSpeed={() => {}} view={{ aspect: "wide", cx: 0.5, cy: 0.5 }} onView={() => {}} srcAspect={9 / 16} />);
    expect(screen.getByLabelText("Frame position down")).toBeTruthy();
    expect(screen.queryByLabelText("Frame position across")).toBeNull();
    unmount();
  });
});

describe("the guide in the player", () => {
  const base = {
    previewError: null,
    setPreviewError: () => {},
    proxy: { url: "blob:x", from: 0, to: 4000 },
    proxyLoading: false,
    previewStalled: false,
    videoRef: { current: null },
    measurePicture: () => {},
    proxyLoaded: () => {},
    playing: false,
    setPlaying: () => {},
    setPlayhead: () => {},
    mediaTimeNow: () => 0,
    capStyle: null,
    captionMode: "none",
    overlayText: "",
    lookView: null,
    pictureBox: { left: 0, top: 0, width: 200, height: 100 },
    layers: [],
    relMs: 0,
    selectedLayer: null,
    setSelectedLayer: () => {},
    moveLayer: () => {},
    moveText: () => {},
    top: "",
    bottom: "",
    playhead: 0,
    srcFps: null,
    seekTo: () => {},
    start: 0,
    end: 4000,
    stepPlayhead: () => {},
    togglePlay: () => {},
    speed: 1,
    setSpeed: () => {},
    volume: { level: 0.7, muted: false },
    setVolume: () => {},
    paths: [],
    showPaths: false,
    setShowPaths: () => {},
  };

  it("darkens what is cropped away and outlines what is kept", () => {
    const { container } = render(<PlayerPanel {...base} cropBox={{ left: 60, top: 0, width: 56, height: 100 }} />);
    expect(screen.getByLabelText("The window that is kept")).toBeTruthy();
    expect(container.querySelectorAll(".crop-veil")).toHaveLength(4);
    expect(container.querySelector(".crop-edge")).toBeTruthy();
  });

  it("draws nothing without a window", () => {
    const { container } = render(<PlayerPanel {...base} />);
    expect(container.querySelector(".crop-guide")).toBeNull();
  });
});

describe("a sequence section's picture look", () => {
  const sec = (id, extra = {}) => ({
    id,
    rating_key: `r-${id}`,
    title: id,
    start_ms: 0,
    end_ms: 4000,
    caption: { mode: "none" },
    subtitle_offset_ms: 0,
    ...extra,
  });
  const noir = { ...PLAIN, tone: "bw", fade_in_ms: 500 };

  it("starts the editor from what was stored and puts it back, and a plain look is not kept", () => {
    expect(editorSeed(sec("a", { grade: noir })).grade).toEqual(noir);
    expect(editorSeed(sec("a")).grade).toEqual(PLAIN);
    expect(editorSeed(sec("a", { grade: { tone: "sepia" } })).grade).toEqual({ ...PLAIN, tone: "sepia" });
    const saved = updatedClip(sec("a"), { start: 0, end: 4000, caption: { mode: "none" }, subtitleOffsetMs: 0, layers: [], music: null, grade: noir });
    expect(saved.grade).toEqual(noir);
    const cleared = updatedClip(saved, { start: 0, end: 4000, caption: { mode: "none" }, subtitleOffsetMs: 0, layers: [], music: null, grade: PLAIN });
    expect(cleared.grade).toBeUndefined();
  });

  it("goes in the export only with Advanced on", async () => {
    const start = vi.spyOn(api, "startSequenceExport").mockResolvedValue({ job_id: "j1" });
    const sequence = [sec("a", { grade: noir }), sec("b")];
    const go = async () => {
      render(<SequenceView sequence={sequence} setSequence={() => {}} onJob={() => {}} />);
      await act(async () => {
        fireEvent.click(await screen.findByText(/Export GIF \(2 clips\)/));
      });
      cleanup();
    };
    await go();
    expect(start.mock.calls[0][0].clips[0].grade).toBeUndefined();
    window.localStorage.setItem(ADVANCED_KEY, "1");
    await go();
    expect(start.mock.calls[1][0].clips[0].grade).toMatchObject({ tone: "bw", fade_in_ms: 500 });
    expect(start.mock.calls[1][0].clips[1].grade).toBeUndefined(); // a plain one sends nothing
  });
});
