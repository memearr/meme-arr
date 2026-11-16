// @vitest-environment jsdom
//
// The Layers box and the layers over the player: add, select, adjust, remove, upload,
// and drag on the picture.
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { useState } from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { api } from "./api.js";
import { LayerOverlay } from "./LayerOverlay.jsx";
import { LayersPanel } from "./LayersPanel.jsx";
import { layersForRequest } from "./layersModel.js";

afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
});

const CLIP = 5000;
const OVERLAYS = [{ id: "cat-1a2b3c4d.gif", name: "cat", kind: "motion", bytes: 10 }];

/** The panel with real state, plus what an export would send for it. */
function Harness({ relMs = 1000, overlays = OVERLAYS, reload = vi.fn() }) {
  const [layers, setLayers] = useState([]);
  const [selected, setSelected] = useState(null);
  return (
    <>
      <LayersPanel
        layers={layers}
        setLayers={setLayers}
        selectedId={selected}
        setSelectedId={setSelected}
        overlays={overlays}
        shapes={[]}
        reloadOverlays={reload}
        clipMs={CLIP}
        relMs={relMs}
      />
      <output data-testid="sent">{JSON.stringify(layersForRequest(layers, CLIP))}</output>
    </>
  );
}
const sent = () => JSON.parse(screen.getByTestId("sent").textContent);

describe("Layers box", () => {
  it("starts empty with a hint", () => {
    render(<Harness />);
    expect(sent()).toEqual([]);
    expect(screen.getByText(/Add an arrow/)).toBeTruthy();
  });

  it("adds a shape where the playhead is and selects it", () => {
    render(<Harness relMs={1500} />);
    fireEvent.click(screen.getByText("+ Arrow"));
    expect(sent()).toMatchObject([{ kind: "shape", shape: "arrow", start_ms: 1500, end_ms: 3500 }]);
    // selected: its controls are there
    expect(screen.getByText(/Size/)).toBeTruthy();
    expect(screen.getByText("Drag it on the player to move it.")).toBeTruthy();
  });

  it("changes size, turn, opacity and colour of the selected layer", () => {
    render(<Harness />);
    fireEvent.click(screen.getByText("+ Ring"));
    fireEvent.change(screen.getByLabelText(/Size/), { target: { value: "0.5" } });
    fireEvent.change(screen.getByLabelText(/Turn/), { target: { value: "90" } });
    fireEvent.change(screen.getByLabelText(/Opacity/), { target: { value: "0.5" } });
    fireEvent.change(screen.getByLabelText("Colour"), { target: { value: "#00ff00" } });
    expect(sent()[0]).toMatchObject({ size: 0.5, rotation: 90, opacity: 0.5, color: "#00ff00" });
  });

  it("sets the times from the number boxes and from the playhead", () => {
    render(<Harness relMs={2400} />);
    fireEvent.click(screen.getByText("+ Box"));
    fireEvent.change(screen.getByLabelText("On at (s)"), { target: { value: "0.5" } });
    fireEvent.change(screen.getByLabelText("Off at (s)"), { target: { value: "4" } });
    expect(sent()[0]).toMatchObject({ start_ms: 500, end_ms: 4000 });
    fireEvent.click(screen.getAllByText("= playhead")[1]);
    expect(sent()[0].end_ms).toBe(2400);
    // putting the start at the playhead (where the end already is) keeps the layer a little long
    fireEvent.click(screen.getAllByText("= playhead")[0]);
    expect(sent()[0]).toMatchObject({ start_ms: 2400, end_ms: 2500 });
  });

  it("lists layers, switches the selection and removes one", () => {
    render(<Harness />);
    fireEvent.click(screen.getByText("+ Arrow"));
    fireEvent.click(screen.getByText("+ Dot"));
    expect(sent().map((l) => l.shape)).toEqual(["arrow", "dot"]);
    fireEvent.click(screen.getByLabelText("Remove Arrow (up)"));
    expect(sent().map((l) => l.shape)).toEqual(["dot"]);
    fireEvent.click(screen.getByLabelText("Remove Dot"));
    expect(sent()).toEqual([]);
    expect(screen.queryByText("Drag it on the player to move it.")).toBeNull();
  });

  it("adds an uploaded picture from the list", () => {
    render(<Harness />);
    fireEvent.change(screen.getByLabelText("Add an uploaded picture or clip"), {
      target: { value: "cat-1a2b3c4d.gif" },
    });
    expect(sent()).toMatchObject([{ kind: "media", asset: "cat-1a2b3c4d.gif" }]);
  });

  it("uploads a file, reloads the list and adds it as a layer", async () => {
    const upload = vi
      .spyOn(api, "uploadOverlay")
      .mockResolvedValue({ overlay: { id: "new-9f8e7d6c.png", name: "new", kind: "still" } });
    const reload = vi.fn().mockResolvedValue(undefined);
    render(<Harness reload={reload} />);
    const file = new File([new Uint8Array([1, 2, 3])], "new.png", { type: "image/png" });
    fireEvent.change(screen.getByLabelText(/Upload/), { target: { files: [file] } });
    await waitFor(() => expect(sent()).toMatchObject([{ kind: "media", asset: "new-9f8e7d6c.png" }]));
    expect(upload).toHaveBeenCalledWith(file);
    expect(reload).toHaveBeenCalled();
  });

  it("shows why an upload was refused and adds nothing", async () => {
    vi.spyOn(api, "uploadOverlay").mockRejectedValue(new Error("only PNG, JPEG, GIF, WebP, MP4 and WebM files can be used"));
    render(<Harness />);
    const file = new File(["x"], "evil.exe");
    fireEvent.change(screen.getByLabelText(/Upload/), { target: { files: [file] } });
    await waitFor(() => expect(screen.getByRole("alert").textContent).toContain("only PNG"));
    expect(sent()).toEqual([]);
  });

  it("stops adding at the limit", () => {
    render(<Harness />);
    for (let i = 0; i < 12; i++) fireEvent.click(screen.getByText("+ Dot"));
    expect(sent()).toHaveLength(12);
    expect(screen.getByText("+ Dot").disabled).toBe(true);
  });
});

describe("layers over the player", () => {
  const box = { left: 10, top: 20, width: 400, height: 200 };
  const layer = (over = {}) => ({
    id: "a",
    kind: "shape",
    shape: "arrow",
    start_ms: 1000,
    end_ms: 3000,
    x: 0.5,
    y: 0.5,
    size: 0.2,
    rotation: 0,
    opacity: 1,
    ...over,
  });

  it("shows a layer only while it is on", () => {
    const { container, rerender } = render(<LayerOverlay layers={[layer()]} relMs={500} pictureBox={box} />);
    expect(container.querySelector("[data-layer]")).toBeNull();
    rerender(<LayerOverlay layers={[layer()]} relMs={1500} pictureBox={box} />);
    const el = container.querySelector("[data-layer='a']");
    expect(el).not.toBeNull();
    expect(el.getAttribute("src")).toContain("/api/sprites/arrow.png");
    expect(el.style.left).toBe("50%");
    rerender(<LayerOverlay layers={[layer()]} relMs={3000} pictureBox={box} />);
    expect(container.querySelector("[data-layer]")).toBeNull();
  });

  it("sits over the picture, not the letterbox", () => {
    const { container } = render(<LayerOverlay layers={[layer()]} relMs={1500} pictureBox={box} />);
    const el = container.querySelector(".layer-overlay");
    expect([el.style.left, el.style.top, el.style.width, el.style.height]).toEqual(["10px", "20px", "400px", "200px"]);
  });

  it("plays an uploaded video and shows an uploaded picture", () => {
    const { container } = render(
      <LayerOverlay
        layers={[
          layer({ id: "v", kind: "media", asset: "clip-1.mp4" }),
          layer({ id: "p", kind: "media", asset: "pic-1.gif" }),
        ]}
        relMs={1500}
        pictureBox={box}
      />
    );
    expect(container.querySelector("video[data-layer='v']")).not.toBeNull();
    expect(container.querySelector("img[data-layer='p']")).not.toBeNull();
  });

  it("selects on a press and moves with the pointer, kept inside the picture", () => {
    const onSelect = vi.fn();
    const onMove = vi.fn();
    const { container } = render(
      <LayerOverlay layers={[layer()]} relMs={1500} pictureBox={box} selectedId={null} onSelect={onSelect} onMove={onMove} />
    );
    const stage = container.querySelector(".layer-overlay");
    stage.getBoundingClientRect = () => ({ left: 10, top: 20, width: 400, height: 200 });
    const el = container.querySelector("[data-layer='a']");
    fireEvent.pointerDown(el, { pointerId: 1, clientX: 210, clientY: 120 });
    expect(onSelect).toHaveBeenCalledWith("a");
    fireEvent.pointerMove(el, { pointerId: 1, clientX: 110, clientY: 70 });
    expect(onMove).toHaveBeenLastCalledWith("a", { x: 0.25, y: 0.25 });
    fireEvent.pointerMove(el, { pointerId: 1, clientX: 9999, clientY: -50 });
    expect(onMove).toHaveBeenLastCalledWith("a", { x: 1, y: 0 });
    // after the pointer is released it no longer drags
    fireEvent.pointerUp(el, { pointerId: 1 });
    onMove.mockClear();
    fireEvent.pointerMove(el, { pointerId: 1, clientX: 110, clientY: 70 });
    expect(onMove).not.toHaveBeenCalled();
  });

  it("marks the selected layer and draws nothing without a picture box or layers", () => {
    const { container, rerender } = render(
      <LayerOverlay layers={[layer()]} relMs={1500} pictureBox={box} selectedId="a" />
    );
    expect(container.querySelector("[data-layer='a'].selected")).not.toBeNull();
    rerender(<LayerOverlay layers={[layer()]} relMs={1500} pictureBox={null} />);
    expect(container.firstChild).toBeNull();
    rerender(<LayerOverlay layers={[]} relMs={1500} pictureBox={box} />);
    expect(container.firstChild).toBeNull();
  });
});
