import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import { Editor } from "./Editor.jsx";
import { FrameCheck } from "./FrameCheck.jsx";
import { SequenceView } from "./SequenceView.jsx";
import { Timeline, flagPos, tickStep } from "./Timeline.jsx";
import { TrimPanel } from "./TrimPanel.jsx";

const NTSC = { num: 24000, den: 1001 };
const noop = () => {};

describe("TrimPanel", () => {
  const props = {
    start: 2420,
    end: 7100,
    duration: 36_000,
    fps: NTSC,
    playhead: 3000,
    lockLength: false,
    onLock: noop,
    snap: "frame",
    onSnap: noop,
    onTrim: noop,
  };

  it("shows the points as times and frame numbers, and the length in both", () => {
    const html = renderToStaticMarkup(<TrimPanel {...props} />);
    expect(html).toContain('value="0:02.420"'); // In time
    expect(html).toContain('value="58"'); // In frame
    expect(html).toContain('value="0:07.100"'); // Out time
    expect(html).toContain('value="170"'); // Out frame
    expect(html).toContain("4.680 s");
    expect(html).toContain("112 frames");
    expect(html).toContain("23.976 fps");
  });

  it("offers the nudges and the playhead shortcut for each point", () => {
    const html = renderToStaticMarkup(<TrimPanel {...props} />);
    for (const label of ["−1s", "−10f", "−1f", "+1f", "+10f", "+1s"]) {
      expect(html.split(label).length - 1).toBe(2); // once for In, once for Out
    }
    expect(html.split("Set to playhead").length - 1).toBe(2);
  });

  it("falls back to milliseconds and disables frame boxes without a frame rate", () => {
    const html = renderToStaticMarkup(<TrimPanel {...props} fps={null} />);
    expect(html).toContain("disabled");
    expect(html).not.toContain("frames</div>");
    expect(html).not.toContain("source video (");
  });

  it("flags a clip over the 60 second limit", () => {
    const html = renderToStaticMarkup(<TrimPanel {...props} end={70_000} />);
    expect(html).toContain("trim-length-value error");
  });
});

describe("Timeline", () => {
  const cues = [
    { start_ms: 3000, end_ms: 4600, text: "Zero gravity, full flavor.", index: 0 },
    { start_ms: 9000, end_ms: 9500, text: "Out of view", index: null },
  ];
  const props = {
    ratingKey: "k1",
    view: { from: 0, to: 12_000 },
    start: 2400,
    end: 7100,
    playhead: 3000,
    fps: NTSC,
    snap: "frame",
    cues,
    onTrim: noop,
    onCueMove: noop,
    onSeek: noop,
    onZoom: noop,
    onFit: noop,
  };

  it("labels both handles and flags the points with times and frames", () => {
    const html = renderToStaticMarkup(<Timeline {...props} />);
    expect(html).toContain('aria-label="In point 0:02.400"');
    expect(html).toContain('aria-label="Out point 0:07.100"');
    expect(html).toContain("IN 0:02.400 · f58");
    expect(html).toContain("OUT 0:07.100 · f170");
  });

  it("places the handles and playhead by percentage of the view", () => {
    const html = renderToStaticMarkup(<Timeline {...props} />);
    expect(html).toContain("left:20%"); // In: 2.4 of 12 s
    expect(html).toContain("left:25%"); // playhead: 3 of 12 s
  });

  it("draws the subtitle lines, the burned ones marked", () => {
    const html = renderToStaticMarkup(<Timeline {...props} />);
    expect(html).toContain("Zero gravity, full flavor.");
    expect(html).toContain("tl-cue inside editable");
  });

  it("has zoom and fit controls", () => {
    const html = renderToStaticMarkup(<Timeline {...props} />);
    expect(html).toContain('aria-label="Zoom in"');
    expect(html).toContain('aria-label="Zoom out"');
    expect(html).toContain("Fit clip");
  });
});

describe("tickStep", () => {
  it("keeps labels roughly 80px apart", () => {
    expect(tickStep(12_000, 800)).toBe(2000); // 15 ms/px * 80 = 1.2 s -> next step up, 2 s
    expect(tickStep(1000, 800)).toBe(100);
    expect(tickStep(120_000, 800)).toBe(15_000);
    expect(tickStep(10_000_000, 800)).toBe(300_000); // never off the end of the table
  });
});

describe("FrameCheck", () => {
  it("shows the frames either side of both cuts, with their numbers", () => {
    const html = renderToStaticMarkup(<FrameCheck ratingKey="k1" start={2420} end={7100} fps={NTSC} />);
    for (const label of ["Last cut", "First kept", "Last kept", "First cut"]) expect(html).toContain(label);
    expect(html).toContain("f57");
    expect(html).toContain("f58");
    expect(html).toContain("f169");
    expect(html).toContain("f170");
    expect(html).toContain("/api/frame/k1?t=");
  });

  it("draws nothing until the frame rate is known", () => {
    expect(renderToStaticMarkup(<FrameCheck ratingKey="k1" start={0} end={1000} fps={null} />)).toBe("");
  });
});

describe("flagPos", () => {
  const view = { from: 1000, to: 11_000 };
  it("hangs the flags off the handles", () => {
    expect(flagPos("in", 3000, view)).toEqual({ right: "80%" });
    expect(flagPos("out", 6000, view)).toEqual({ left: "50%" });
  });
  it("keeps a flag at the edge when its point is out of view", () => {
    expect(flagPos("in", 0, view)).toEqual({ left: 0 });
    expect(flagPos("out", 20_000, view)).toEqual({ right: 0 });
    expect(flagPos("in", 20_000, view)).toEqual({ right: "0%" });
    expect(flagPos("out", 0, view)).toEqual({ left: "0%" });
  });
});

describe("sequence editing", () => {
  const clip = {
    id: "c1",
    rating_key: "k1",
    title: "Pilot",
    line: "Say it with pie",
    start_ms: 1000,
    end_ms: 4000,
    caption: { mode: "meme", top: "TOP", bottom: "BOTTOM", font_scale: 100 },
    subtitle_offset_ms: 0,
  };
  const seq = { clip, index: 1, count: 3, hasNext: true, output: { width: 640, fps: 12 }, onSave: noop };

  it("opens a stored clip in the full editor with save in place of export", () => {
    const html = renderToStaticMarkup(<Editor item={clip} onClose={noop} onJob={noop} sequenceEdit={seq} />);
    expect(html).toContain("Editing clip 2 of 3 in your sequence");
    expect(html).toContain("Save to sequence");
    expect(html).toContain("Save and edit next clip");
    expect(html).toContain("Back without saving");
    expect(html).not.toContain("Add to sequence");
    expect(html).not.toContain(">Export ");
    // the stored meme text is what the boxes start with
    expect(html).toContain('value="TOP"');
    expect(html).toContain('value="BOTTOM"');
    // the sequence-wide output settings are hidden
    expect(html).not.toContain("<legend>Output</legend>");
    expect(html).not.toContain("<legend>Format</legend>");
  });

  it("offers no 'next' on the last clip", () => {
    const html = renderToStaticMarkup(
      <Editor item={clip} onClose={noop} onJob={noop} sequenceEdit={{ ...seq, hasNext: false }} />
    );
    expect(html).not.toContain("Save and edit next clip");
  });

  it("is the normal editor, with export and add-to-sequence, without it", () => {
    const html = renderToStaticMarkup(<Editor item={clip} onClose={noop} onJob={noop} />);
    expect(html).toContain("Add to sequence");
    expect(html).toContain("<legend>Output</legend>");
  });

  const clips = [clip, { ...clip, id: "c2", title: "Second", start_ms: 5000, end_ms: 8000 }];
  const sequenceHtml = () =>
    renderToStaticMarkup(<SequenceView sequence={clips} setSequence={noop} onJob={noop} />);

  it("shows the whole set: a player, a strip of every section, and edits for one or all", () => {
    const html = sequenceHtml();
    // the strip: one block per section, in order, with the total against the limit
    expect(html).toContain('aria-label="Sections in play order"');
    expect(html.match(/class="seq-block[ "]/g)).toHaveLength(2);
    expect(html).toContain("2 sections");
    // the player
    expect(html).toContain("Play the sequence");
    expect(html).toContain('aria-label="Position in the sequence"');
    // one section's editor (the first is selected), and the all-sections editor
    expect(html).toContain("Section 1 of 2");
    expect(html).toContain("Detailed edit");
    expect(html).toContain("All sections");
    expect(html).toContain("Burn subtitles in all");
    expect(html).toContain("Reverse the order");
    // undo and the export controls are still there
    expect(html).toContain("↶ Undo");
    expect(html).toContain("Export GIF (2 clips)");
  });

  it("says so when there is nothing in the sequence", () => {
    const html = renderToStaticMarkup(<SequenceView sequence={[]} setSequence={noop} onJob={noop} />);
    expect(html).toContain("No clips yet");
    expect(html).not.toContain("Sections in play order");
  });

  it("flags a sequence over the limit", () => {
    const long = [
      { ...clip, id: "x1", start_ms: 0, end_ms: 60_000 },
      { ...clip, id: "x2", start_ms: 0, end_ms: 60_000 },
      { ...clip, id: "x3", start_ms: 0, end_ms: 60_000 },
      { ...clip, id: "x4", start_ms: 0, end_ms: 5_000 },
    ];
    const html = renderToStaticMarkup(<SequenceView sequence={long} setSequence={noop} onJob={noop} />);
    expect(html).toContain("over the 3-minute limit");
  });
});
