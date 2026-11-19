// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { useState } from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { api } from "./api.js";
import { MusicPanel } from "./MusicPanel.jsx";
import { musicForRequest } from "./musicModel.js";

const CLIP = 5000;
const tracks = [
  { id: "s1", title: "A Song", artist: "Band", album: "LP", duration_s: 200, cover_art: "al-1" },
  { id: "s2", title: "Other", artist: "", album: "", duration_s: 0 },
];

beforeEach(() => {
  window.localStorage.clear();
  // jsdom has no media playback
  vi.spyOn(window.HTMLMediaElement.prototype, "play").mockImplementation(() => Promise.resolve());
  vi.spyOn(window.HTMLMediaElement.prototype, "pause").mockImplementation(() => {});
  vi.spyOn(api, "musicPeaks").mockResolvedValue({ duration_ms: 0, peaks: [] });
});
afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
});

function Harness({ audio = false, playing = false, relMs = 0, onSeen, preview = true, what = "clip" }) {
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
      preview={preview}
      what={what}
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
      duck: 0,
    });
    expect(screen.getByText("50%")).toBeTruthy();
  });

  it("ducks the song under the clip's sound only when that sound is kept", async () => {
    vi.spyOn(api, "getNavidromeSettings").mockResolvedValue({ configured: true });
    vi.spyOn(api, "musicSearch").mockResolvedValue({ tracks });
    let seen;
    const { rerender } = render(<Harness audio={false} onSeen={(s) => (seen = s)} />);
    fireEvent.change(await screen.findByLabelText("Search your music"), { target: { value: "x" } });
    fireEvent.click(screen.getByText("Search"));
    fireEvent.click(await screen.findByText("A Song"));
    const slider = () => screen.getByLabelText(/Duck under the clip's sound/);
    expect(slider().disabled).toBe(true);
    rerender(<Harness audio onSeen={(s) => (seen = s)} />);
    expect(slider().disabled).toBe(false);
    fireEvent.change(slider(), { target: { value: "0.6" } });
    expect(musicForRequest(seen.music, CLIP, true).duck).toBe(0.6);
    expect(musicForRequest(seen.music, CLIP, false).duck).toBe(0);
  });

  it("calls it the clips' sound in a sequence", async () => {
    vi.spyOn(api, "getNavidromeSettings").mockResolvedValue({ configured: true });
    vi.spyOn(api, "musicSearch").mockResolvedValue({ tracks });
    render(<Harness what="sequence" preview={false} />);
    fireEvent.change(await screen.findByLabelText("Search your music"), { target: { value: "x" } });
    fireEvent.click(screen.getByText("Search"));
    fireEvent.click(await screen.findByText("A Song"));
    expect(screen.getByLabelText(/Duck under the clips' sound/)).toBeTruthy();
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

  it("draws the song's waveform and a pick on it sets where the song starts", async () => {
    vi.spyOn(api, "getNavidromeSettings").mockResolvedValue({ configured: true });
    vi.spyOn(api, "musicSearch").mockResolvedValue({ tracks });
    const peaks = vi.spyOn(api, "musicPeaks").mockResolvedValue({
      duration_ms: 200_000,
      peaks: [0.2, 0.9, 0.4, 1, 0.3, 0.6, 0.5, 0.8, 0.1, 0.7],
    });
    let seen;
    render(<Harness onSeen={(s) => (seen = s)} />);
    fireEvent.change(await screen.findByLabelText("Search your music"), { target: { value: "x" } });
    fireEvent.click(screen.getByText("Search"));
    fireEvent.click(await screen.findByText("A Song"));
    const el = await screen.findByRole("slider", { name: "Where in the song to start" });
    expect(peaks).toHaveBeenCalledWith("s1");
    el.getBoundingClientRect = () => ({ left: 0, width: 1000, top: 0, height: 50 });
    fireEvent.pointerDown(el, { clientX: 500, pointerId: 1 }); // the middle of a 200 s song, 5 s plays
    expect(seen.music.start_ms).toBe(97_500);
  });

  it("falls back to the slider when the waveform can't be made", async () => {
    vi.spyOn(api, "getNavidromeSettings").mockResolvedValue({ configured: true });
    vi.spyOn(api, "musicSearch").mockResolvedValue({ tracks });
    vi.spyOn(api, "musicPeaks").mockRejectedValue(new Error("couldn't read that song"));
    render(<Harness />);
    fireEvent.change(await screen.findByLabelText("Search your music"), { target: { value: "x" } });
    fireEvent.click(screen.getByText("Search"));
    fireEvent.click(await screen.findByText("A Song"));
    expect(await screen.findByText(/Couldn't draw the waveform/)).toBeTruthy();
    expect(screen.queryByRole("slider", { name: "Where in the song to start" })).toBeNull();
    expect(screen.getByLabelText(/Start the song at/)).toBeTruthy();
  });

  it("shows each song's cover, and the chosen song's", async () => {
    vi.spyOn(api, "getNavidromeSettings").mockResolvedValue({ configured: true });
    vi.spyOn(api, "musicSearch").mockResolvedValue({ tracks });
    render(<Harness />);
    fireEvent.change(await screen.findByLabelText("Search your music"), { target: { value: "x" } });
    fireEvent.click(screen.getByText("Search"));
    await screen.findByText("A Song");
    const art = document.querySelectorAll(".music-results img.song-art");
    expect(art).toHaveLength(1); // the second song has no cover
    expect(art[0].getAttribute("src")).toMatch(/api\/music\/cover\/al-1\?size=64$/);
    expect(document.querySelectorAll(".music-results .song-art.none")).toHaveLength(1);
    fireEvent.click(screen.getByText("A Song"));
    expect(document.querySelector(".music-title img.song-art").getAttribute("src")).toMatch(/cover\/al-1\?size=96$/);
  });

  it("offers the songs picked before, newest first, and picking one again brings it to the front", async () => {
    vi.spyOn(api, "getNavidromeSettings").mockResolvedValue({ configured: true });
    vi.spyOn(api, "musicSearch").mockResolvedValue({ tracks });
    const first = render(<Harness />);
    fireEvent.change(await screen.findByLabelText("Search your music"), { target: { value: "x" } });
    fireEvent.click(screen.getByText("Search"));
    fireEvent.click(await screen.findByText("A Song"));
    first.unmount();
    // a new visit: nothing searched yet, the pick is waiting
    render(<Harness />);
    const list = await screen.findByRole("list", { name: "Recent songs" });
    expect(list.textContent).toContain("A Song");
    expect(screen.getByText("Recent songs")).toBeTruthy();
    fireEvent.click(screen.getByText("A Song"));
    expect(screen.getByText("Remove")).toBeTruthy(); // chosen straight from the list
  });

  it("forgets the recent songs when asked", async () => {
    window.localStorage.setItem("memearr.recentSongs", JSON.stringify([tracks[0]]));
    vi.spyOn(api, "getNavidromeSettings").mockResolvedValue({ configured: true });
    render(<Harness />);
    await screen.findByRole("list", { name: "Recent songs" });
    fireEvent.click(screen.getByText("Clear"));
    expect(screen.queryByRole("list", { name: "Recent songs" })).toBeNull();
    expect(window.localStorage.getItem("memearr.recentSongs")).toBe("[]");
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

describe("the Music box on a sequence", () => {
  it("talks about the sequence and its clips, and does not play the song", async () => {
    vi.spyOn(api, "getNavidromeSettings").mockResolvedValue({ configured: true });
    vi.spyOn(api, "musicSearch").mockResolvedValue({ tracks });
    render(<Harness preview={false} what="sequence" />);
    fireEvent.change(await screen.findByLabelText("Search your music"), { target: { value: "x" } });
    fireEvent.click(screen.getByText("Search"));
    fireEvent.click(await screen.findByText("A Song"));
    expect(screen.getByLabelText(/Clips' sound volume/)).toBeTruthy();
    expect(screen.getByText(/stops where the sequence does/)).toBeTruthy();
    expect(screen.getByText(/not played here/)).toBeTruthy();
    expect(screen.queryByText(/Hear it in the player/)).toBeNull();
    expect(document.querySelector("audio")).toBeNull();
  });
});
