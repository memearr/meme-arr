// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { useState } from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { api } from "./api.js";
import { MixTimeline, songBars } from "./MixTimeline.jsx";
import { newMusic, previewState } from "./musicModel.js";

beforeEach(() => {
  vi.spyOn(api, "musicPeaks").mockResolvedValue({ duration_ms: 100000, peaks: Array.from({ length: 100 }, (_, i) => (i % 5) / 5) });
  vi.spyOn(api, "clipWaveform").mockResolvedValue({ has_audio: true, peaks: [0.2, 0.9, 0.5, 0.1] });
});
afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
});

const rect = { left: 0, top: 0, width: 1000, height: 50, right: 1000, bottom: 50, x: 0, y: 0 };

function Harness({ audio = true, withMusic = true, seen }) {
  const [clipCurve, setClipCurve] = useState([]);
  const [music, setMusic] = useState(withMusic ? { ...newMusic({ id: "s1", title: "S", duration_s: 100 }), at_ms: 0 } : null);
  seen?.({ clipCurve, music });
  return (
    <MixTimeline
      ratingKey="k1"
      start={10000}
      end={14000}
      relMs={1000}
      onSeek={() => {}}
      audio={audio}
      clipCurve={clipCurve}
      onClipCurve={setClipCurve}
      music={music}
      onMusicCurve={(curve) => setMusic((m) => ({ ...m, curve }))}
    />
  );
}

describe("the mix timeline", () => {
  it("shows the picture, the clip's sound and the music over the clip's length", async () => {
    render(<Harness />);
    expect(screen.getByLabelText("Tracks")).toBeTruthy();
    expect(screen.getByLabelText("Music volume over the clip")).toBeTruthy();
    expect(screen.getByLabelText("Clip sound volume over the clip")).toBeTruthy();
    await waitFor(() => expect(api.clipWaveform).toHaveBeenCalledWith("k1", 10000, 14000));
    await waitFor(() => expect(api.musicPeaks).toHaveBeenCalledWith("s1"));
  });

  it("has no clip-sound lane when the clip's sound is off, and no music lane without a song", () => {
    render(<Harness audio={false} withMusic={false} />);
    expect(screen.queryByLabelText("Clip sound volume over the clip")).toBeNull();
    expect(screen.queryByLabelText("Music volume over the clip")).toBeNull();
  });

  it("puts a point where the lane is clicked, with the height as the volume", () => {
    let seen;
    render(<Harness seen={(s) => (seen = s)} />);
    const lane = screen.getByLabelText("Music volume over the clip");
    lane.getBoundingClientRect = () => rect;
    fireEvent.pointerDown(lane, { clientX: 500, clientY: 25, button: 0, buttons: 1 });
    expect(seen.music.curve).toHaveLength(1);
    expect(seen.music.curve[0].t_ms).toBe(2000);
    expect(seen.music.curve[0].gain).toBeCloseTo(0.5, 2);
    expect(screen.getByLabelText("Music volume at 2 s: 50%")).toBeTruthy();
  });

  it("drags a point, and removes it by the Delete key or a double-click", () => {
    let seen;
    render(<Harness seen={(s) => (seen = s)} />);
    const lane = screen.getByLabelText("Music volume over the clip");
    lane.getBoundingClientRect = () => rect;
    fireEvent.pointerDown(lane, { clientX: 250, clientY: 0, button: 0, buttons: 1 });
    fireEvent.pointerMove(lane, { clientX: 750, clientY: 50, buttons: 1 });
    expect(seen.music.curve).toHaveLength(1);
    expect(seen.music.curve[0].t_ms).toBe(3000);
    expect(seen.music.curve[0].gain).toBe(0);
    fireEvent.pointerUp(lane);
    fireEvent.keyDown(screen.getByLabelText(/Music volume at 3 s/), { key: "Delete" });
    expect(seen.music.curve).toHaveLength(0);
    fireEvent.pointerDown(lane, { clientX: 500, clientY: 10, button: 0, buttons: 1 });
    fireEvent.pointerUp(lane);
    fireEvent.doubleClick(screen.getByLabelText(/Music volume at 2 s/));
    expect(seen.music.curve).toHaveLength(0);
  });

  it("nudges a focused point with the arrow keys", () => {
    let seen;
    render(<Harness seen={(s) => (seen = s)} />);
    const lane = screen.getByLabelText("Clip sound volume over the clip");
    lane.getBoundingClientRect = () => rect;
    fireEvent.pointerDown(lane, { clientX: 500, clientY: 0, button: 0, buttons: 1 });
    fireEvent.pointerUp(lane);
    fireEvent.keyDown(screen.getByLabelText(/Clip sound volume at 2 s/), { key: "ArrowDown" });
    expect(seen.clipCurve[0].gain).toBeCloseTo(0.95, 2);
    fireEvent.keyDown(screen.getByLabelText(/Clip sound volume at 2 s/), { key: "ArrowRight", shiftKey: true });
    expect(seen.clipCurve[0].t_ms).toBe(3000);
  });

  it("resets a line", () => {
    let seen;
    render(<Harness seen={(s) => (seen = s)} />);
    const lane = screen.getByLabelText("Music volume over the clip");
    lane.getBoundingClientRect = () => rect;
    fireEvent.pointerDown(lane, { clientX: 500, clientY: 25, button: 0, buttons: 1 });
    fireEvent.pointerUp(lane);
    fireEvent.click(screen.getByLabelText("Reset the Music volume line"));
    expect(seen.music.curve).toEqual([]);
  });

  it("says when the video has no sound", async () => {
    api.clipWaveform.mockResolvedValue({ has_audio: false, peaks: [] });
    render(<Harness withMusic={false} />);
    expect(await screen.findByText("This video has no sound.")).toBeTruthy();
  });
});

describe("the part of a song that plays", () => {
  it("is the bars from where it starts for as long as it plays", () => {
    const wave = { duration_ms: 100000, peaks: Array.from({ length: 100 }, (_, i) => i / 100) };
    const part = songBars(wave, 10000, 20000);
    expect(part[0]).toBeCloseTo(0.1, 5);
    expect(part).toHaveLength(20);
    expect(songBars(null, 0, 1000)).toEqual([]);
  });
});

describe("the preview of a volume line", () => {
  it("lowers the song in the player by what the line says", () => {
    const m = { ...newMusic({ id: "s1", duration_s: 100 }), curve: [{ t_ms: 0, gain: 1 }, { t_ms: 4000, gain: 0 }] };
    expect(previewState(m, 0, 4000).gain).toBeCloseTo(1, 5);
    expect(previewState(m, 2000, 4000).gain).toBeCloseTo(0.5, 5);
    expect(previewState(m, 4000, 4000).gain).toBeCloseTo(0, 5);
  });
});

describe("the tracks", () => {
  const base = { ratingKey: "k1", start: 10000, end: 14000, relMs: 1000, onSeek: () => {} };

  it("is there without a song or the clip's sound, with a way to add each", () => {
    const onAudio = vi.fn();
    const onAddSong = vi.fn();
    render(
      <MixTimeline {...base} audio={false} clipCurve={[]} onClipCurve={() => {}} music={null} onAudio={onAudio} onAddSong={onAddSong} />
    );
    expect(screen.getByLabelText("Tracks")).toBeTruthy();
    fireEvent.click(screen.getByText("Include clip sound"));
    fireEvent.click(screen.getByText("Add a song"));
    expect(onAudio).toHaveBeenCalledTimes(1);
    expect(onAddSong).toHaveBeenCalledTimes(1);
  });

  it("shows the caption lines and subtitle cues as bars", () => {
    render(
      <MixTimeline
        {...base}
        audio={false}
        clipCurve={[]}
        onClipCurve={() => {}}
        music={null}
        captions={{ lines: ["Top: hi"], cues: [{ start_ms: 500, end_ms: 1500, text: "a line" }] }}
      />
    );
    const lane = screen.getByLabelText("Captions over the clip");
    expect(lane.textContent).toContain("Top: hi");
    expect(lane.textContent).toContain("a line");
  });

  it("says when there are no captions or layers, and offers to add a layer", () => {
    const onAddLayer = vi.fn();
    render(
      <MixTimeline
        {...base}
        audio={false}
        clipCurve={[]}
        onClipCurve={() => {}}
        music={null}
        captions={{ lines: [], cues: [] }}
        layers={[]}
        onAddLayer={onAddLayer}
      />
    );
    expect(screen.getByText("No captions on this clip.")).toBeTruthy();
    fireEvent.click(screen.getByText("Add a layer"));
    expect(onAddLayer).toHaveBeenCalled();
  });

  it("lays each layer on its own lane and selects it when clicked", () => {
    const onSelectLayer = vi.fn();
    render(
      <MixTimeline
        {...base}
        audio={false}
        clipCurve={[]}
        onClipCurve={() => {}}
        music={null}
        layers={[
          { id: "a", label: "Arrow", start_ms: 0, end_ms: 1000, selected: false },
          { id: "b", label: "Star", start_ms: 1000, end_ms: null, selected: true },
        ]}
        onSelectLayer={onSelectLayer}
      />
    );
    fireEvent.click(screen.getByText("Arrow"));
    expect(onSelectLayer).toHaveBeenCalledWith("a");
    expect(screen.getByText("Star").closest("button").getAttribute("aria-pressed")).toBe("true");
  });
});
