// @vitest-environment jsdom
import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { api } from "./api.js";
import { SequenceView } from "./SequenceView.jsx";
import { SongPreview } from "./SongPreview.jsx";
import { editorSeed, updatedClip } from "./sequenceClip.js";

const song = {
  id: "s1",
  title: "A Song",
  artist: "Band",
  duration_s: 200,
  cover_art: "al-1",
  start_ms: 30_000,
  at_ms: 500,
  volume: 0.7,
  fade_in_ms: 200,
  fade_out_ms: 0,
  duck: 0.6,
};
const clip = (id, start, end, extra = {}) => ({
  id,
  rating_key: `r-${id}`,
  title: id,
  start_ms: start,
  end_ms: end,
  caption: { mode: "none" },
  subtitle_offset_ms: 0,
  ...extra,
});

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

describe("a song on one section of a sequence", () => {
  const sequence = [clip("a", 0, 4000), clip("b", 10_000, 13_000, { music: song }), clip("c", 20_000, 22_000)];

  it("is sent with that section only, in the section's own time, and moves GIF to MP4", async () => {
    const start = vi.spyOn(api, "startSequenceExport").mockResolvedValue({ job_id: "j1" });
    render(<SequenceView sequence={sequence} setSequence={() => {}} onJob={() => {}} />);
    // a song needs sound: the default GIF has gone to MP4 on its own
    const button = await screen.findByText("Export MP4 (3 clips)");
    expect(screen.getByLabelText("GIF").disabled).toBe(true);
    await act(async () => {
      fireEvent.click(button);
    });
    expect(start).toHaveBeenCalledTimes(1);
    const sent = start.mock.calls[0][0];
    expect(sent.format).toBe("mp4");
    expect(sent.clips[0].music).toBeUndefined();
    expect(sent.clips[2].music).toBeUndefined();
    expect(sent.clips[1].music).toEqual({
      id: "s1",
      start_ms: 30_000,
      at_ms: 500,
      volume: 0.7,
      fade_in_ms: 200,
      fade_out_ms: 0,
      duck: 0, // ducking is for the clips' own sound, which isn't kept here
      pan: 0,
      highpass_hz: 0,
      lowpass_hz: 0,
      curve: [],
    });
    expect(sent.music).toBeUndefined(); // the sequence's own song is separate
  });

  it("ducks when the sequence keeps the clips' sound", async () => {
    const start = vi.spyOn(api, "startSequenceExport").mockResolvedValue({ job_id: "j1" });
    render(<SequenceView sequence={sequence} setSequence={() => {}} onJob={() => {}} />);
    fireEvent.click(await screen.findByLabelText(/Include audio/));
    await act(async () => {
      fireEvent.click(await screen.findByText("Export MP4 (3 clips)"));
    });
    expect(start.mock.calls[0][0].clips[1].music.duck).toBe(0.6);
  });

  it("is played with the player, with a switch to hear it, and none without songs", async () => {
    render(<SequenceView sequence={sequence} setSequence={() => {}} onJob={() => {}} />);
    await screen.findByText("Export MP4 (3 clips)");
    const audio = document.querySelectorAll("audio");
    expect([...audio].some((a) => /api\/music\/s1\/file$/.test(a.getAttribute("src")))).toBe(true);
    expect(screen.getByLabelText(/Hear the sections' songs in the player/).checked).toBe(true);
    cleanup();
    render(<SequenceView sequence={[clip("a", 0, 4000)]} setSequence={() => {}} onJob={() => {}} />);
    await screen.findByText(/Export GIF/);
    expect(screen.queryByLabelText(/Hear the sections' songs/)).toBeNull();
  });
});

describe("a section's song when the section is edited", () => {
  it("is what the editor starts from, and what it saves back", () => {
    const stored = clip("b", 0, 3000, { music: song });
    expect(editorSeed(stored).music).toBe(song);
    expect(editorSeed(clip("x", 0, 1000)).music).toBeNull();
    expect(editorSeed({ ...stored, music: { title: "no id" } }).music).toBeNull();
    const base = { start: 0, end: 3000, caption: { mode: "none" }, subtitleOffsetMs: 0 };
    expect(updatedClip(stored, { ...base, music: { ...song, volume: 0.2 } }).music.volume).toBe(0.2);
    expect(updatedClip(stored, base).music).toBeNull(); // taken off
  });
});

describe("a song playing along with a player", () => {
  const playing = () => document.querySelector("audio");
  const m = { ...song, start_ms: 0, at_ms: 1000, fade_in_ms: 0, duck: 0 };

  it("plays inside its clip and stops when the player goes on past the end of that clip", () => {
    const { rerender } = render(<SongPreview music={m} relMs={2000} clipMs={4000} playing />);
    expect(window.HTMLMediaElement.prototype.play).toHaveBeenCalled();
    window.HTMLMediaElement.prototype.pause.mockClear();
    rerender(<SongPreview music={m} relMs={4500} clipMs={4000} playing />);
    expect(window.HTMLMediaElement.prototype.pause).toHaveBeenCalled(); // the next section's time
    expect(playing().getAttribute("src")).toMatch(/api\/music\/s1\/file$/);
  });

  it("waits before the song comes in, and makes no sound when not wanted", () => {
    window.HTMLMediaElement.prototype.play.mockClear();
    const { rerender } = render(<SongPreview music={m} relMs={500} clipMs={4000} playing />);
    expect(window.HTMLMediaElement.prototype.play).not.toHaveBeenCalled();
    rerender(<SongPreview music={m} relMs={2000} clipMs={4000} playing hear={false} />);
    expect(window.HTMLMediaElement.prototype.play).not.toHaveBeenCalled();
    expect(playing().volume).toBe(0);
  });

  it("draws nothing without a song", () => {
    const { container } = render(<SongPreview music={null} relMs={0} clipMs={1000} playing />);
    expect(container.firstChild).toBeNull();
  });
});
