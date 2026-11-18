// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { Waveform } from "./Waveform.jsx";
import { songWindow, startAtFraction } from "./musicModel.js";

afterEach(cleanup);

describe("where the playing stretch sits", () => {
  it("is a fraction of the song, kept inside it", () => {
    expect(songWindow(50_000, 10_000, 200_000)).toEqual({ left: 0.25, width: 0.05 });
    const edge = songWindow(195_000, 30_000, 200_000);
    expect(edge.left).toBeCloseTo(0.975, 9);
    expect(edge.width).toBeCloseTo(0.025, 9); // cut off at the end of the song
    expect(songWindow(0, 5000, 0)).toEqual({ left: 0, width: 0 });
  });
  it("is centred on a pick and never leaves the song", () => {
    expect(startAtFraction(0.5, 200_000, 10_000)).toBe(95_000);
    expect(startAtFraction(0, 200_000, 10_000)).toBe(0);
    expect(startAtFraction(1, 200_000, 10_000)).toBe(190_000);
    expect(startAtFraction(0.5, 3000, 8000)).toBe(0);
    expect(startAtFraction(NaN, 200_000, 10_000)).toBe(0);
  });
});

const peaks = [0.1, 0.9, 0.5, 1, 0.2, 0.7, 0.3, 0.6, 0.4, 0.8];

function wave(extra = {}) {
  const onPick = vi.fn();
  render(<Waveform peaks={peaks} songMs={100_000} startMs={20_000} playingMs={10_000} onPick={onPick} {...extra} />);
  const el = screen.getByRole("slider", { name: "Where in the song to start" });
  el.getBoundingClientRect = () => ({ left: 100, width: 400, top: 0, height: 50 });
  return { el, onPick };
}

describe("the waveform", () => {
  it("draws nothing until it has bars and a song length", () => {
    const { container } = render(<Waveform peaks={[]} songMs={1000} startMs={0} playingMs={500} onPick={() => {}} />);
    expect(container.querySelector("svg")).toBeNull();
    const again = render(<Waveform peaks={peaks} songMs={0} startMs={0} playingMs={500} onPick={() => {}} />);
    expect(again.container.querySelector("svg")).toBeNull();
  });

  it("highlights the stretch that plays and reports where it starts", () => {
    const { el } = wave();
    expect(el.getAttribute("aria-valuenow")).toBe("20000");
    expect(el.getAttribute("aria-valuetext")).toBe("0:20.0");
    const w = el.querySelector(".wave-window");
    expect(Number(w.getAttribute("x"))).toBeCloseTo(2, 6); // 10 bars, a fifth of the way in
    expect(Number(w.getAttribute("width"))).toBeCloseTo(1, 6);
    expect(el.querySelectorAll(".wave-bars").length).toBe(1);
  });

  it("moves the stretch to a click, centred on it", () => {
    const { el, onPick } = wave();
    fireEvent.pointerDown(el, { clientX: 300, pointerId: 1 }); // halfway
    expect(onPick).toHaveBeenLastCalledWith(45_000);
  });

  it("follows a drag but not a mouse that is up", () => {
    const { el, onPick } = wave();
    fireEvent.pointerMove(el, { clientX: 200, buttons: 0 });
    expect(onPick).not.toHaveBeenCalled();
    fireEvent.pointerMove(el, { clientX: 200, buttons: 1 });
    expect(onPick).toHaveBeenLastCalledWith(20_000);
  });

  it("nudges a second with the arrows and five with shift", () => {
    const { el, onPick } = wave();
    fireEvent.keyDown(el, { key: "ArrowRight" });
    expect(onPick).toHaveBeenLastCalledWith(21_000);
    fireEvent.keyDown(el, { key: "ArrowLeft", shiftKey: true });
    expect(onPick).toHaveBeenLastCalledWith(15_000);
    onPick.mockClear();
    fireEvent.keyDown(el, { key: "a" });
    expect(onPick).not.toHaveBeenCalled();
  });
});
