// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { useState } from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { api } from "./api.js";
import { LevelMeter } from "./LevelMeter.jsx";
import { MixTimeline } from "./MixTimeline.jsx";
import { MusicPanel } from "./MusicPanel.jsx";
import { QUIET } from "./mixModel.js";
import { adjustMusic, musicForRequest, newMusic } from "./musicModel.js";
import { mixMeter, setPan, tap } from "./audioGraph.js";
import { panText, previewMaster } from "./mixModel.js";

beforeEach(() => {
  vi.spyOn(api, "musicPeaks").mockResolvedValue({ duration_ms: 100000, peaks: [0.5, 0.5] });
  vi.spyOn(api, "clipWaveform").mockResolvedValue({ has_audio: true, peaks: [0.5, 0.5] });
  vi.spyOn(api, "musicSources").mockResolvedValue({ navidrome: true, library: false, uploads: 0 });
  vi.spyOn(window.HTMLMediaElement.prototype, "play").mockImplementation(() => Promise.resolve());
  vi.spyOn(window.HTMLMediaElement.prototype, "pause").mockImplementation(() => {});
});
afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});

describe("pan and master in the numbers", () => {
  it("says where a pan is, in words", () => {
    expect(panText(0)).toBe("Centre");
    expect(panText(-0.3)).toBe("Left 30%");
    expect(panText(1)).toBe("Right 100%");
    expect(panText(9)).toBe("Right 100%");
    expect(panText(undefined)).toBe("Centre");
  });

  it("holds the player's master at full volume (the export goes to 200%)", () => {
    expect(previewMaster(0.4)).toBe(0.4);
    expect(previewMaster(1.8)).toBe(1);
    expect(previewMaster(-1)).toBe(0);
    expect(previewMaster(undefined)).toBe(1);
  });

  it("keeps the song's pan inside the speakers and sends it", () => {
    const m = adjustMusic({ ...newMusic({ id: "s1", duration_s: 100 }), pan: 0 }, { pan: 3 }, 5000);
    expect(m.pan).toBe(1);
    expect(adjustMusic(m, { pan: -0.123 }, 5000).pan).toBe(-0.12);
    expect(adjustMusic(m, { pan: NaN }, 5000).pan).toBe(0);
    expect(musicForRequest({ ...m, pan: -0.5 }, 5000).pan).toBe(-0.5);
    expect(newMusic({ id: "s1" }).pan).toBe(0);
  });
});

function Harness({ seen }) {
  const [clipPan, setClipPan] = useState(0);
  const [songPan, setSongPan] = useState(0.5);
  const [master, setMaster] = useState(1);
  seen?.({ clipPan, songPan, master });
  const track = (pan, onPan) => ({
    muted: false,
    solo: false,
    level: 1,
    pan,
    onMute: () => {},
    onSolo: () => {},
    onLevel: () => {},
    onPan,
    getEl: () => null,
    active: false,
  });
  return (
    <MixTimeline
      ratingKey="k1"
      start={0}
      end={4000}
      relMs={0}
      audio
      clipCurve={[]}
      onClipCurve={() => {}}
      music={{ ...newMusic({ id: "s1", title: "S", duration_s: 100 }), at_ms: 0 }}
      onMusicCurve={() => {}}
      clipTrack={track(clipPan, setClipPan)}
      songTrack={track(songPan, setSongPan)}
      master={{ level: master, onLevel: setMaster, active: false }}
    />
  );
}

describe("the pan and master controls in the Mix", () => {
  it("pans each sound from its own slider, and double-click puts it back in the middle", () => {
    let seen;
    render(<Harness seen={(s) => (seen = s)} />);
    fireEvent.change(screen.getByLabelText("Clip sound pan"), { target: { value: "-0.6" } });
    expect(seen.clipPan).toBeCloseTo(-0.6, 2);
    expect(screen.getByLabelText("Clip sound pan").getAttribute("aria-valuetext")).toBe("Left 60%");
    expect(screen.getByLabelText("Music pan").getAttribute("aria-valuetext")).toBe("Right 50%");
    fireEvent.doubleClick(screen.getByLabelText("Music pan"));
    expect(seen.songPan).toBe(0);
  });

  it("has one master fader and a master meter, and double-click puts the fader at 100%", () => {
    let seen;
    render(<Harness seen={(s) => (seen = s)} />);
    expect(screen.getByRole("meter", { name: "Master meter" })).toBeTruthy();
    fireEvent.change(screen.getByLabelText("Master level"), { target: { value: "0.7" } });
    expect(seen.master).toBeCloseTo(0.7, 2);
    fireEvent.doubleClick(screen.getByLabelText("Master level"));
    expect(seen.master).toBe(1);
  });

  it("has no master without one asked for", () => {
    render(<MixTimeline ratingKey="k" start={0} end={4000} relMs={0} audio clipCurve={[]} onClipCurve={() => {}} />);
    expect(screen.queryByLabelText("Master level")).toBeNull();
  });
});

describe("the audio graph", () => {
  const signal = { samples: new Float32Array(1024) };
  const panners = [];
  class Ctx {
    constructor() {
      this.destination = {};
      this.resume = vi.fn(() => Promise.resolve());
    }
    createMediaElementSource() {
      return { connect: vi.fn() };
    }
    createGain() {
      return { gain: { value: 1 }, connect: vi.fn() };
    }
    createStereoPanner() {
      const p = { pan: { value: 0 }, connect: vi.fn() };
      panners.push(p);
      return p;
    }
    createAnalyser() {
      return { fftSize: 0, connect: vi.fn(), getFloatTimeDomainData: (b) => b.set(signal.samples.slice(0, b.length)) };
    }
  }

  it("keeps a pan for an element that isn't wired yet, and moves it once it is", () => {
    vi.stubGlobal("AudioContext", Ctx);
    const el = document.createElement("audio");
    setPan(el, -0.4);
    const before = panners.length;
    expect(tap(el).panner).toBe(panners[before]);
    expect(panners[before].pan.value).toBeCloseTo(-0.4, 6);
    setPan(el, 2);
    expect(panners[before].pan.value).toBe(1);
    setPan(el, NaN);
    expect(panners[before].pan.value).toBe(0);
    setPan(null, 1); // nothing to pan: no harm
  });

  it("meters the whole mix once something is wired in", async () => {
    vi.stubGlobal("AudioContext", Ctx);
    const el = document.createElement("audio");
    tap(el);
    expect(mixMeter()).toBeTruthy();
    signal.samples = new Float32Array(1024).fill(0.25);
    render(<LevelMeter master active label="Master" />);
    const meter = screen.getByRole("meter", { name: "Master meter" });
    await waitFor(() => expect(Number(meter.getAttribute("aria-valuenow"))).toBe(-12));
  });

  it("works without a panner (old browsers): the sound still goes through", () => {
    class NoPan extends Ctx {
      createStereoPanner = undefined;
    }
    vi.stubGlobal("AudioContext", NoPan);
    // (the context is the page's one, made already by an earlier test, so this checks setPan only)
    const el = document.createElement("audio");
    expect(() => setPan(el, 0.5)).not.toThrow();
  });
});

describe("one place for the levels", () => {
  function PanelHarness({ levelsInMix }) {
    const [music, setMusic] = useState({ ...newMusic({ id: "s1", title: "A Song", duration_s: 200 }) });
    return (
      <MusicPanel
        music={music}
        onChoose={setMusic}
        onClear={() => setMusic(null)}
        onChange={setMusic}
        clipVolume={1}
        setClipVolume={() => {}}
        audio
        clipMs={5000}
        relMs={0}
        playing={false}
        levelsInMix={levelsInMix}
      />
    );
  }

  it("keeps the volume sliders and the player checkbox where the Mix isn't", async () => {
    render(<PanelHarness levelsInMix={false} />);
    expect(await screen.findByLabelText(/Music volume/)).toBeTruthy();
    expect(screen.getByLabelText(/Clip sound volume/)).toBeTruthy();
    expect(screen.getByLabelText(/Hear it in the player/)).toBeTruthy();
  });

  it("drops them when the Mix has the levels, and says where they went", async () => {
    render(<PanelHarness levelsInMix />);
    await screen.findByText(/level, pan, mute and solo/);
    expect(screen.queryByLabelText(/Music volume/)).toBeNull();
    expect(screen.queryByLabelText(/Clip sound volume/)).toBeNull();
    expect(screen.queryByLabelText(/Hear it in the player/)).toBeNull();
    // the song is still in the page for the player
    expect(document.querySelector("audio")).toBeTruthy();
    expect(QUIET.solo).toBe(null);
  });
});
