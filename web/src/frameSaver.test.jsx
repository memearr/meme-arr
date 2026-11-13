import { renderToStaticMarkup } from "react-dom/server";
import { afterEach, describe, expect, it, vi } from "vitest";
import { api } from "./api.js";
import { FrameSaver } from "./Editor.jsx";

const noop = () => {};
const NTSC = { num: 24000, den: 1001 };

function view(over = {}) {
  return renderToStaticMarkup(
    <FrameSaver
      format="png"
      onFormat={noop}
      width={960}
      onWidth={noop}
      captionMode="none"
      playhead={1250}
      srcFps={NTSC}
      state={null}
      onSave={noop}
      {...over}
    />
  );
}

describe("FrameSaver", () => {
  it("offers PNG, JPG and GIF, a width, and a button naming the frame", () => {
    const html = view();
    for (const f of ["PNG", "JPG", "GIF"]) expect(html).toContain(f);
    expect(html).toContain("960px");
    expect(html).toContain("1920px");
    expect(html).toContain("Save frame at 0:01.250");
    expect(html).toContain("f30"); // 1.25 s at 23.976 fps
  });

  it("checks the chosen format", () => {
    expect(view({ format: "jpg" })).toMatch(/checked=""[^>]*\/>JPG/);
  });

  it("says what caption the picture gets", () => {
    expect(view({ captionMode: "none" })).toContain("no caption");
    expect(view({ captionMode: "source" })).toContain("subtitle that is on screen");
    expect(view({ captionMode: "meme" })).toContain("your meme text");
  });

  it("disables the button while saving and shows an error", () => {
    expect(view({ state: { busy: true } })).toMatch(/disabled=""[^>]*>Saving/);
    expect(view({ state: { error: "ffmpeg failed" } })).toContain("ffmpeg failed");
  });

  it("shows the finished picture with a download link", () => {
    const html = view({
      state: { filename: "frame.png", download_path: "/api/download/frame.png", at: 1250 },
    });
    expect(html).toContain('src="/api/download/frame.png"');
    expect(html).toContain("Download frame.png");
    expect(html).toContain('download="frame.png"');
  });
});

describe("api.saveFrame", () => {
  afterEach(() => vi.restoreAllMocks());

  it("posts the frame request as JSON to /api/still", async () => {
    const fetchMock = vi
      .fn()
      .mockResolvedValue(new Response(JSON.stringify({ filename: "a.png" }), { status: 200 }));
    vi.stubGlobal("fetch", fetchMock);
    const body = { rating_key: "7", t_ms: 1250, format: "png", width: 960, caption: { mode: "none" } };
    const r = await api.saveFrame(body);
    expect(r.filename).toBe("a.png");
    const [url, init] = fetchMock.mock.calls[0];
    expect(String(url)).toContain("/api/still");
    expect(init.method).toBe("POST");
    expect(JSON.parse(init.body)).toEqual(body);
    vi.unstubAllGlobals();
  });
});
