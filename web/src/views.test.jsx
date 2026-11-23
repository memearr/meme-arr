// @vitest-environment jsdom
//
// The Activity, System and Wanted views, each driven through its states with the server and the
// panels that have their own tests replaced by stand-ins.
import { act, cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("./api.js", () => ({ api: new Proxy({}, { get: (t, k) => (t[k] ??= vi.fn()) }) }));
vi.mock("./TasksPanel.jsx", () => ({ TasksPanel: () => <div>tasks panel</div> }));
vi.mock("./BackupsPanel.jsx", () => ({ BackupsPanel: () => <div>backups panel</div> }));
vi.mock("./LogsPanel.jsx", () => ({ LogsPanel: () => <div>logs panel</div> }));
vi.mock("./BazarrActions.jsx", () => ({ default: ({ itemId }) => <span>bazarr {itemId}</span> }));
vi.mock("./SubtitleRequests.jsx", () => ({
  default: ({ requests, loading, error, onChanged, onOpen }) => (
    <div>
      <span>requests {requests.length}</span>
      {loading && <span>requests loading</span>}
      {error && <span>{error}</span>}
      <button onClick={onChanged}>changed</button>
      <button onClick={() => onOpen({ rating_key: "r1" })}>open request</button>
    </div>
  ),
}));
vi.mock("./SubtitleSearch.jsx", () => ({
  default: ({ itemId, onIndexed, onRequested }) => (
    <div>
      <span>search {itemId}</span>
      <button onClick={onIndexed}>indexed</button>
      <button onClick={onRequested}>requested</button>
    </div>
  ),
  SubtitleStatus: () => <div>subtitle status</div>,
  CapabilityNote: ({ cap }) => (cap ? <p>capability note</p> : null),
  useCapability: () => [globalThis.__cap],
}));

import { api } from "./api.js";
import { ActivityView, fmtClock } from "./ActivityView.jsx";
import { SystemView } from "./SystemView.jsx";
import WantedView from "./WantedView.jsx";

beforeEach(() => {
  for (const k of Object.keys(api)) api[k].mockReset();
  window.confirm = vi.fn(() => true);
  globalThis.__cap = { supported: true };
});
afterEach(() => {
  cleanup();
  vi.useRealTimers();
});

// ---- Activity ---------------------------------------------------------------

describe("ActivityView", () => {
  const log = [
    { ts: 1700000000, level: "error", message: "it broke" },
    { ts: 1700000100, level: "warn", message: "careful" },
    { ts: 1700000200, level: "info", message: "fine" },
  ];

  it("formats clock times, and nothing for no time", () => {
    expect(fmtClock(0)).toBe("");
    expect(fmtClock(1700000000)).toMatch(/\d/);
  });

  it("shows an idle index, an empty log and no exports", async () => {
    api.activityLog.mockResolvedValue({ entries: [] });
    render(<ActivityView index={{ stats: { media_count: 3, cue_count: 9 } }} jobs={[]} />);
    expect(screen.getByText(/Idle — 3 items, 9 lines indexed/)).toBeTruthy();
    expect(await screen.findByText("No activity yet.")).toBeTruthy();
    expect(screen.getByText("No exports yet.")).toBeTruthy();
    expect(screen.getByText("Clear log").disabled).toBe(true);
  });

  it("shows indexing progress", async () => {
    api.activityLog.mockResolvedValue({});
    render(<ActivityView index={{ status: { running: true, done: 1, total: 4, current: "Show S01E01" } }} jobs={[]} />);
    expect(screen.getByText(/1\/4 \(25%\)/)).toBeTruthy();
    expect(screen.getByText(/Show S01E01/)).toBeTruthy();
    await screen.findByText("No activity yet.");
  });

  it("shows no progress figure for a run of unknown size", async () => {
    api.activityLog.mockResolvedValue({ entries: [] });
    render(<ActivityView index={{ status: { running: true, done: 0, total: 0 } }} jobs={[]} />);
    expect(screen.getByText(/0\/0 \(0%\)/)).toBeTruthy();
    await screen.findByText("No activity yet.");
  });

  it("lists log lines and this session's exports", async () => {
    api.activityLog.mockResolvedValue({ entries: log });
    const jobs = [
      { id: "1", status: "done", filename: "a.gif", download_path: "/api/download/a.gif" },
      { id: "2", status: "failed", filename: "b.gif", error: "ffmpeg died" },
    ];
    render(<ActivityView jobs={jobs} />);
    expect(await screen.findByText("it broke")).toBeTruthy();
    expect(screen.getByText("careful")).toBeTruthy();
    expect(screen.getByText("download").getAttribute("href")).toBe("/api/download/a.gif");
    expect(screen.getByText("ffmpeg died")).toBeTruthy();
  });

  it("shows a failed load of the log", async () => {
    api.activityLog.mockRejectedValue(new Error("log unavailable"));
    render(<ActivityView jobs={[]} />);
    expect(await screen.findByText("log unavailable")).toBeTruthy();
  });

  it("clears the log once confirmed, and reports a failure", async () => {
    api.activityLog.mockResolvedValue({ entries: log });
    api.clearActivityLog.mockResolvedValueOnce({});
    render(<ActivityView jobs={[]} />);
    await screen.findByText("it broke");
    fireEvent.click(screen.getByText("Clear log"));
    await waitFor(() => expect(api.clearActivityLog).toHaveBeenCalledTimes(1));
    await waitFor(() => expect(api.activityLog).toHaveBeenCalledTimes(2));

    api.clearActivityLog.mockRejectedValueOnce(new Error("cannot clear"));
    fireEvent.click(screen.getByText("Clear log"));
    expect(await screen.findByText("cannot clear")).toBeTruthy();
  });

  it("leaves the log alone when the confirmation is declined", async () => {
    api.activityLog.mockResolvedValue({ entries: log });
    render(<ActivityView jobs={[]} />);
    await screen.findByText("it broke");
    window.confirm = vi.fn(() => false);
    fireEvent.click(screen.getByText("Clear log"));
    expect(api.clearActivityLog).not.toHaveBeenCalled();
  });

  it("refreshes the log on a timer", async () => {
    vi.useFakeTimers();
    api.activityLog.mockResolvedValue({ entries: [] });
    render(<ActivityView jobs={[]} />);
    await act(async () => {
      await vi.advanceTimersByTimeAsync(4100);
    });
    expect(api.activityLog.mock.calls.length).toBeGreaterThanOrEqual(2);
  });
});

// ---- System -----------------------------------------------------------------

describe("SystemView", () => {
  const storage = {
    database: {
      total_bytes: 5000,
      db_file_bytes: 4000,
      wal_bytes: 1000,
      cue_rows: 10,
      subtitle_text_bytes: 100,
      media_rows: 2,
      media_text_bytes: 50,
      index_overhead_bytes: 25,
    },
    preview_cache: { files: 3, bytes: 3000 },
    exports: { files: 2, bytes: 2000 },
  };
  const libs = {
    libraries: [
      { library: "TV", kind: "episode", total: 5, indexed: 4, cue_count: 40 },
      { library: "TV", kind: "movie", total: 1, indexed: 1, cue_count: 2 },
      { library: "Movies", kind: "movie", total: 9, indexed: 9, cue_count: 90 },
    ],
  };
  const issue = { level: "warning", message: "Path missing", fix: "Map it", docs_url: "https://docs" };
  const health = { checked_at: 1700000000, issues: [issue, { ...issue, level: "error", message: "Down" }] };

  beforeEach(() => {
    api.libraryStats.mockResolvedValue(libs);
    api.systemStorage.mockResolvedValue(storage);
  });

  const open = async (label) => {
    const row = (await screen.findByText(label)).closest(".kv");
    fireEvent.click(row);
    return row;
  };

  it("shows a first-check state, then the findings", async () => {
    const { rerender } = render(<SystemView index={{}} health={null} onHealth={vi.fn()} />);
    expect(screen.getByText("first check running…")).toBeTruthy();
    expect(screen.getAllByText("Checking…").length).toBeGreaterThan(0);
    rerender(<SystemView index={{}} health={health} onHealth={vi.fn()} />);
    expect(screen.getByText("Path missing")).toBeTruthy();
    expect(screen.getByText("Down")).toBeTruthy();
    expect(screen.getByText("1 error, 1 warning")).toBeTruthy();
    rerender(<SystemView index={{}} health={{ checked_at: 5, issues: [] }} onHealth={vi.fn()} />);
    expect(screen.getAllByText(/No problems found/).length).toBe(2);
    await screen.findByText("subtitle status");
  });

  it("re-checks health and reports a failure", async () => {
    const onHealth = vi.fn();
    api.recheckHealth.mockResolvedValueOnce({ checked_at: 9, issues: [] });
    render(<SystemView index={{}} health={health} onHealth={onHealth} />);
    fireEvent.click(screen.getByText("Re-check now"));
    await waitFor(() => expect(onHealth).toHaveBeenCalledWith({ checked_at: 9, issues: [] }));
    api.recheckHealth.mockRejectedValueOnce(new Error("check failed"));
    fireEvent.click(screen.getByText("Re-check now"));
    expect(await screen.findByText("check failed")).toBeTruthy();
  });

  it("disables the re-check while one is running", () => {
    render(<SystemView index={{}} health={{ ...health, running: true }} onHealth={vi.fn()} />);
    expect(screen.getByText("Checking…", { selector: "button" }).disabled).toBe(true);
  });

  it("drills into media and subtitle counts per library", async () => {
    render(<SystemView index={{ stats: { media_count: 15, cue_count: 132 } }} health={health} onHealth={vi.fn()} />);
    await screen.findByText("subtitle status");
    await waitFor(() => expect(api.libraryStats).toHaveBeenCalled());
    const media = await open("Indexed media");
    expect(media.getAttribute("aria-expanded")).toBe("true");
    expect(screen.getAllByText("episode").length).toBe(1);
    fireEvent.keyDown(media, { key: "Enter" });
    expect(media.getAttribute("aria-expanded")).toBe("false");
    fireEvent.keyDown(media, { key: " " });
    fireEvent.keyDown(media, { key: "a" });
    expect(media.getAttribute("aria-expanded")).toBe("true");

    await open("Subtitle lines");
    expect(screen.getAllByText("Movies").length).toBeGreaterThan(0);
  });

  it("copes with the library stats failing", async () => {
    api.libraryStats.mockRejectedValue(new Error("no stats"));
    render(<SystemView index={undefined} health={undefined} onHealth={vi.fn()} />);
    expect(await screen.findByText("Application")).toBeTruthy();
  });

  it("shows the storage breakdown", async () => {
    render(<SystemView index={{}} health={health} onHealth={vi.fn()} />);
    await open("Index storage");
    expect(await screen.findByText("Database file")).toBeTruthy();
    expect(screen.getByText("Write-ahead log")).toBeTruthy();
    expect(screen.getByText(/Preview cache \(3 files\)/)).toBeTruthy();
  });

  it("hides the write-ahead log row when there is none", async () => {
    api.systemStorage.mockResolvedValue({ ...storage, database: { ...storage.database, wal_bytes: 0 } });
    render(<SystemView index={{}} health={health} onHealth={vi.fn()} />);
    await open("Index storage");
    await screen.findByText("Database file");
    expect(screen.queryByText("Write-ahead log")).toBeNull();
  });

  it("shows a storage failure", async () => {
    api.systemStorage.mockRejectedValue(new Error("no storage"));
    render(<SystemView index={{}} health={health} onHealth={vi.fn()} />);
    expect(await screen.findByText("no storage")).toBeTruthy();
    expect(screen.getByText("–")).toBeTruthy();
  });

  it("vacuums, clears the cache and clears exports, reporting each result", async () => {
    api.vacuumDatabase.mockResolvedValue({ saved_bytes: 1000, duration_ms: 1500, before_bytes: 3000, after_bytes: 2000 });
    api.clearPreviewCache.mockResolvedValue({ bytes: 3000, files: 3 });
    api.clearExports.mockResolvedValue({ bytes: 2000, files: 2 });
    render(<SystemView index={{}} health={health} onHealth={vi.fn()} />);
    await open("Index storage");
    fireEvent.click(await screen.findByText(/Vacuum & optimize/));
    expect(await screen.findByText(/Freed .* in 1.5s/)).toBeTruthy();
    fireEvent.click(screen.getByText("Clear preview cache"));
    expect(await screen.findByText(/Cleared preview cache: .* \(3 files\)/)).toBeTruthy();
    fireEvent.click(screen.getByText("Clear exported memes"));
    expect(await screen.findByText(/Cleared exported memes: .* \(2 files\)/)).toBeTruthy();
  });

  it("reports maintenance that failed, and skips clearing exports when declined", async () => {
    api.vacuumDatabase.mockRejectedValue(new Error("vacuum failed"));
    api.clearPreviewCache.mockRejectedValue(new Error("cache failed"));
    api.clearExports.mockRejectedValue(new Error("exports failed"));
    render(<SystemView index={{}} health={health} onHealth={vi.fn()} />);
    await open("Index storage");
    fireEvent.click(await screen.findByText(/Vacuum & optimize/));
    expect(await screen.findByText("vacuum failed")).toBeTruthy();
    fireEvent.click(screen.getByText("Clear preview cache"));
    expect(await screen.findByText("cache failed")).toBeTruthy();
    window.confirm = vi.fn(() => false);
    fireEvent.click(screen.getByText("Clear exported memes"));
    expect(api.clearExports).not.toHaveBeenCalled();
    window.confirm = vi.fn(() => true);
    fireEvent.click(screen.getByText("Clear exported memes"));
    expect(await screen.findByText("exports failed")).toBeTruthy();
  });

  it("disables clearing when there is nothing to clear", async () => {
    api.systemStorage.mockResolvedValue({
      ...storage,
      preview_cache: { files: 0, bytes: 0 },
      exports: { files: 0, bytes: 0 },
    });
    render(<SystemView index={{}} health={health} onHealth={vi.fn()} />);
    await open("Index storage");
    expect((await screen.findByText("Clear preview cache")).disabled).toBe(true);
    expect(screen.getByText("Clear exported memes").disabled).toBe(true);
  });

  it("says when nothing has been exported", async () => {
    api.listExports.mockResolvedValue({ files: [] });
    render(<SystemView index={{}} health={health} onHealth={vi.fn()} />);
    await open("Exported memes");
    expect(await screen.findByText("Nothing exported yet.")).toBeTruthy();
  });

  it("shows exported files with download links and refreshes on request", async () => {
    api.listExports.mockResolvedValue({ files: [{ name: "a b.gif", bytes: 2048, modified_at: 1700000000 }] });
    render(<SystemView index={{}} health={health} onHealth={vi.fn()} />);
    await open("Exported memes");
    const link = await screen.findByText("download");
    expect(link.getAttribute("href")).toBe("/api/download/a%20b.gif");
    fireEvent.click(screen.getByText("↻ refresh"));
    await waitFor(() => expect(api.listExports).toHaveBeenCalledTimes(2));
  });

  it("shows a failed export listing", async () => {
    api.listExports.mockRejectedValue(new Error("no exports"));
    render(<SystemView index={{}} health={health} onHealth={vi.fn()} />);
    await open("Exported memes");
    expect(await screen.findByText("no exports")).toBeTruthy();
    expect(screen.getByText("Loading…")).toBeTruthy();
  });

  it("reloads the export list after clearing them when it is open", async () => {
    api.listExports.mockResolvedValue({ files: [{ name: "x.gif", bytes: 1, modified_at: 1 }] });
    api.clearExports.mockResolvedValue({ bytes: 1, files: 1 });
    render(<SystemView index={{}} health={health} onHealth={vi.fn()} />);
    await open("Index storage");
    await open("Exported memes");
    await screen.findByText("download");
    fireEvent.click(screen.getByText("Clear exported memes"));
    await waitFor(() => expect(api.listExports).toHaveBeenCalledTimes(2));
  });
});

// ---- Wanted -----------------------------------------------------------------

describe("WantedView", () => {
  const item = (n, over = {}) => ({ rating_key: `k${n}`, title: `Item ${n}`, library: "TV", ...over });
  const page = (items, total = items.length, unreachable = 0) => ({ items, total, unreachable });

  beforeEach(() => {
    api.libraryStats.mockResolvedValue({ libraries: [{ library: "TV" }, { library: "Movies" }, { library: "TV" }] });
    api.subtitleRequests.mockResolvedValue({ requests: [{ id: 1 }, { id: 2 }] });
    api.wanted.mockResolvedValue(page([item(1), item(2, { library: "" })], 2, 3));
  });

  const settle = () => screen.findByText("Item 1");

  it("lists what is wanted, with the unreachable note and request count", async () => {
    render(<WantedView onOpen={vi.fn()} />);
    await settle();
    expect(screen.getByText("Missing (2)")).toBeTruthy();
    expect(screen.getByText("—")).toBeTruthy();
    expect(screen.getByText("bazarr k1")).toBeTruthy();
    expect(await screen.findByText(/Requests \(2\)/)).toBeTruthy();
  });

  it("says nothing is wanted, or that nothing matches the filter", async () => {
    api.wanted.mockResolvedValue(page([]));
    render(<WantedView onOpen={vi.fn()} />);
    expect(await screen.findByText(/Nothing wanted/)).toBeTruthy();
    fireEvent.change(screen.getByPlaceholderText(/Filter by show/), { target: { value: "zzz" } });
    expect(await screen.findByText("Nothing matches.")).toBeTruthy();
    await waitFor(() => expect(api.wanted).toHaveBeenLastCalledWith("zzz", [], 0, 50));
  });

  it("shows a failed load", async () => {
    api.wanted.mockRejectedValue(new Error("wanted failed"));
    render(<WantedView onOpen={vi.fn()} />);
    expect(await screen.findByText("wanted failed")).toBeTruthy();
  });

  it("loads more, and reports a failure doing so", async () => {
    api.wanted.mockResolvedValueOnce(page([item(1)], 3));
    api.wanted.mockResolvedValueOnce(page([item(2), item(3)], 3));
    render(<WantedView onOpen={vi.fn()} />);
    await settle();
    fireEvent.click(screen.getByText("Load more (2 left)"));
    expect(await screen.findByText("Item 3")).toBeTruthy();
    expect(screen.queryByText(/Load more/)).toBeNull();

    api.wanted.mockResolvedValueOnce(page([item(1)], 3));
    api.wanted.mockRejectedValueOnce(new Error("page failed"));
    fireEvent.change(screen.getByPlaceholderText(/Filter by show/), { target: { value: "x" } });
    fireEvent.click(await screen.findByText("Load more (2 left)"));
    expect(await screen.findByText("page failed")).toBeTruthy();
  });

  it("opens an item's search, closes it, and drops the item once it has subtitles", async () => {
    render(<WantedView onOpen={vi.fn()} />);
    await settle();
    fireEvent.click(screen.getAllByText("Find subtitles")[0]);
    expect(screen.getByText("search k1")).toBeTruthy();
    fireEvent.click(screen.getByText("Close"));
    expect(screen.queryByText("search k1")).toBeNull();

    fireEvent.click(screen.getAllByText("Find subtitles")[0]);
    fireEvent.click(screen.getByText("requested"));
    fireEvent.click(screen.getByText("indexed"));
    await waitFor(() => expect(screen.queryByText("Item 1")).toBeNull());
    expect(screen.getByText(/Now searchable: Item 1/)).toBeTruthy();
    expect(screen.getByText("Missing (1)")).toBeTruthy();
  });

  it("opens the item in the editor", async () => {
    const onOpen = vi.fn();
    render(<WantedView onOpen={onOpen} />);
    await settle();
    fireEvent.click(screen.getAllByText("Open")[0]);
    expect(onOpen).toHaveBeenCalledWith(expect.objectContaining({ rating_key: "k1" }));
  });

  it("disables searching when the server can't do it", async () => {
    globalThis.__cap = { supported: false };
    render(<WantedView onOpen={vi.fn()} />);
    await settle();
    expect(screen.getAllByText("Find subtitles")[0].disabled).toBe(true);
    expect(screen.getByText("capability note")).toBeTruthy();
  });

  it("switches to the requests tab and back", async () => {
    const onOpen = vi.fn();
    render(<WantedView onOpen={onOpen} />);
    await settle();
    fireEvent.click(screen.getByRole("tab", { name: /Requests/ }));
    expect(await screen.findByText("requests 2")).toBeTruthy();
    fireEvent.click(screen.getByText("open request"));
    expect(onOpen).toHaveBeenCalledWith({ rating_key: "r1" });
    const before = api.wanted.mock.calls.length;
    fireEvent.click(screen.getByText("changed"));
    await waitFor(() => expect(api.wanted.mock.calls.length).toBeGreaterThan(before));
    fireEvent.click(screen.getByRole("tab", { name: /Missing/ }));
    expect(await screen.findByText("Item 1")).toBeTruthy();
  });

  it("shows a failed load of requests", async () => {
    api.subtitleRequests.mockRejectedValue(new Error("requests failed"));
    render(<WantedView onOpen={vi.fn()} />);
    await settle();
    fireEvent.click(screen.getByRole("tab", { name: /Requests/ }));
    expect(await screen.findByText("requests failed")).toBeTruthy();
  });

  it("starts from a focus: filter, libraries and an item's search", async () => {
    const onFocusHandled = vi.fn();
    render(
      <WantedView
        onOpen={vi.fn()}
        onFocusHandled={onFocusHandled}
        focus={{ query: "Item 1", libraries: ["TV"], openKey: "k1", label: "Item 1" }}
      />
    );
    expect(await screen.findByText("search k1")).toBeTruthy();
    expect(onFocusHandled).toHaveBeenCalled();
    expect(api.wanted).toHaveBeenCalledWith("Item 1", ["TV"], 0, 50);
  });

  it("explains a focus whose item isn't on the list", async () => {
    render(
      <WantedView
        onOpen={vi.fn()}
        focus={{ query: "", libraries: [], openKey: "gone", label: "Gone item" }}
      />
    );
    expect(await screen.findByText(/Gone item.*isn't on the Wanted list/)).toBeTruthy();
  });

  it("refreshes the requests every half minute", async () => {
    vi.useFakeTimers();
    render(<WantedView onOpen={vi.fn()} />);
    await act(async () => {
      await vi.advanceTimersByTimeAsync(30500);
    });
    expect(api.subtitleRequests.mock.calls.length).toBeGreaterThanOrEqual(2);
  });
});
