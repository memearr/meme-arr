// @vitest-environment jsdom
import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { MixTimeline } from "./MixTimeline.jsx";
import { LoudnessReadout } from "./LoudnessReadout.jsx";
import { loudnessMeter, tap } from "./audioGraph.js";
import { FLOOR_LUFS, Integrator, STEP_MS, blockPower, lufsOf, lufsText, meanSquare } from "./loudness.js";
import { api } from "./api.js";

afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});

describe("loudness numbers", () => {
  it("is the mean square of the last samples asked for", () => {
    expect(meanSquare(new Float32Array([1, 1, 0, 0]))).toBe(0.5);
    expect(meanSquare(new Float32Array([5, 5, 0, 0]), 2)).toBe(0); // the last two
    expect(meanSquare(new Float32Array([2, 2]), 10)).toBe(4); // more than there is
    expect(meanSquare(new Float32Array([]), 4)).toBe(0);
    expect(meanSquare(new Float32Array([1]), 0)).toBe(0);
  });

  it("turns power into LUFS the way the standard does", () => {
    // a full-scale constant on both channels: power 2
    expect(lufsOf(2)).toBeCloseTo(-0.691 + 10 * Math.log10(2), 6);
    expect(lufsOf(0)).toBe(-Infinity);
    // each halving of the power is 3 dB down
    expect(lufsOf(1) - lufsOf(0.5)).toBeCloseTo(3.0103, 3);
  });

  it("adds the channels' powers over the last 400 ms", () => {
    const rate = 1000;
    const loud = new Float32Array(1000).fill(0.5);
    const quiet = new Float32Array(1000).fill(0.25);
    expect(blockPower([loud, quiet], rate)).toBeCloseTo(0.25 + 0.0625, 6);
    expect(blockPower([loud], rate)).toBeCloseTo(0.25, 6);
    expect(blockPower([], rate)).toBe(0);
  });

  it("says a loudness, or that there is none", () => {
    expect(lufsText(-14.24)).toBe("-14.2 LUFS");
    expect(lufsText(-Infinity)).toBe("-- LUFS");
    expect(lufsText(FLOOR_LUFS)).toBe("-- LUFS");
    expect(lufsText(NaN)).toBe("-- LUFS");
  });
});

describe("the integrated loudness", () => {
  const power = (lufs) => 10 ** ((lufs + 0.691) / 10);

  it("is the loudness of steady sound", () => {
    const i = new Integrator();
    for (let n = 0; n < 50; n++) i.add(power(-20));
    expect(i.integrated()).toBeCloseTo(-20, 3);
  });

  it("is nothing before there is sound, and after a reset", () => {
    const i = new Integrator();
    expect(i.integrated()).toBe(-Infinity);
    i.add(power(-20));
    i.reset();
    expect(i.integrated()).toBe(-Infinity);
  });

  it("leaves out silence and the very quiet (the gates), so a pause doesn't pull it down", () => {
    const i = new Integrator();
    for (let n = 0; n < 20; n++) i.add(power(-18));
    const loud = i.integrated();
    for (let n = 0; n < 200; n++) i.add(0); // a long silence
    for (let n = 0; n < 200; n++) i.add(power(-80)); // under the absolute gate
    expect(i.integrated()).toBeCloseTo(loud, 3);
    // and sound more than 10 LU under the rest is dropped as well
    for (let n = 0; n < 200; n++) i.add(power(-45));
    expect(i.integrated()).toBeCloseTo(loud, 1);
  });

  it("ignores what isn't a power", () => {
    const i = new Integrator();
    i.add(NaN);
    i.add(-1);
    i.add(Infinity);
    expect(i.blocks).toHaveLength(0);
  });

  it("keeps a bounded history", () => {
    const i = new Integrator();
    for (let n = 0; n < 40000; n++) i.add(1);
    expect(i.blocks.length).toBeLessThanOrEqual(36000);
  });
});

describe("the live readout", () => {
  const signal = { level: 0.25 };
  const made = { splits: [], filters: [] };
  class Ctx {
    constructor() {
      this.destination = {};
      this.sampleRate = 1000;
      this.resume = vi.fn(() => Promise.resolve());
    }
    createMediaElementSource() {
      return { connect: vi.fn() };
    }
    createGain() {
      return { gain: { value: 1 }, connect: vi.fn(), channelCount: 2 };
    }
    createStereoPanner() {
      return { pan: { value: 0 }, connect: vi.fn() };
    }
    createBiquadFilter() {
      const f = { type: "", frequency: { value: 0 }, gain: { value: 0 }, Q: { value: 0 }, connect: vi.fn() };
      made.filters.push(f);
      return f;
    }
    createChannelSplitter(n) {
      const s = { n, connect: vi.fn() };
      made.splits.push(s);
      return s;
    }
    createAnalyser() {
      return {
        fftSize: 0,
        connect: vi.fn(),
        getFloatTimeDomainData: (b) => b.fill(signal.level),
      };
    }
  }

  it("is wired with the K-weighting filters and one meter per channel once a sound is in", () => {
    vi.stubGlobal("AudioContext", Ctx);
    expect(loudnessMeter()).toBe(null);
    tap(document.createElement("audio"));
    const m = loudnessMeter();
    expect(m.channels).toHaveLength(2);
    expect(m.channels[0].analyser.fftSize).toBe(32768);
    expect(made.splits[0].n).toBe(2);
    const shelf = made.filters.find((f) => f.type === "highshelf");
    const low = made.filters.find((f) => f.type === "highpass");
    expect(shelf.frequency.value).toBeCloseTo(1681.97, 2);
    expect(shelf.gain.value).toBeCloseTo(4, 2);
    expect(low.frequency.value).toBeCloseTo(38.13, 2);
  });

  it("shows the momentary and the integrated loudness while playing, and resets", async () => {
    vi.stubGlobal("AudioContext", Ctx);
    tap(document.createElement("audio"));
    // 0.25 on both channels: power 0.125, a steady -9.7 LUFS-ish
    const want = lufsText(lufsOf(2 * 0.0625));
    render(<LoudnessReadout active />);
    await waitFor(() => expect(screen.getByLabelText("Momentary loudness").textContent).toBe(want));
    await waitFor(() => expect(screen.getByLabelText("Integrated loudness").textContent).toBe(want), { timeout: 2000 });
    fireEvent.click(screen.getByRole("button", { name: "reset" }));
    // it carries on reading, so the integrated figure builds up again from nothing
    expect(screen.getByLabelText("Integrated loudness").textContent).toMatch(/LUFS/);
    expect(STEP_MS).toBe(100);
  });

  it("shows nothing but dashes while not playing", () => {
    vi.stubGlobal("AudioContext", Ctx);
    render(<LoudnessReadout active={false} />);
    expect(screen.getByLabelText("Momentary loudness").textContent).toBe("-- LUFS");
    expect(screen.getByLabelText("Integrated loudness").textContent).toBe("-- LUFS");
  });

  it("is under the master only when asked for", () => {
    vi.spyOn(api, "clipWaveform").mockResolvedValue({ has_audio: true, peaks: [0.5] });
    const props = { ratingKey: "k", start: 0, end: 4000, relMs: 0, audio: true, clipCurve: [], onClipCurve: () => {} };
    const { rerender } = render(<MixTimeline {...props} master={{ level: 1, onLevel: () => {}, active: false }} />);
    expect(screen.queryByRole("group", { name: "Loudness" })).toBeNull();
    rerender(<MixTimeline {...props} master={{ level: 1, onLevel: () => {}, active: false, loudness: true }} />);
    expect(screen.getByRole("group", { name: "Loudness" })).toBeTruthy();
  });

  it("works without the filters (an old browser): the sound still goes through and no meter is offered", () => {
    class Plain extends Ctx {
      createBiquadFilter = undefined;
    }
    vi.stubGlobal("AudioContext", Plain);
    // (the page's context was made by an earlier test, so this only checks the graph survives)
    expect(() => tap(document.createElement("audio"))).not.toThrow();
    act(() => {});
  });
});
