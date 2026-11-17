// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { useState } from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { api } from "./api.js";
import { MusicPanel } from "./MusicPanel.jsx";
import { musicForRequest } from "./musicModel.js";

const CLIP = 5000;
const tracks = [
  { id: "s1", title: "A Song", artist: "Band", album: "LP", duration_s: 200 },
  { id: "s2", title: "Other", artist: "", album: "", duration_s: 0 },
];

beforeEach(() => {
  // jsdom has no media playback
  vi.spyOn(window.HTMLMediaElement.prototype, "play").mockImplementation(() => Promise.resolve());
  vi.spyOn(window.HTMLMediaElement.prototype, "pause").mockImplementation(() => {});
});
afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
});

function Harness({ audio = false, playing = false, relMs = 0, onSeen }) {
  const [music, setMusic] = useState(null);
  const [clipVolume, setClipVolume] = useState(1);
  onSeen?.({ music, clipVolume });
  return (
    <MusicPanel
      music={music}
      onChoose={setMusic}
      onClear={() => setMusic(null)}
      onChange={setMusic}
      clipVolume={clipVolume}
      setClipVolume={setClipVolume}
      audio={audio}
      clipMs={CLIP}
      relMs={relMs}
      playing={playing}
    />
  );
}

describe("the Music box", () => {
  it("points to Settings when Navidrome is not set up, and offers no search", async () => {
    vi.spyOn(api, "getNavidromeSettings").mockResolvedValue({ configured: false });
    render(<Harness />);
    await waitFor(() => expect(screen.getByText(/Connect Navidrome/)).toBeTruthy());
    expect(screen.queryByLabelText("Search your music")).toBeNull();
  });

  it("searches, lists songs, and a chosen song gets its controls", async () => {
    vi.spyOn(api, "getNavidromeSettings").mockResolvedValue({ configured: true });
    const search = vi.spyOn(api, "musicSearch").mockResolvedValue({ tracks });
    let seen;
    render(<Harness onSeen={(s) => (seen = s)} />);
    const box = await screen.findByLabelText("Search your music");
    fireEvent.change(box, { target: { value: " song " } });
    fireEvent.click(screen.getByText("Search"));
    await screen.findByText("A Song");
    expect(search).toHaveBeenCalledWith("song");
    expect(screen.getByText(/3:20/)).toBeTruthy();

    fireEvent.click(screen.getByText("A Song"));
    expect(seen.music).toMatchObject({ id: "s1", volume: 1, start_ms: 0, at_ms: 0 });
    // the controls, and the search is put away
    expect(screen.getByText("Music volume")).toBeTruthy();
    expect(screen.getByText("Start the song at")).toBeTruthy();
    expect(screen.getByText("Comes in at")).toBeTruthy();
    expect(screen.getByText("Fade in")).toBeTruthy();
    expect(screen.getByText("Fade out")).toBeTruthy();
    expect(screen.queryByLabelText("Search your music")).toBeNull();
    // the song is in the page for the preview
    expect(document.querySelector("audio").getAttribute("src")).toMatch(/api\/music\/s1\/file$/);
  });

  it("changes the volume, and what it sends is what was set", async () => {
    vi.spyOn(api, "getNavidromeSettings").mockResolvedValue({ configured: true });
    vi.spyOn(api, "musicSearch").mockResolvedValue({ tracks });
    let seen;
    render(<Harness onSeen={(s) => (seen = s)} />);
    fireEvent.change(await screen.findByLabelText("Search your music"), { target: { value: "x" } });
    fireEvent.click(screen.getByText("Search"));
    fireEvent.click(await screen.findByText("A Song"));
    fireEvent.change(screen.getByLabelText(/Music volume/), { target: { value: "0.5" } });
    fireEvent.change(screen.getByLabelText(/Comes in at/), { target: { value: "1000" } });
    fireEvent.change(screen.getByLabelText(/Fade out/), { target: { value: "1500" } });
    expect(musicForRequest(seen.music, CLIP)).toEqual({
      id: "s1",
      start_ms: 0,
      at_ms: 1000,
      volume: 0.5,
      fade_in_ms: 0,
      fade_out_ms: 1500,
    });
    expect(screen.getByText("50%")).toBeTruthy();
  });

  it("removes the song and goes back to searching", async () => {
    vi.spyOn(api, "getNavidromeSettings").mockResolvedValue({ configured: true });
    vi.spyOn(api, "musicSearch").mockResolvedValue({ tracks });
    render(<Harness />);
    fireEvent.change(await screen.findByLabelText("Search your music"), { target: { value: "x" } });
    fireEvent.click(screen.getByText("Search"));
    fireEvent.click(await screen.findByText("A Song"));
    fireEvent.click(screen.getByText("Remove"));
    expect(await screen.findByLabelText("Search your music")).toBeTruthy();
    expect(document.querySelector("audio")).toBeNull();
  });

  it("says when nothing is found, and why a search failed", async () => {
    vi.spyOn(api, "getNavidromeSettings").mockResolvedValue({ configured: true });
    const search = vi.spyOn(api, "musicSearch").mockResolvedValueOnce({ tracks: [] }).mockRejectedValueOnce(new Error("Wrong username or password"));
    render(<Harness />);
    fireEvent.change(await screen.findByLabelText("Search your music"), { target: { value: "x" } });
    fireEvent.click(screen.getByText("Search"));
    expect(await screen.findByText("No songs found.")).toBeTruthy();
    fireEvent.click(screen.getByText("Search"));
    expect((await screen.findByRole("alert")).textContent).toContain("Wrong username or password");
    expect(search).toHaveBeenCalledTimes(2);
  });

  it("the clip's volume only moves when its sound is kept", async () => {
    vi.spyOn(api, "getNavidromeSettings").mockResolvedValue({ configured: true });
    const { rerender } = render(<Harness audio={false} />);
    const slider = await screen.findByLabelText(/Clip sound volume/);
    expect(slider.disabled).toBe(true);
    expect(screen.getByText(/Tick/)).toBeTruthy();
    rerender(<Harness audio />);
    expect(screen.getByLabelText(/Clip sound volume/).disabled).toBe(false);
  });
});

describe("hearing it in the player", () => {
  async function withSong(props) {
    vi.spyOn(api, "getNavidromeSettings").mockResolvedValue({ configured: true });
    vi.spyOn(api, "musicSearch").mockResolvedValue({ tracks });
    const view = render(<Harness {...props} />);
    fireEvent.change(await screen.findByLabelText("Search your music"), { target: { value: "x" } });
    fireEvent.click(screen.getByText("Search"));
    fireEvent.click(await screen.findByText("A Song"));
    return view;
  }

  it("plays while the clip plays, from the place in the song that matches, and stops with it", async () => {
    const view = await withSong({ playing: false, relMs: 1500 });
    const audio = document.querySelector("audio");
    expect(window.HTMLMediaElement.prototype.play).not.toHaveBeenCalled();
    view.rerender(<Harness playing relMs={1500} />);
    await waitFor(() => expect(window.HTMLMediaElement.prototype.play).toHaveBeenCalled());
    expect(audio.currentTime).toBeCloseTo(1.5, 1);
    view.rerender(<Harness playing={false} relMs={1500} />);
    await waitFor(() => expect(window.HTMLMediaElement.prototype.pause).toHaveBeenCalled());
  });

  it("is silent until the song comes in", async () => {
    const view = await withSong({ playing: false, relMs: 0 });
    // coming in at 2 s: nothing yet at 1 s
    fireEvent.change(screen.getByLabelText(/Comes in at/), { target: { value: "2000" } });
    view.rerender(<Harness playing relMs={1000} />);
    expect(window.HTMLMediaElement.prototype.play).not.toHaveBeenCalled();
  });
});
