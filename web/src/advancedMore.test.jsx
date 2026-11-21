// @vitest-environment jsdom
import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { useState } from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { api } from "./api.js";
import { AdvancedAudio } from "./AdvancedAudio.jsx";
import { AdvancedPicture } from "./AdvancedPicture.jsx";
import { ADVANCED_KEY } from "./advanced.js";
import { FLAT_EQ, PLAIN_SOUND, adjustEq, dbBandText, isFlatEq, songCuts, soundForMixdown } from "./advancedAudio.js";
import { WHOLE, sectionSpeed } from "./advancedPicture.js";
import { adjustMusic, musicForRequest, newMusic } from "./musicModel.js";
import { SequenceView } from "./SequenceView.jsx";
import { editorSeed, updatedClip } from "./sequenceClip.js";
import { transitionLimit } from "./transitionModel.js";

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

describe("the equaliser's numbers", () => {
  it("keeps each band in range, in half-dB steps, and a missing one flat", () => {
    expect(adjustEq(undefined)).toEqual(FLAT_EQ);
    expect(adjustEq({ low_db: 99, mid_db: -99, high_db: 1.24 })).toEqual({ low_db: 12, mid_db: -12, high_db: 1 });
    expect(adjustEq({ low_db: 2.3 }, { mid_db: 4 })).toEqual({ low_db: 2.5, mid_db: 4, high_db: 0 });
    expect(adjustEq({ low_db: NaN, mid_db: "x" })).toEqual(FLAT_EQ);
  });

  it("knows a flat one, and says a band in words", () => {
    expect(isFlatEq(FLAT_EQ)).toBe(true);
    expect(isFlatEq(undefined)).toBe(true);
    expect(isFlatEq({ low_db: 0.5 })).toBe(false);
    expect(dbBandText(3)).toBe("+3 dB");
    expect(dbBandText(-1.5)).toBe("-1.5 dB");
    expect(dbBandText(0)).toBe("0 dB");
    expect(dbBandText(NaN)).toBe("0 dB");
  });

  it("goes in the request for the clip only with Advanced on and the sound kept", () => {
    const sound = { ...PLAIN_SOUND, clipEq: { low_db: 4, mid_db: 0, high_db: -3 } };
    expect(soundForMixdown(sound, { advanced: true, audio: true }).clip_eq).toEqual({ low_db: 4, mid_db: 0, high_db: -3 });
    expect(soundForMixdown(sound, { advanced: false, audio: true }).clip_eq).toEqual(FLAT_EQ);
    expect(soundForMixdown(sound, { advanced: true, audio: false }).clip_eq).toEqual(FLAT_EQ);
  });

  it("is kept on the song, sent, and dropped with Advanced off", () => {
    const m = adjustMusic(newMusic({ id: "s1", duration_s: 100 }), { eq: { low_db: 20, mid_db: -2, high_db: 0 } }, 5000);
    expect(m.eq).toEqual({ low_db: 12, mid_db: -2, high_db: 0 });
    expect(musicForRequest(m, 5000).eq).toEqual({ low_db: 12, mid_db: -2, high_db: 0 });
    expect(newMusic({ id: "s1" }).eq).toEqual(FLAT_EQ);
    expect(songCuts(m, false).eq).toEqual(FLAT_EQ);
    expect(songCuts(m, true).eq).toEqual(m.eq);
  });
});

function Harness({ seen }) {
  const [sound, setSound] = useState(PLAIN_SOUND);
  const [song, setSong] = useState(newMusic({ id: "s1", title: "S", duration_s: 100 }));
  seen?.({ sound, song });
  return (
    <AdvancedAudio
      ratingKey="k1"
      start={0}
      end={4000}
      sound={sound}
      onSound={(c) => setSound((s) => ({ ...s, ...c }))}
      music={song}
      onMusic={(c) => setSong((m) => adjustMusic(m, c, 5000))}
    />
  );
}

describe("the equaliser sliders", () => {
  it("turn the clip's and the song's bands, and double-click puts one back at 0", () => {
    let seen;
    render(<Harness seen={(s) => (seen = s)} />);
    fireEvent.change(screen.getByLabelText("Clip bass"), { target: { value: "6" } });
    expect(seen.sound.clipEq).toEqual({ low_db: 6, mid_db: 0, high_db: 0 });
    expect(screen.getByLabelText("Clip bass").getAttribute("aria-valuetext")).toBe("+6 dB");
    fireEvent.change(screen.getByLabelText("Clip treble"), { target: { value: "-4.5" } });
    expect(seen.sound.clipEq).toEqual({ low_db: 6, mid_db: 0, high_db: -4.5 });
    fireEvent.doubleClick(screen.getByLabelText("Clip bass"));
    expect(seen.sound.clipEq.low_db).toBe(0);
    fireEvent.change(screen.getByLabelText("Song middle"), { target: { value: "-12" } });
    expect(seen.song.eq).toEqual({ low_db: 0, mid_db: -12, high_db: 0 });
    fireEvent.doubleClick(screen.getByLabelText("Song middle"));
    expect(seen.song.eq.mid_db).toBe(0);
  });
});

describe("speed and a window on a section of a sequence", () => {
  const sec = (id, ms, extra = {}) => ({
    id,
    rating_key: `r-${id}`,
    title: id,
    start_ms: 0,
    end_ms: ms,
    caption: { mode: "none" },
    subtitle_offset_ms: 0,
    ...extra,
  });
  const crop = { x: 0.34, y: 0, w: 0.32, h: 1 };

  it("is the section's own speed, or normal when it has a song of its own", () => {
    expect(sectionSpeed(sec("a", 4000))).toBe(1);
    expect(sectionSpeed(sec("a", 4000, { speed: 2 }))).toBe(2);
    expect(sectionSpeed(sec("a", 4000, { speed: 2, music: { id: "s" } }))).toBe(1);
    expect(sectionSpeed(sec("a", 4000, { speed: 99 }))).toBe(1);
    expect(sectionSpeed(undefined)).toBe(1);
  });

  it("starts the editor from what was stored and puts it back, and normal values are not kept", () => {
    const seed = editorSeed(sec("a", 4000, { speed: 2, reframe: { aspect: "vertical", cx: 0.3, cy: 0.5 }, crop }));
    expect(seed.speed).toBe(2);
    expect(seed.reframe).toEqual({ aspect: "vertical", cx: 0.3, cy: 0.5 });
    expect(editorSeed(sec("a", 4000)).speed).toBe(1);
    expect(editorSeed(sec("a", 4000)).reframe).toEqual(WHOLE);
    const base = { start: 0, end: 4000, caption: { mode: "none" }, subtitleOffsetMs: 0, layers: [], music: null };
    const saved = updatedClip(sec("a", 4000), { ...base, speed: 2, reframe: seed.reframe, crop });
    expect(saved).toMatchObject({ speed: 2, reframe: seed.reframe, crop });
    const plain = updatedClip(saved, { ...base, speed: 1, reframe: WHOLE, crop: null });
    expect(plain.speed).toBeUndefined();
    expect(plain.reframe).toBeUndefined();
    expect(plain.crop).toBeUndefined();
  });

  it("makes a section last as long as it plays when working out how much room a transition has", () => {
    // 4 s at 2x is 2 s: a 1.5 s transition into the next section would leave it too little
    const seq = [sec("a", 5000), sec("b", 4000, { speed: 2 }), sec("c", 5000, { transition: { kind: "fade", ms: 1500 } })];
    expect(transitionLimit(seq, 1)).toBe(500);
    const normal = [sec("a", 5000), sec("b", 4000), sec("c", 5000, { transition: { kind: "fade", ms: 1500 } })];
    expect(transitionLimit(normal, 1)).toBe(2500);
  });

  it("goes in the export only with Advanced on, and not a speed on a section with a song", async () => {
    const start = vi.spyOn(api, "startSequenceExport").mockResolvedValue({ job_id: "j1" });
    const sequence = [
      sec("a", 4000, { speed: 2, crop }),
      sec("b", 4000, { speed: 2, music: { id: "s1", start_ms: 0, at_ms: 0, volume: 1, fade_in_ms: 0, fade_out_ms: 0, duck: 0 } }),
      sec("c", 4000),
    ];
    const go = async () => {
      render(<SequenceView sequence={sequence} setSequence={() => {}} onJob={() => {}} />);
      await act(async () => {
        fireEvent.click(await screen.findByText(/Export MP4 \(3 clips\)/));
      });
      cleanup();
    };
    await go();
    expect(start.mock.calls[0][0].clips.map((c) => [c.speed, c.crop])).toEqual([
      [undefined, undefined],
      [undefined, undefined],
      [undefined, undefined],
    ]);
    window.localStorage.setItem(ADVANCED_KEY, "1");
    await go();
    expect(start.mock.calls[1][0].clips.map((c) => [c.speed, c.crop])).toEqual([
      [2, crop],
      [undefined, undefined], // it has a song of its own
      [undefined, undefined],
    ]);
  });

  it("locks the speed in the picture box when the section has a song, and says why", () => {
    render(<AdvancedPicture speed={1} onSpeed={() => {}} view={WHOLE} onView={() => {}} srcAspect={16 / 9} speedLocked />);
    expect(screen.getByLabelText("Clip speed").disabled).toBe(true);
    expect(screen.getByText(/song of its own plays at normal speed/)).toBeTruthy();
    cleanup();
    render(<AdvancedPicture speed={1} onSpeed={() => {}} view={WHOLE} onView={() => {}} srcAspect={16 / 9} />);
    expect(screen.getByLabelText("Clip speed").disabled).toBe(false);
  });
});
