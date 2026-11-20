// @vitest-environment jsdom
import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { useState } from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { api } from "./api.js";
import { LevelMeter } from "./LevelMeter.jsx";
import { MixTimeline } from "./MixTimeline.jsx";
import { QUIET, toggleMute, toggleSolo } from "./mixModel.js";
import { newMusic } from "./musicModel.js";

beforeEach(() => {
  vi.spyOn(api, "musicPeaks").mockResolvedValue({ duration_ms: 100000, peaks: [0.5, 0.5] });
  vi.spyOn(api, "clipWaveform").mockResolvedValue({ has_audio: true, peaks: [0.5, 0.5] });
});
afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});

function Harness({ seen }) {
  const [mix, setMix] = useState(QUIET);
  const [clipVolume, setClipVolume] = useState(1);
  const [music, setMusic] = useState({ ...newMusic({ id: "s1", title: "S", duration_s: 100 }), at_ms: 0 });
  seen?.({ mix, clipVolume, music });
  return (
    <MixTimeline
      ratingKey="k1"
      start={0}
      end={4000}
      relMs={0}
      onSeek={() => {}}
      audio
      clipCurve={[]}
      onClipCurve={() => {}}
      music={music}
      onMusicCurve={() => {}}
      clipTrack={{
        muted: mix.clipMuted,
        solo: mix.solo === "clip",
        level: clipVolume,
        onMute: () => setMix((m) => toggleMute(m, "clip")),
        onSolo: () => setMix((m) => toggleSolo(m, "clip")),
        onLevel: setClipVolume,
        getEl: () => null,
        active: false,
      }}
      songTrack={{
        muted: mix.songMuted,
        solo: mix.solo === "song",
        level: music.volume,
        onMute: () => setMix((m) => toggleMute(m, "song")),
        onSolo: () => setMix((m) => toggleSolo(m, "song")),
        onLevel: (v) => setMusic((m) => ({ ...m, volume: v })),
        getEl: () => null,
        active: false,
      }}
    />
  );
}

describe("the mixing controls in the mix", () => {
  it("has mute, solo, a level and a meter for each sound", async () => {
    render(<Harness />);
    for (const name of ["Clip sound", "Music"]) {
      expect(screen.getByLabelText(`Mute ${name}`)).toBeTruthy();
      expect(screen.getByLabelText(`Solo ${name}`)).toBeTruthy();
      expect(screen.getByLabelText(`${name} level`)).toBeTruthy();
      expect(screen.getByRole("meter", { name: `${name} meter` })).toBeTruthy();
    }
    await waitFor(() => expect(api.clipWaveform).toHaveBeenCalled());
  });

  it("mutes and solos, showing which are on", () => {
    let seen;
    render(<Harness seen={(s) => (seen = s)} />);
    fireEvent.click(screen.getByLabelText("Mute Music"));
    expect(seen.mix.songMuted).toBe(true);
    expect(screen.getByLabelText("Mute Music").getAttribute("aria-pressed")).toBe("true");
    expect(screen.getByLabelText("Mute Clip sound").getAttribute("aria-pressed")).toBe("false");
    fireEvent.click(screen.getByLabelText("Solo Clip sound"));
    expect(seen.mix.solo).toBe("clip");
    fireEvent.click(screen.getByLabelText("Solo Music"));
    expect(seen.mix.solo).toBe("song");
    expect(screen.getByLabelText("Solo Clip sound").getAttribute("aria-pressed")).toBe("false");
    fireEvent.click(screen.getByLabelText("Solo Music"));
    expect(seen.mix.solo).toBe(null);
  });

  it("sets each sound's level from its fader", () => {
    let seen;
    render(<Harness seen={(s) => (seen = s)} />);
    fireEvent.change(screen.getByLabelText("Music level"), { target: { value: "0.4" } });
    expect(seen.music.volume).toBeCloseTo(0.4, 2);
    fireEvent.change(screen.getByLabelText("Clip sound level"), { target: { value: "1.5" } });
    expect(seen.clipVolume).toBeCloseTo(1.5, 2);
  });

  it("draws no controls without a track", () => {
    render(<MixTimeline ratingKey="k" start={0} end={4000} relMs={0} audio clipCurve={[]} onClipCurve={() => {}} />);
    expect(screen.queryByLabelText("Mute Clip sound")).toBeNull();
  });
});

describe("the level meter", () => {
  // The page keeps one audio graph for good, so the fake is one class whose signal the tests change.
  const signal = { samples: new Float32Array(1024) };
  const wired = [];
  class Ctx {
    constructor() {
      this.destination = {};
      this.resume = vi.fn(() => Promise.resolve());
    }
    createMediaElementSource() {
      wired.push("source");
      return { connect: vi.fn() };
    }
    createGain() {
      return { gain: { value: 1 }, connect: vi.fn() };
    }
    createStereoPanner() {
      return { pan: { value: 0 }, connect: vi.fn() };
    }
    createAnalyser() {
      return {
        fftSize: 0,
        connect: vi.fn(),
        getFloatTimeDomainData: (buf) => buf.set(signal.samples.slice(0, buf.length)),
      };
    }
  }

  // (first: before any audio graph exists, so it can find none)
  it("stays at rest, and breaks nothing, where the browser has no Web Audio", () => {
    vi.stubGlobal("AudioContext", undefined);
    vi.stubGlobal("webkitAudioContext", undefined);
    const el = document.createElement("audio");
    render(<LevelMeter getEl={() => el} active label="Music" />);
    expect(screen.getByRole("meter", { name: "Music meter" }).getAttribute("aria-valuetext")).toBe("-∞ dB");
  });

  it("reads the level of the element while active and shows it", async () => {
    vi.stubGlobal("AudioContext", Ctx);
    signal.samples = new Float32Array(1024).fill(0.5);
    const el = document.createElement("audio");
    const before = wired.length;
    render(<LevelMeter getEl={() => el} active label="Music" />);
    const meter = screen.getByRole("meter", { name: "Music meter" });
    await waitFor(() => expect(Number(meter.getAttribute("aria-valuenow"))).toBe(-6));
    expect(meter.getAttribute("aria-valuetext")).toBe("-6 dB");
    expect(wired.length - before).toBe(1); // wired once, however many frames
  });

  it("lights the clip warning on a full-scale signal and rests when not active", async () => {
    vi.stubGlobal("AudioContext", Ctx);
    signal.samples = new Float32Array(1024).fill(1);
    const el = document.createElement("audio");
    const { rerender } = render(<LevelMeter getEl={() => el} active label="Clip sound" />);
    const meter = screen.getByRole("meter", { name: "Clip sound meter" });
    await waitFor(() => expect(meter.className).toContain("clip"));
    await act(async () => rerender(<LevelMeter getEl={() => el} active={false} label="Clip sound" />));
    expect(meter.className).not.toContain("clip");
    expect(meter.getAttribute("aria-valuetext")).toBe("-∞ dB");
  });
});
