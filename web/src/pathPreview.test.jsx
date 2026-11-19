// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { PathPreview } from "./PathPreview.jsx";
import { PlayerPanel } from "./PlayerPanel.jsx";

afterEach(cleanup);

const box = { left: 10, top: 5, width: 400, height: 225 };
const route = [
  { t_ms: 1000, x: 0.1, y: 0.2 },
  { t_ms: 3000, x: 0.5, y: 0.5 },
  { t_ms: 5000, x: 0.9, y: 0.1 },
];
const paths = [
  { id: "a", path: route, selected: true, label: "Arrow" },
  { id: "caption-text", path: [{ t_ms: 0, x: 0.5, y: 0.8 }, { t_ms: 2000, x: 0.5, y: 0.2 }], selected: false, label: "Caption text" },
];

describe("the motion path preview", () => {
  it("draws nothing without a picture box or any path", () => {
    const { container } = render(<PathPreview paths={paths} relMs={0} pictureBox={null} />);
    expect(container.firstChild).toBeNull();
    const none = render(<PathPreview paths={[{ id: "x", path: [], label: "x" }]} relMs={0} pictureBox={box} />);
    expect(none.container.firstChild).toBeNull();
  });

  it("sits on the picture and draws each route, the way gone solid and the way ahead dashed", () => {
    const { container } = render(<PathPreview paths={paths} relMs={2000} pictureBox={box} />);
    const wrap = container.querySelector(".path-preview");
    expect(wrap.style.left).toBe("10px");
    expect(wrap.style.width).toBe("400px");
    const arrow = container.querySelector('[data-path="a"]');
    expect(arrow.querySelector("polyline.done").getAttribute("points")).toBe("10,20 30,35");
    expect(arrow.querySelector("polyline.ahead").getAttribute("points")).toBe("30,35 50,50 90,10");
    expect(arrow.querySelector("polyline").className.baseVal).toContain("selected");
    const text = container.querySelector('[data-path="caption-text"]');
    expect(text.querySelector("polyline.done")).toBeTruthy(); // the caption's route is gone by 2 s
    expect(text.querySelector("polyline.ahead")).toBeNull();
  });

  it("puts a dot at every keyframe, the ends marked, and a click goes to that moment", () => {
    const onSeek = vi.fn();
    render(<PathPreview paths={paths} relMs={0} pictureBox={box} onSeek={onSeek} />);
    const dots = screen.getAllByRole("button");
    expect(dots).toHaveLength(5);
    const mid = screen.getByLabelText("Arrow: keyframe at 3 s");
    expect(mid.style.left).toBe("50%");
    expect(mid.style.top).toBe("50%");
    expect(mid.className).not.toMatch(/first|last/);
    expect(screen.getByLabelText("Arrow: keyframe at 1 s").className).toContain("first");
    expect(screen.getByLabelText("Arrow: keyframe at 5 s").className).toContain("last");
    fireEvent.click(mid);
    expect(onSeek).toHaveBeenCalledWith(3000);
  });

  it("shows a single keyframe as a dot with no line", () => {
    const { container } = render(
      <PathPreview paths={[{ id: "one", path: [{ t_ms: 500, x: 0.4, y: 0.4 }], label: "Dot" }]} relMs={0} pictureBox={box} />
    );
    expect(container.querySelector("polyline")).toBeNull();
    expect(screen.getAllByRole("button")).toHaveLength(1);
  });
});

describe("the player's switch for the paths", () => {
  const props = {
    proxy: null,
    volume: { level: 1, muted: false },
    speed: 1,
    start: 0,
    end: 4000,
    playhead: 0,
    relMs: 0,
    seekTo: () => {},
    stepPlayhead: () => {},
    togglePlay: () => {},
    setSpeed: () => {},
    setVolume: () => {},
  };
  it("is offered only when something travels, and says what it does", () => {
    render(<PlayerPanel {...props} paths={[]} showPaths />);
    expect(screen.queryByLabelText(/Show motion paths/)).toBeNull();
    cleanup();
    const set = vi.fn();
    render(<PlayerPanel {...props} paths={paths} showPaths setShowPaths={set} />);
    const box = screen.getByLabelText(/Show motion paths/);
    expect(box.checked).toBe(true);
    fireEvent.click(box);
    expect(set).toHaveBeenCalledWith(false);
  });
});
