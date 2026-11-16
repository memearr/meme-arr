// @vitest-environment jsdom
//
// "Move over time": the keyframe list for a layer and for the caption text.
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { useState } from "react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { LayersPanel } from "./LayersPanel.jsx";
import { PlayerPanel } from "./PlayerPanel.jsx";
import { LookControls } from "./LookControls.jsx";
import { PathControls } from "./PathControls.jsx";
import { layersForRequest } from "./layersModel.js";
import { lookForRequest } from "./textLook.js";

afterEach(cleanup);

const CLIP = 5000;

function Controls({ initial = [], relMs = 1000, place = { x: 0.3, y: 0.6 }, onSeek }) {
  const [path, setPath] = useState(initial);
  return (
    <>
      <PathControls path={path} onChange={setPath} onSeek={onSeek} place={place} relMs={relMs} clipMs={CLIP} />
      <output data-testid="path">{JSON.stringify(path)}</output>
    </>
  );
}
const path = () => JSON.parse(screen.getByTestId("path").textContent);

describe("Move over time", () => {
  it("is off with no keyframes and shows nothing else", () => {
    render(<Controls />);
    expect(screen.getByLabelText("Move over time").checked).toBe(false);
    expect(screen.queryByLabelText("Keyframes")).toBeNull();
  });

  it("turning it on puts a first keyframe where the thing is at the playhead", () => {
    render(<Controls relMs={1500} place={{ x: 0.3, y: 0.6 }} />);
    fireEvent.click(screen.getByLabelText("Move over time"));
    expect(path()).toEqual([{ t_ms: 1500, x: 0.3, y: 0.6 }]);
    expect(screen.getByLabelText("Keyframes")).toBeTruthy();
  });

  it("adds a keyframe at the playhead, lists them in time order, and jumps to one", () => {
    const onSeek = vi.fn();
    const { rerender } = render(<Controls initial={[{ t_ms: 3000, x: 0.8, y: 0.8 }]} relMs={1000} onSeek={onSeek} />);
    fireEvent.click(screen.getByText("Keyframe at the playhead"));
    expect(path().map((k) => k.t_ms)).toEqual([1000, 3000]);
    fireEvent.click(screen.getByText("3 s"));
    expect(onSeek).toHaveBeenCalledWith(3000);
    rerender(<Controls initial={[]} />);
  });

  it("removes a keyframe, and turning it off drops them all", () => {
    render(
      <Controls
        initial={[
          { t_ms: 0, x: 0.1, y: 0.1 },
          { t_ms: 2000, x: 0.9, y: 0.9 },
        ]}
      />
    );
    fireEvent.click(screen.getByLabelText("Remove the keyframe at 0 s"));
    expect(path()).toEqual([{ t_ms: 2000, x: 0.9, y: 0.9 }]);
    fireEvent.click(screen.getByLabelText("Move over time"));
    expect(path()).toEqual([]);
  });
});

describe("in the Layers box", () => {
  function Harness() {
    const [layers, setLayers] = useState([]);
    const [selected, setSelected] = useState(null);
    return (
      <>
        <LayersPanel
          layers={layers}
          setLayers={setLayers}
          selectedId={selected}
          setSelectedId={setSelected}
          overlays={[]}
          shapes={[]}
          clipMs={CLIP}
          relMs={2000}
        />
        <output data-testid="sent">{JSON.stringify(layersForRequest(layers, CLIP))}</output>
      </>
    );
  }

  it("a layer can be given a path, which goes into the export", () => {
    render(<Harness />);
    fireEvent.click(screen.getByText("+ Arrow"));
    fireEvent.click(screen.getByLabelText("Move over time"));
    const [layer] = JSON.parse(screen.getByTestId("sent").textContent);
    expect(layer.path).toEqual([{ t_ms: 2000, x: 0.5, y: 0.5 }]);
    fireEvent.click(screen.getByLabelText("Move over time"));
    expect(JSON.parse(screen.getByTestId("sent").textContent)[0]).not.toHaveProperty("path");
  });
});

describe("in the Look controls", () => {
  function Harness() {
    const [look, setLook] = useState({ preset: "block" });
    return (
      <>
        <LookControls look={look} setLook={setLook} relMs={1000} clipMs={CLIP} />
        <output data-testid="sent">{JSON.stringify(lookForRequest(look, CLIP))}</output>
      </>
    );
  }
  const sent = () => JSON.parse(screen.getByTestId("sent").textContent);

  it("the text can follow a path, and then the fixed-place controls step aside", () => {
    render(<Harness />);
    fireEvent.click(screen.getByLabelText("Place the text myself"));
    fireEvent.change(screen.getByLabelText(/Across/), { target: { value: "30" } });
    fireEvent.click(screen.getByLabelText("Move over time"));
    // the first keyframe is where the text is now: the place just chosen
    expect(sent().path).toEqual([{ t_ms: 1000, x: 0.3, y: 0.8 }]);
    expect(screen.queryByLabelText("Place the text myself")).toBeNull();
    // turning the path off brings them back, with the place it had
    fireEvent.click(screen.getByLabelText("Move over time"));
    expect(sent()).not.toHaveProperty("path");
    expect(screen.getByLabelText("Place the text myself").checked).toBe(true);
  });

  it("starts from the bottom of the picture when the text was never placed", () => {
    render(<Harness />);
    fireEvent.click(screen.getByLabelText("Move over time"));
    expect(sent().path).toEqual([{ t_ms: 1000, x: 0.5, y: 0.8 }]);
  });
});

describe("dragging the styled caption on the player", () => {
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
      capStyle: { layer: { left: 0, top: 0, width: 400, height: 200 } },
      captionMode: "source",
      overlayText: "hello",
      lookView: { style: { position: "absolute" }, lines: [[{ text: "hello", emphasis: false, shown: true }]], emphasis: 1.5 },
      moveText: vi.fn(),
      relMs: 1500,
      top: "",
      bottom: "",
      playhead: 3500,
      srcFps: null,
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

  it("moves the text with the pointer, at the playhead's moment, kept inside the picture", () => {
    const { props, container } = player();
    const layer = container.querySelector(".cap-layer");
    layer.getBoundingClientRect = () => ({ left: 0, top: 0, width: 400, height: 200 });
    const text = container.querySelector(".cap-look");
    fireEvent.pointerDown(text, { pointerId: 1, clientX: 200, clientY: 100 });
    fireEvent.pointerMove(text, { pointerId: 1, clientX: 100, clientY: 50 });
    expect(props.moveText).toHaveBeenLastCalledWith({ x: 0.25, y: 0.25 }, 1500);
    fireEvent.pointerMove(text, { pointerId: 1, clientX: 5000, clientY: -9 });
    expect(props.moveText).toHaveBeenLastCalledWith({ x: 1, y: 0 }, 1500);
    fireEvent.pointerUp(text, { pointerId: 1 });
    props.moveText.mockClear();
    fireEvent.pointerMove(text, { pointerId: 1, clientX: 100, clientY: 50 });
    expect(props.moveText).not.toHaveBeenCalled();
  });

  it("does not move without a press", () => {
    const { props, container } = player();
    container.querySelector(".cap-layer").getBoundingClientRect = () => ({ left: 0, top: 0, width: 400, height: 200 });
    fireEvent.pointerMove(container.querySelector(".cap-look"), { pointerId: 1, clientX: 100, clientY: 50 });
    expect(props.moveText).not.toHaveBeenCalled();
  });
});
