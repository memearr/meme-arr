// @vitest-environment jsdom
import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { api } from "./api.js";
import SequencePlayer from "./SequencePlayer.jsx";

const clip = (id, start, end) => ({
  id,
  rating_key: "r1",
  title: id,
  start_ms: start,
  end_ms: end,
  caption: { mode: "none" },
});
const sequence = [clip("a", 0, 4000), clip("b", 10_000, 13_000)];

beforeEach(() => {
  vi.spyOn(api, "fetchBlobUrl").mockResolvedValue("blob:section");
  vi.spyOn(window.HTMLMediaElement.prototype, "play").mockImplementation(() => Promise.resolve());
  vi.spyOn(window.HTMLMediaElement.prototype, "pause").mockImplementation(() => {});
  URL.revokeObjectURL = vi.fn();
});
afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
});

describe("the sequence player tells what it is doing", () => {
  it("reports playing and pausing, so the music can follow it", async () => {
    const onPlaying = vi.fn();
    render(<SequencePlayer sequence={sequence} width={480} fps={15} index={0} onIndex={() => {}} onPlaying={onPlaying} />);
    expect(onPlaying).toHaveBeenLastCalledWith(false);
    const play = await screen.findByLabelText("Play the sequence");
    await waitFor(() => expect(play.disabled).toBe(false));
    fireEvent.click(play);
    expect(onPlaying).toHaveBeenLastCalledWith(true);
    // jsdom's mocked play() doesn't move the video out of "paused"
    Object.defineProperty(document.querySelector("video"), "paused", { value: false, configurable: true });
    fireEvent.click(screen.getByLabelText("Pause"));
    expect(onPlaying).toHaveBeenLastCalledWith(false);
  });

  it("reports the playhead in the sequence's time, across sections", async () => {
    const onProgress = vi.fn();
    const { rerender } = render(
      <SequencePlayer sequence={sequence} width={480} fps={15} index={0} onIndex={() => {}} onProgress={onProgress} />
    );
    await waitFor(() => expect(onProgress).toHaveBeenCalledWith(0));
    const video = await waitFor(() => {
      const v = document.querySelector("video");
      expect(v).toBeTruthy();
      return v;
    });
    Object.defineProperty(video, "currentTime", { value: 1.5, configurable: true });
    act(() => {
      fireEvent.timeUpdate(video);
    });
    expect(onProgress).toHaveBeenLastCalledWith(1500);
    rerender(<SequencePlayer sequence={sequence} width={480} fps={15} index={1} onIndex={() => {}} onProgress={onProgress} />);
    await waitFor(() => expect(onProgress).toHaveBeenLastCalledWith(4000)); // the second section starts after the first's 4 s
  });
});
