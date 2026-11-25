// @vitest-environment jsdom
//
// The Tracks strip inside the whole editor: it is there for a plain clip, and its buttons reach the boxes
// that edit each lane.
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

// Nothing answers: the editor shows its first state, which is all these checks need.
vi.mock("./api.js", () => ({
  api: new Proxy(
    {},
    {
      get: (t, k) =>
        (t[k] ??= vi.fn(() => (String(k).endsWith("Url") ? "" : new Promise(() => {})))),
    }
  ),
}));

import { api } from "./api.js";
import { Editor } from "./Editor.jsx";

const clip = {
  rating_key: "k1",
  title: "Pilot",
  line: "Say it with pie",
  start_ms: 1000,
  end_ms: 4000,
};

beforeEach(() => {
  window.HTMLElement.prototype.scrollIntoView = vi.fn();
  window.HTMLMediaElement.prototype.load = vi.fn();
  window.HTMLMediaElement.prototype.pause = vi.fn();
});
afterEach(cleanup);

const open = () => render(<Editor item={clip} onClose={() => {}} onJob={() => {}} />);

describe("the Tracks strip in the editor", () => {
  it("is there with no song and no clip sound, with the captions and the empty lanes", () => {
    open();
    expect(screen.getByLabelText("Tracks")).toBeTruthy();
    // the subtitle lines haven't arrived yet in this first state
    expect(screen.getByText("No captions on this clip.")).toBeTruthy();
    expect(screen.getByText("Nothing laid over the clip.")).toBeTruthy();
    expect(screen.getByText("The clip's own sound is off.")).toBeTruthy();
    expect(screen.getByText("No song.")).toBeTruthy();
  });

  it("draws the subtitle lines said during the clip as bars, shifted to clip time", async () => {
    api.media.mockResolvedValueOnce({ duration_ms: 60_000 });
    api.videoInfo.mockRejectedValueOnce(new Error("no info"));
    api.mediaSubtitles.mockResolvedValueOnce({
      cues: [
        { start_ms: 1500, end_ms: 2500, text: "Said inside the clip" },
        { start_ms: 30_000, end_ms: 31_000, text: "Said much later" },
      ],
    });
    open();
    const lane = screen.getByLabelText("Captions over the clip");
    await waitFor(() => expect(lane.textContent).toContain("Said inside the clip"));
    expect(lane.textContent).not.toContain("Said much later");
  });

  it("draws meme text as a bar the clip's length", () => {
    open();
    fireEvent.click(screen.getByLabelText("Meme text"));
    const lane = screen.getByLabelText("Captions over the clip");
    expect(lane.textContent).toContain("Bottom: Say it with pie");
  });

  it("turns the clip's sound on from its lane", () => {
    open();
    fireEvent.click(screen.getByText("Include clip sound"));
    expect(screen.queryByText("The clip's own sound is off.")).toBeNull();
    expect(screen.getByLabelText("Clip sound volume over the clip")).toBeTruthy();
  });

  it("takes you to the Music and Layers boxes from the empty lanes", () => {
    open();
    fireEvent.click(screen.getByText("Add a song"));
    fireEvent.click(screen.getByText("Add a layer"));
    expect(window.HTMLElement.prototype.scrollIntoView).toHaveBeenCalledTimes(2);
  });

  it("shows a layer on its own lane and selects it from there", () => {
    open();
    fireEvent.click(screen.getAllByText(/^\+ /).find((b) => b.closest("#layers-box")));
    expect(screen.queryByText("Nothing laid over the clip.")).toBeNull();
    const bar = screen.getByLabelText("Tracks").querySelector(".mix-bar");
    expect(bar).toBeTruthy();
    fireEvent.click(bar);
    expect(bar.getAttribute("aria-pressed")).toBe("true");
  });
});
