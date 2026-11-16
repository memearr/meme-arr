// @vitest-environment jsdom
//
// Following a spot automatically: merging what the tracker found into a path, and the
// "Follow it automatically" button.
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { useState } from "react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { api } from "./api.js";
import { PathControls } from "./PathControls.jsx";
import { MAX_PATH_POINTS, mergeTracked, trackLimit } from "./pathModel.js";

afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
});

const CLIP = 5000;
const k = (t_ms, x, y) => ({ t_ms, x, y });

describe("merging a tracked path", () => {
  it("keeps the keyframes before where tracking started and takes over from there", () => {
    const mine = [k(0, 0.1, 0.1), k(1000, 0.2, 0.2), k(2000, 0.3, 0.3), k(4000, 0.9, 0.9)];
    const found = [k(2000, 0.35, 0.35), k(3000, 0.5, 0.5)];
    // the ones from 2 s on (mine) are replaced by what was followed
    expect(mergeTracked(mine, found, CLIP)).toEqual([k(0, 0.1, 0.1), k(1000, 0.2, 0.2), k(2000, 0.35, 0.35), k(3000, 0.5, 0.5)]);
  });

  it("replaces a keyframe put at about the moment it started (the one that marks the spot)", () => {
    const mine = [k(0, 0.1, 0.1), k(1980, 0.3, 0.3)];
    const found = [k(2000, 0.31, 0.3), k(2500, 0.6, 0.6)];
    expect(mergeTracked(mine, found, CLIP).map((f) => f.t_ms)).toEqual([0, 2000, 2500]);
  });

  it("is just the tracked path when there was none, and leaves the path alone when nothing was found", () => {
    const found = [k(0, 0.2, 0.2), k(1000, 0.4, 0.4)];
    expect(mergeTracked(undefined, found, CLIP)).toEqual(found);
    expect(mergeTracked([k(0, 0.5, 0.5)], [], CLIP)).toEqual([k(0, 0.5, 0.5)]);
    expect(mergeTracked(undefined, undefined, CLIP)).toEqual([]);
  });

  it("asks the tracker for no more keyframes than there is room for", () => {
    expect(trackLimit([], 1000)).toBe(MAX_PATH_POINTS);
    expect(trackLimit([k(0, 0, 0), k(500, 0, 0), k(2000, 0, 0)], 1000)).toBe(MAX_PATH_POINTS - 2);
    const many = Array.from({ length: MAX_PATH_POINTS }, (_, i) => k(i, 0, 0));
    expect(trackLimit(many, 99_999)).toBe(2);
  });
});

describe("the request to the server", () => {
  it("is a POST of the clip, the moment and the spot", async () => {
    const fetchSpy = vi.spyOn(globalThis, "fetch").mockResolvedValue(
      new Response(JSON.stringify({ path: [], tracked_ms: 0, lost: false }), { status: 200 })
    );
    await api.track({ rating_key: "7", start_ms: 0, end_ms: 4000, t_ms: 500, x: 0.4, y: 0.6, size: 0.12, max_points: 80 });
    const [url, init] = fetchSpy.mock.calls[0];
    expect(String(url)).toMatch(/api\/track$/);
    expect(init.method).toBe("POST");
    expect(JSON.parse(init.body)).toEqual({ rating_key: "7", start_ms: 0, end_ms: 4000, t_ms: 500, x: 0.4, y: 0.6, size: 0.12, max_points: 80 });
  });
});

describe("Follow it automatically", () => {
  function Controls({ onTrack, initial = [], relMs = 1000 }) {
    const [path, setPath] = useState(initial);
    return (
      <>
        <PathControls
          path={path}
          onChange={setPath}
          onTrack={onTrack}
          place={{ x: 0.3, y: 0.6 }}
          relMs={relMs}
          clipMs={CLIP}
          what="the arrow"
        />
        <div data-testid="path">{JSON.stringify(path)}</div>
      </>
    );
  }
  const path = () => JSON.parse(screen.getByTestId("path").textContent);

  it("is not offered when there is nothing to ask", () => {
    render(<Controls />);
    expect(screen.queryByText("Follow it automatically")).toBeNull();
  });

  it("asks for the spot under the thing at the playhead, and takes the path it gets", async () => {
    const onTrack = vi.fn().mockResolvedValue({
      path: [k(1000, 0.3, 0.6), k(3000, 0.8, 0.4)],
      tracked_ms: 4000,
      lost: false,
    });
    render(<Controls onTrack={onTrack} relMs={1000} />);
    fireEvent.change(screen.getByLabelText("Spot size"), { target: { value: "0.2" } });
    fireEvent.click(screen.getByText("Follow it automatically"));
    // while it works it cannot be pressed again
    expect(screen.getByText("Following…").disabled).toBe(true);
    await waitFor(() => expect(path()).toEqual([k(1000, 0.3, 0.6), k(3000, 0.8, 0.4)]));
    expect(onTrack).toHaveBeenCalledWith({ place: { x: 0.3, y: 0.6 }, relMs: 1000, size: 0.2, maxPoints: MAX_PATH_POINTS });
    expect(screen.getByRole("status").textContent).toContain("Followed it for 4 s, to the end of the clip.");
    expect(screen.getByText("Follow it automatically").disabled).toBe(false);
  });

  it("keeps the keyframes from before, and leaves room for them", async () => {
    const onTrack = vi.fn().mockResolvedValue({ path: [k(2000, 0.5, 0.5), k(4000, 0.9, 0.9)], tracked_ms: 3000, lost: false });
    render(<Controls onTrack={onTrack} initial={[k(0, 0.1, 0.1), k(1000, 0.2, 0.2)]} relMs={2000} />);
    fireEvent.click(screen.getByText("Follow it automatically"));
    await waitFor(() => expect(path().map((f) => f.t_ms)).toEqual([0, 1000, 2000, 4000]));
    expect(onTrack.mock.calls[0][0].maxPoints).toBe(MAX_PATH_POINTS - 2);
  });

  it("says when it lost the spot, and keeps what it followed", async () => {
    const onTrack = vi.fn().mockResolvedValue({ path: [k(1000, 0.3, 0.6), k(2200, 0.5, 0.6)], tracked_ms: 1200, lost: true });
    render(<Controls onTrack={onTrack} />);
    fireEvent.click(screen.getByText("Follow it automatically"));
    await waitFor(() => expect(path()).toHaveLength(2));
    const note = screen.getByRole("status").textContent;
    expect(note).toContain("Followed it for 1.2 s, then lost it");
    expect(note).toContain("put the arrow back on it");
  });

  it("shows why it could not follow, and changes nothing", async () => {
    const onTrack = vi.fn().mockRejectedValue(new Error("there is nothing to follow at that spot (the picture is flat there)"));
    render(<Controls onTrack={onTrack} initial={[k(0, 0.5, 0.5)]} />);
    fireEvent.click(screen.getByText("Follow it automatically"));
    await waitFor(() => expect(screen.getByRole("status").textContent).toContain("nothing to follow"));
    expect(path()).toEqual([k(0, 0.5, 0.5)]);
    expect(screen.getByText("Follow it automatically").disabled).toBe(false);
  });

  it("does not change the path when the server finds no keyframes", async () => {
    const onTrack = vi.fn().mockResolvedValue({ path: [], tracked_ms: 0, lost: true });
    render(<Controls onTrack={onTrack} initial={[k(0, 0.5, 0.5)]} />);
    fireEvent.click(screen.getByText("Follow it automatically"));
    await waitFor(() => expect(screen.getByRole("status").textContent).toContain("Nothing could be followed"));
    expect(path()).toEqual([k(0, 0.5, 0.5)]);
  });
});
