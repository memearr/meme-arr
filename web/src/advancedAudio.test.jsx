// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { useState } from "react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { api } from "./api.js";
import { AdvancedAudio } from "./AdvancedAudio.jsx";
import { ADVANCED_KEY, loadAdvanced, saveAdvanced, useAdvanced } from "./advanced.js";
import {
  HIGH_RANGE,
  LOW_RANGE,
  PLAIN_SOUND,
  cutHz,
  hzText,
  hzToStep,
  loudnessText,
  songCuts,
  soundForMixdown,
  stepToHz,
} from "./advancedAudio.js";
import { adjustMusic, musicForRequest, newMusic } from "./musicModel.js";

afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});

/** A stand-in for localStorage. */
function store(start = {}) {
  const m = new Map(Object.entries(start));
  return { getItem: (k) => (m.has(k) ? m.get(k) : null), setItem: (k, v) => m.set(k, String(v)), m };
}

describe("the Advanced switch", () => {
  it("starts off, and is whatever was left", () => {
    expect(loadAdvanced(store())).toBe(false);
    expect(loadAdvanced(store({ [ADVANCED_KEY]: "1" }))).toBe(true);
    expect(loadAdvanced(store({ [ADVANCED_KEY]: "0" }))).toBe(false);
    expect(loadAdvanced(null)).toBe(false);
  });

  it("is kept, and a browser that refuses storage is no harm", () => {
    const s = store();
    saveAdvanced(s, true);
    expect(s.m.get(ADVANCED_KEY)).toBe("1");
    saveAdvanced(s, false);
    expect(s.m.get(ADVANCED_KEY)).toBe("0");
    const broken = {
      getItem: () => {
        throw new Error("blocked");
      },
      setItem: () => {
        throw new Error("blocked");
      },
    };
    expect(loadAdvanced(broken)).toBe(false);
    expect(() => saveAdvanced(broken, true)).not.toThrow();
  });

  it("stays on between visits to the page", () => {
    const s = store();
    vi.stubGlobal("localStorage", s);
    function Box() {
      const [on, set] = useAdvanced();
      return (
        <label>
          <input type="checkbox" checked={on} onChange={(e) => set(e.target.checked)} /> Advanced
        </label>
      );
    }
    const first = render(<Box />);
    expect(screen.getByLabelText("Advanced").checked).toBe(false);
    fireEvent.click(screen.getByLabelText("Advanced"));
    expect(screen.getByLabelText("Advanced").checked).toBe(true);
    first.unmount();
    render(<Box />); // a new visit
    expect(screen.getByLabelText("Advanced").checked).toBe(true);
    fireEvent.click(screen.getByLabelText("Advanced"));
    expect(s.m.get(ADVANCED_KEY)).toBe("0");
  });
});

describe("the numbers of the advanced sound", () => {
  it("maps a slider to a pitch and back, with 0 for off", () => {
    expect(stepToHz(0, HIGH_RANGE)).toBe(0);
    expect(stepToHz(100, HIGH_RANGE)).toBe(2000);
    expect(stepToHz(1, LOW_RANGE)).toBeGreaterThanOrEqual(1000);
    for (const hz of [20, 120, 800, 2000]) expect(stepToHz(hzToStep(hz, HIGH_RANGE), HIGH_RANGE)).toBeCloseTo(hz, -1);
    expect(hzToStep(0, LOW_RANGE)).toBe(0);
    expect(hzToStep(NaN, LOW_RANGE)).toBe(0);
    expect(hzToStep(99999, LOW_RANGE)).toBe(100);
    expect(stepToHz("junk", HIGH_RANGE)).toBe(0);
  });

  it("says a pitch in words", () => {
    expect(hzText(0)).toBe("Off");
    expect(hzText(120)).toBe("120 Hz");
    expect(hzText(4500)).toBe("4.5 kHz");
  });

  it("says a reading, or why there is none", () => {
    expect(loudnessText(null)).toBe("");
    expect(loudnessText({ has_audio: false })).toMatch(/no sound/);
    expect(loudnessText({ has_audio: true, lufs: null })).toMatch(/silent/);
    expect(loudnessText({ has_audio: true, lufs: -14.94, true_peak_db: -0.41, range_lu: 5.3 })).toBe(
      "-14.9 LUFS, peak -0.4 dB, range 5.3 LU",
    );
  });

  it("keeps a cut inside what the server takes", () => {
    expect(cutHz(5, 20, 2000)).toBe(20);
    expect(cutHz(99999, 20, 2000)).toBe(2000);
    expect(cutHz(NaN, 20, 2000)).toBe(0);
    expect(cutHz(-3, 20, 2000)).toBe(0);
    expect(cutHz(300.4, 20, 2000)).toBe(300);
  });

  it("sends the settings only with Advanced on and the sound kept", () => {
    const sound = { clipHigh: 120, clipLow: 8000, normalize: -16 };
    expect(soundForMixdown(sound, { advanced: true, audio: true })).toEqual({
      clip_highpass_hz: 120,
      clip_lowpass_hz: 8000,
      normalize_lufs: -16,
    });
    const none = { clip_highpass_hz: 0, clip_lowpass_hz: 0, normalize_lufs: null };
    expect(soundForMixdown(sound, { advanced: false, audio: true })).toEqual(none);
    expect(soundForMixdown(sound, { advanced: true, audio: false })).toEqual(none);
    expect(soundForMixdown(PLAIN_SOUND, { advanced: true, audio: true })).toEqual(none);
  });

  it("keeps the song's cuts in range, sends them, and drops them with Advanced off", () => {
    const m = adjustMusic({ ...newMusic({ id: "s1", duration_s: 100 }) }, { highpass_hz: 5000, lowpass_hz: 300 }, 5000);
    expect(m.highpass_hz).toBe(2000);
    expect(m.lowpass_hz).toBe(1000);
    expect(musicForRequest({ ...m, highpass_hz: 150, lowpass_hz: 9000 }, 5000).highpass_hz).toBe(150);
    expect(newMusic({ id: "s1" }).lowpass_hz).toBe(0);
    expect(songCuts(m, true)).toBe(m);
    expect(songCuts(m, false)).toMatchObject({ highpass_hz: 0, lowpass_hz: 0, id: "s1" });
    expect(songCuts(null, false)).toBe(null);
  });
});

function Harness({ music = true, clipSound = true, seen }) {
  const [sound, setSound] = useState(PLAIN_SOUND);
  const [song, setSong] = useState(music ? newMusic({ id: "s1", title: "S", duration_s: 100 }) : null);
  seen?.({ sound, song });
  return (
    <AdvancedAudio
      ratingKey="k1"
      start={1000}
      end={5000}
      sound={sound}
      onSound={(c) => setSound((s) => ({ ...s, ...c }))}
      music={song}
      onMusic={(c) => setSong((m) => adjustMusic(m, c, 5000))}
      clipSound={clipSound}
    />
  );
}

describe("the advanced sound box", () => {
  it("cuts the clip and the song from their own sliders, and double-click turns a cut off", () => {
    let seen;
    render(<Harness seen={(s) => (seen = s)} />);
    fireEvent.change(screen.getByLabelText("Clip high-pass"), { target: { value: "50" } });
    expect(seen.sound.clipHigh).toBeGreaterThan(100);
    expect(screen.getByLabelText("Clip high-pass").getAttribute("aria-valuetext")).toMatch(/Hz/);
    fireEvent.change(screen.getByLabelText("Song low-pass"), { target: { value: "100" } });
    expect(seen.song.lowpass_hz).toBe(20000);
    fireEvent.doubleClick(screen.getByLabelText("Song low-pass"));
    expect(seen.song.lowpass_hz).toBe(0);
  });

  it("asks for a loudness target, and takes it away", () => {
    let seen;
    render(<Harness seen={(s) => (seen = s)} />);
    expect(screen.queryByLabelText("Loudness target")).toBeNull();
    fireEvent.click(screen.getByLabelText("Normalise the mix"));
    expect(seen.sound.normalize).toBe(-16);
    fireEvent.change(screen.getByLabelText("Loudness target"), { target: { value: "-23" } });
    expect(seen.sound.normalize).toBe(-23);
    fireEvent.click(screen.getByLabelText("Normalise the mix"));
    expect(seen.sound.normalize).toBe(null);
  });

  it("measures the clip's loudness over the clip and shows it", async () => {
    const spy = vi
      .spyOn(api, "clipLoudness")
      .mockResolvedValue({ has_audio: true, lufs: -19.2, true_peak_db: -3, range_lu: 2 });
    render(<Harness />);
    fireEvent.click(screen.getByRole("button", { name: /Measure the clip/ }));
    await waitFor(() => expect(screen.getByRole("status").textContent).toBe("-19.2 LUFS, peak -3.0 dB, range 2.0 LU"));
    expect(spy).toHaveBeenCalledWith("k1", 1000, 5000);
  });

  it("says when it could not measure", async () => {
    vi.spyOn(api, "clipLoudness").mockRejectedValue(new Error("ffmpeg went wrong"));
    render(<Harness />);
    fireEvent.click(screen.getByRole("button", { name: /Measure the clip/ }));
    await waitFor(() => expect(screen.getByRole("status").textContent).toBe("ffmpeg went wrong"));
  });

  it("has only the song in a sequence's clip, and only the clip without a song", () => {
    const { unmount } = render(<Harness clipSound={false} />);
    expect(screen.getByLabelText("Song high-pass")).toBeTruthy();
    expect(screen.queryByLabelText("Clip high-pass")).toBeNull();
    expect(screen.queryByLabelText("Normalise the mix")).toBeNull();
    unmount();
    render(<Harness music={false} />);
    expect(screen.getByLabelText("Clip high-pass")).toBeTruthy();
    expect(screen.queryByLabelText("Song high-pass")).toBeNull();
  });
});
