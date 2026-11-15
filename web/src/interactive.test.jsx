// @vitest-environment jsdom
//
// The parts of the editor that only make sense when something is clicked or
// dragged: the timeline handles, the transport bar and the Bazarr apply button.
// The other tests render static markup; these run the components in a DOM.
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { CaptionPanel } from "./CaptionPanel.jsx";
import { PlayerPanel } from "./PlayerPanel.jsx";
import { Timeline } from "./Timeline.jsx";

afterEach(cleanup);

// ---- the timeline: drag the handles, the clip, the ruler, a subtitle line ----

const NTSC = { num: 24000, den: 1001 };
const TRACK_PX = 1000; // the track is 1000 px wide, so 1 px = 10 ms over a 10 s view

function timeline(over = {}) {
  const props = {
    ratingKey: "7",
    view: { from: 0, to: 10_000 },
    start: 2000,
    end: 6000,
    playhead: 1000,
    fps: null,
    snap: "off",
    cues: [],
    onTrim: vi.fn(),
    onCueMove: vi.fn(),
    onSeek: vi.fn(),
    onZoom: vi.fn(),
    onFit: vi.fn(),
    ...over,
  };
  const view = render(<Timeline {...props} />);
  return { props, ...view };
}

describe("Timeline dragging", () => {
  beforeEach(() => {
    // jsdom has no layout: say where the track is.
    vi.spyOn(Element.prototype, "getBoundingClientRect").mockReturnValue({
      left: 0, top: 0, right: TRACK_PX, bottom: 40, width: TRACK_PX, height: 40, x: 0, y: 0, toJSON() {},
    });
  });
  afterEach(() => vi.restoreAllMocks());

  const drag = (el, from, to) => {
    fireEvent.pointerDown(el, { clientX: from, pointerId: 1 });
    fireEvent.pointerMove(el, { clientX: to, pointerId: 1 });
    fireEvent.pointerUp(el, { clientX: to, pointerId: 1 });
  };

  it("drags the In handle: the new start while moving, then one committed change", () => {
    const { props } = timeline();
    drag(screen.getByLabelText(/^In point/), 200, 300);
    expect(props.onTrim).toHaveBeenNthCalledWith(1, 3000, 6000, false, "start");
    expect(props.onTrim).toHaveBeenLastCalledWith(2000, 6000, true, "end"); // the drag ended
    // The move also bubbles up to the film strip's own handler, so it can be
    // reported twice; it is the same value, and only one call commits.
    const calls = props.onTrim.mock.calls;
    expect(calls.filter((c) => c[2] === true)).toHaveLength(1);
    expect(new Set(calls.filter((c) => c[2] === false).map((c) => c.join())).size).toBe(1);
  });

  it("drags the Out handle", () => {
    const { props } = timeline();
    drag(screen.getByLabelText(/^Out point/), 600, 800);
    expect(props.onTrim).toHaveBeenNthCalledWith(1, 2000, 8000, false, "end");
  });

  it("will not drag In past Out: the clip keeps its minimum length", () => {
    const { props } = timeline();
    drag(screen.getByLabelText(/^In point/), 200, 900);
    const [start, end] = props.onTrim.mock.calls[0];
    expect(end).toBe(6000);
    expect(start).toBeLessThan(end);
  });

  it("slides the whole clip when its band is dragged, keeping its length", () => {
    const { props, container } = timeline();
    drag(container.querySelector(".tl-band"), 400, 500);
    expect(props.onTrim).toHaveBeenNthCalledWith(1, 3000, 7000, false, "start");
  });

  it("seeks when the ruler or the film strip is dragged, and never below zero", () => {
    const { props, container } = timeline();
    fireEvent.pointerDown(container.querySelector(".tl-ruler"), { clientX: 450, pointerId: 1 });
    expect(props.onSeek).not.toHaveBeenCalled(); // pressing records; moving seeks
    fireEvent.pointerMove(container.querySelector(".tl-ruler"), { clientX: 450, pointerId: 1 });
    expect(props.onSeek).toHaveBeenLastCalledWith(4500);
    fireEvent.pointerMove(container.querySelector(".tl-ruler"), { clientX: -50, pointerId: 1 });
    expect(props.onSeek).toHaveBeenLastCalledWith(0);
  });

  it("ignores movement when nothing is being dragged", () => {
    const { props, container } = timeline();
    fireEvent.pointerMove(container.querySelector(".tl-strip"), { clientX: 300, pointerId: 1 });
    expect(props.onTrim).not.toHaveBeenCalled();
    expect(props.onSeek).not.toHaveBeenCalled();
  });

  it("stops following the pointer once it is released", () => {
    const { props } = timeline();
    const handle = screen.getByLabelText(/^In point/);
    drag(handle, 200, 300);
    props.onTrim.mockClear();
    fireEvent.pointerMove(handle, { clientX: 500, pointerId: 1 });
    expect(props.onTrim).not.toHaveBeenCalled();
  });

  it("moves an editable subtitle line, and ends the drag", () => {
    const cues = [{ start_ms: 3000, end_ms: 4000, text: "hello", index: 0 }];
    const { props, container } = timeline({ cues });
    drag(container.querySelector(".tl-cue"), 350, 450);
    expect(props.onCueMove).toHaveBeenNthCalledWith(1, 0, 4000, 5000, false);
    expect(props.onCueMove).toHaveBeenLastCalledWith(0, null, null, true);
  });

  it("leaves a subtitle line alone when it is not editable", () => {
    const cues = [{ start_ms: 3000, end_ms: 4000, text: "fixed", index: null }];
    const { props, container } = timeline({ cues });
    drag(container.querySelector(".tl-cue"), 350, 450);
    expect(props.onCueMove).not.toHaveBeenCalled();
  });

  it("snaps a dragged In point to a whole frame", () => {
    const { props } = timeline({ snap: "frame", fps: NTSC });
    drag(screen.getByLabelText(/^In point/), 200, 301);
    const [start] = props.onTrim.mock.calls[0];
    const frame = (start * NTSC.num) / (1000 * NTSC.den);
    expect(Math.abs(frame - Math.round(frame))).toBeLessThan(0.05);
  });

  it("steps a handle by the arrow keys, ten at a time with Shift", () => {
    const { props } = timeline();
    const handle = screen.getByLabelText(/^In point/);
    fireEvent.keyDown(handle, { key: "ArrowRight" });
    expect(props.onTrim).toHaveBeenLastCalledWith(2040, 6000, true, "start");
    fireEvent.keyDown(handle, { key: "ArrowLeft", shiftKey: true });
    expect(props.onTrim).toHaveBeenLastCalledWith(1600, 6000, true, "start");
    props.onTrim.mockClear();
    fireEvent.keyDown(handle, { key: "a" });
    expect(props.onTrim).not.toHaveBeenCalled();
  });

  it("zooms and fits from its buttons", () => {
    const { props } = timeline();
    fireEvent.click(screen.getByLabelText("Zoom in"));
    expect(props.onZoom).toHaveBeenLastCalledWith(0.5, 4000);
    fireEvent.click(screen.getByLabelText("Zoom out"));
    expect(props.onZoom).toHaveBeenLastCalledWith(2, 4000);
    fireEvent.click(screen.getByText("Fit clip"));
    expect(props.onFit).toHaveBeenCalledTimes(1);
  });
});

// ---- the player: transport buttons, and the video telling us it plays/pauses ----

function player(over = {}) {
  const props = {
    previewError: null,
    setPreviewError: vi.fn(),
    proxy: { src: "blob:preview", from: 0, to: 8000 },
    proxyLoading: false,
    previewStalled: false,
    videoRef: { current: null },
    measurePicture: vi.fn(),
    proxyLoaded: vi.fn(),
    playing: false,
    setPlaying: vi.fn(),
    setPlayhead: vi.fn(),
    mediaTimeNow: () => 2500,
    capStyle: {},
    captionMode: "none",
    overlayText: "",
    top: "",
    bottom: "",
    playhead: 2000,
    srcFps: NTSC,
    seekTo: vi.fn(),
    start: 2000,
    end: 6000,
    stepPlayhead: vi.fn(),
    togglePlay: vi.fn(),
    speed: 1,
    setSpeed: vi.fn(),
    volume: { level: 1, muted: false },
    setVolume: vi.fn(),
    ...over,
  };
  const view = render(<PlayerPanel {...props} />);
  return { props, ...view };
}

describe("PlayerPanel playback controls", () => {
  it("play/pause calls togglePlay, and its label follows the state", () => {
    const { props, unmount } = player();
    fireEvent.click(screen.getByLabelText("Play the clip"));
    expect(props.togglePlay).toHaveBeenCalledTimes(1);
    unmount();
    player({ playing: true });
    expect(screen.getByLabelText("Pause")).toBeTruthy();
    expect(screen.queryByLabelText("Play the clip")).toBeNull();
  });

  it("steps a frame back and forward", () => {
    const { props } = player();
    fireEvent.click(screen.getByLabelText("Back one frame"));
    fireEvent.click(screen.getByLabelText("Forward one frame"));
    expect(props.stepPlayhead.mock.calls).toEqual([[-1], [1]]);
  });

  it("jumps to the In point and to just inside the Out point", () => {
    const { props } = player();
    fireEvent.click(screen.getByLabelText("Go to In point"));
    expect(props.seekTo).toHaveBeenLastCalledWith(2000);
    fireEvent.click(screen.getByLabelText("Go to Out point"));
    expect(props.seekTo).toHaveBeenLastCalledWith(5999);
  });

  it("never jumps to an Out point before the In point", () => {
    const { props } = player({ start: 5000, end: 5000 });
    fireEvent.click(screen.getByLabelText("Go to Out point"));
    expect(props.seekTo).toHaveBeenLastCalledWith(5000);
  });

  it("learns from the video element when it starts and stops", () => {
    const { props, container } = player();
    const video = container.querySelector("video");
    fireEvent.play(video);
    expect(props.setPlaying).toHaveBeenLastCalledWith(true);
    fireEvent.pause(video);
    expect(props.setPlaying).toHaveBeenLastCalledWith(false);
  });

  it("follows the video when it is seeked while paused, but not while playing", () => {
    const { props, container, unmount } = player();
    fireEvent.seeked(container.querySelector("video"));
    expect(props.setPlayhead).toHaveBeenLastCalledWith(2500);
    unmount();
    const playing = player({ playing: true });
    fireEvent.seeked(playing.container.querySelector("video"));
    expect(playing.props.setPlayhead).not.toHaveBeenCalled();
  });

  it("shows the error, not a video, when the preview failed", () => {
    const { container } = player({ previewError: "ffmpeg failed" });
    expect(container.querySelector("video")).toBeNull();
    expect(screen.getByText("ffmpeg failed")).toBeTruthy();
  });
});

// ---- the Bazarr apply button -------------------------------------------------

function captions(over = {}) {
  const props = {
    captionMode: "source",
    setCaptionMode: vi.fn(),
    font: "",
    setFont: vi.fn(),
    fontList: [],
    fontScale: 1,
    setFontScale: vi.fn(),
    top: "",
    setTop: vi.fn(),
    bottom: "",
    setBottom: vi.fn(),
    subtitleOffsetMs: 500,
    setSubtitleOffsetMs: vi.fn(),
    bazarrOn: true,
    applying: false,
    applyNote: null,
    applyToSubtitleFile: vi.fn(),
    ...over,
  };
  const view = render(<CaptionPanel {...props} />);
  return { props, ...view };
}

describe("the Bazarr apply button", () => {
  const BUTTON = /Apply to the subtitle file/;

  it("asks Bazarr to shift the file when clicked", () => {
    const { props } = captions();
    fireEvent.click(screen.getByText(BUTTON));
    expect(props.applyToSubtitleFile).toHaveBeenCalledTimes(1);
  });

  it("is not there when Bazarr is not set up", () => {
    captions({ bazarrOn: false });
    expect(screen.queryByText(BUTTON)).toBeNull();
  });

  it("is off while there is no shift to apply", () => {
    captions({ subtitleOffsetMs: 0 });
    expect(screen.getByText(BUTTON).disabled).toBe(true);
  });

  it("can't be pressed twice while it works", () => {
    const { props } = captions({ applying: true });
    const btn = screen.getByText("Applying…");
    expect(btn.disabled).toBe(true);
    fireEvent.click(btn);
    expect(props.applyToSubtitleFile).not.toHaveBeenCalled();
  });

  it("shows what Bazarr answered, as a hint or as an error", () => {
    const ok = captions({ applyNote: { ok: true, text: "The subtitle file was shifted and re-read." } });
    expect(screen.getByRole("status").className).toContain("hint");
    ok.unmount();
    captions({ applyNote: { ok: false, text: "Bazarr doesn't have this file" } });
    expect(screen.getByRole("status").className).toBe("error");
    expect(screen.getByText("Bazarr doesn't have this file")).toBeTruthy();
  });

  it("nudges the sync and resets it", () => {
    const { props } = captions();
    fireEvent.click(screen.getByText("reset"));
    expect(props.setSubtitleOffsetMs).toHaveBeenLastCalledWith(0);
  });
});
