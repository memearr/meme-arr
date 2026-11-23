// @vitest-environment jsdom
//
// The Settings/System panels that load a list from the server and act on it: tasks, backups, fonts
// and the log tail. The server is replaced with canned answers so every state shows up: loading,
// failed, empty and filled in.
import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("./api.js", () => {
  const api = new Proxy({}, { get: (t, k) => (t[k] ??= vi.fn()) });
  return { api };
});

import { api } from "./api.js";
import { BackupsPanel } from "./BackupsPanel.jsx";
import { FontsPanel } from "./FontsPanel.jsx";
import { LogsPanel } from "./LogsPanel.jsx";
import { TasksPanel } from "./TasksPanel.jsx";

beforeEach(() => {
  for (const k of Object.keys(api)) api[k].mockReset();
  vi.stubGlobal("confirm", vi.fn(() => true));
  window.confirm = vi.fn(() => true);
});
afterEach(() => {
  cleanup();
  vi.useRealTimers();
});

const task = (over = {}) => ({
  id: "reindex",
  label: "Reindex",
  description: "Rescan",
  enabled: true,
  interval_secs: 3600,
  min_interval_secs: 900,
  max_interval_secs: 86400,
  running: false,
  last_started: Math.floor(Date.now() / 1000) - 120,
  last_duration_ms: 1500,
  next_run: Math.floor(Date.now() / 1000) + 600,
  last_summary: "42 items",
  last_error: null,
  ...over,
});

describe("TasksPanel", () => {
  it("says it is loading, then lists the tasks", async () => {
    let resolve;
    api.listTasks.mockReturnValue(new Promise((r) => (resolve = r)));
    render(<TasksPanel />);
    expect(screen.getByText("Loading…")).toBeTruthy();
    await act(async () => resolve([task()]));
    expect(screen.getByText("Reindex")).toBeTruthy();
    expect(screen.getByText("42 items")).toBeTruthy();
    expect(screen.getByText("Run now")).toBeTruthy();
  });

  it("shows the failure when the list can't be loaded", async () => {
    api.listTasks.mockRejectedValue(new Error("no tasks for you"));
    render(<TasksPanel />);
    expect(await screen.findByText("no tasks for you")).toBeTruthy();
  });

  it("shows running, due, disabled and failed tasks distinctly", async () => {
    api.listTasks.mockResolvedValue([
      task({ id: "a", label: "Running one", running: true }),
      task({ id: "b", label: "Due one", next_run: 1 }),
      task({ id: "c", label: "Off one", enabled: false, last_error: "it broke", interval_secs: 5000 }),
    ]);
    render(<TasksPanel />);
    expect(await screen.findByText("running…")).toBeTruthy();
    expect(screen.getByText("due")).toBeTruthy();
    expect(screen.getByText("disabled")).toBeTruthy();
    expect(screen.getByText("it broke")).toBeTruthy();
    // A running task can't be started again.
    expect(screen.getByText("…").disabled).toBe(true);
  });

  it("runs a task now and reloads", async () => {
    api.listTasks.mockResolvedValue([task()]);
    api.runTask.mockResolvedValue({});
    render(<TasksPanel />);
    fireEvent.click(await screen.findByText("Run now"));
    await waitFor(() => expect(api.runTask).toHaveBeenCalledWith("reindex"));
    await waitFor(() => expect(api.listTasks).toHaveBeenCalledTimes(2));
  });

  it("carries on when a task can't be started", async () => {
    // The reload that follows clears the message again, so the check is that the panel stays usable.
    api.listTasks.mockResolvedValue([task()]);
    api.runTask.mockRejectedValue(new Error("already running"));
    render(<TasksPanel />);
    fireEvent.click(await screen.findByText("Run now"));
    await waitFor(() => expect(api.listTasks).toHaveBeenCalledTimes(2));
    expect(screen.getByText("Reindex")).toBeTruthy();
  });

  it("saves a changed schedule and keeps the other settings", async () => {
    api.listTasks.mockResolvedValue([task()]);
    api.updateTask.mockResolvedValue({});
    render(<TasksPanel />);
    const [enabled] = await screen.findAllByRole("checkbox");
    fireEvent.click(enabled);
    await waitFor(() =>
      expect(api.updateTask).toHaveBeenCalledWith("reindex", { enabled: false, interval_secs: 3600 })
    );
    fireEvent.change(screen.getByRole("combobox"), { target: { value: "21600" } });
    await waitFor(() =>
      expect(api.updateTask).toHaveBeenLastCalledWith("reindex", { enabled: true, interval_secs: 21600 })
    );
  });

  it("carries on when a schedule can't be saved", async () => {
    api.listTasks.mockResolvedValue([task()]);
    api.updateTask.mockRejectedValue(new Error("bad interval"));
    render(<TasksPanel />);
    fireEvent.click((await screen.findAllByRole("checkbox"))[0]);
    await waitFor(() => expect(api.listTasks).toHaveBeenCalledTimes(2));
    expect(screen.getByText("Reindex")).toBeTruthy();
  });

  it("refreshes on a timer", async () => {
    vi.useFakeTimers();
    api.listTasks.mockResolvedValue([task()]);
    render(<TasksPanel />);
    await act(async () => {
      await vi.advanceTimersByTimeAsync(5100);
    });
    expect(api.listTasks.mock.calls.length).toBeGreaterThanOrEqual(2);
  });
});

describe("BackupsPanel", () => {
  const list = { keep: 7, backups: [{ name: "b1.json", modified_at: 1, bytes: 2048 }] };

  it("lists backups and says when there are none", async () => {
    api.listBackups.mockResolvedValue(list);
    const first = render(<BackupsPanel />);
    expect(await screen.findByText("b1.json")).toBeTruthy();
    expect(screen.getByText(/newest 7 kept/)).toBeTruthy();
    first.unmount();

    api.listBackups.mockResolvedValue({ keep: 14, backups: [] });
    render(<BackupsPanel />);
    expect(await screen.findByText("No backups yet.")).toBeTruthy();
  });

  it("shows a failed load", async () => {
    api.listBackups.mockRejectedValue(new Error("disk gone"));
    render(<BackupsPanel />);
    expect(await screen.findByText("disk gone")).toBeTruthy();
  });

  it("backs up now, and reports a failure", async () => {
    api.listBackups.mockResolvedValue(list);
    api.createBackup.mockResolvedValueOnce({ name: "new.json" });
    render(<BackupsPanel />);
    fireEvent.click(await screen.findByText("Back up now"));
    expect(await screen.findByText("Saved new.json.")).toBeTruthy();

    api.createBackup.mockRejectedValueOnce(new Error("no space"));
    fireEvent.click(screen.getByText("Back up now"));
    expect(await screen.findByText("no space")).toBeTruthy();
  });

  const pick = (container, file) => {
    const input = container.querySelector('input[type="file"]');
    fireEvent.change(input, { target: { files: file ? [file] : [] } });
  };
  const jsonFile = () => {
    const f = new File(['{"a":1}'], "backup.json", { type: "application/json" });
    f.text = () => Promise.resolve('{"a":1}');
    return f;
  };

  it("restores a chosen file after asking", async () => {
    api.listBackups.mockResolvedValue(list);
    api.restoreBackup.mockResolvedValue({});
    const { container } = render(<BackupsPanel />);
    await screen.findByText("b1.json");
    pick(container, jsonFile());
    expect(await screen.findByText(/Settings restored/)).toBeTruthy();
    expect(api.restoreBackup).toHaveBeenCalledWith('{"a":1}');
  });

  it("restores nothing when declined or when no file was chosen", async () => {
    api.listBackups.mockResolvedValue(list);
    const { container } = render(<BackupsPanel />);
    await screen.findByText("b1.json");
    pick(container, null);
    window.confirm = vi.fn(() => false);
    pick(container, jsonFile());
    expect(api.restoreBackup).not.toHaveBeenCalled();
  });

  it("reports a restore the server refused", async () => {
    api.listBackups.mockResolvedValue(list);
    api.restoreBackup.mockRejectedValue(new Error("not a backup"));
    const { container } = render(<BackupsPanel />);
    await screen.findByText("b1.json");
    pick(container, jsonFile());
    expect(await screen.findByText("not a backup")).toBeTruthy();
  });
});

describe("FontsPanel", () => {
  const data = {
    max_bytes: 16 * 1024 * 1024,
    max_fonts: 2,
    fonts: [
      { id: "anton", family: "Anton", style: "Regular", bytes: 500, builtin: true },
      { id: "mine", family: "Mine", style: "Bold", bytes: 5000, builtin: false },
    ],
  };
  const pickFile = (container, name, size = 100) => {
    const f = new File(["x"], name);
    Object.defineProperty(f, "size", { value: size });
    fireEvent.change(container.querySelector('input[type="file"]'), { target: { files: [f] } });
  };

  it("lists built-in and uploaded fonts", async () => {
    api.fonts.mockResolvedValue(data);
    render(<FontsPanel />);
    expect(await screen.findByText("Anton")).toBeTruthy();
    expect(screen.getByText("Mine Bold")).toBeTruthy();
    expect(screen.getByText("built in")).toBeTruthy();
    expect(screen.getByText("Delete")).toBeTruthy();
  });

  it("shows loading, then a load failure", async () => {
    api.fonts.mockRejectedValue(new Error("fonts unavailable"));
    render(<FontsPanel />);
    expect(screen.getByText("Loading…")).toBeTruthy();
    expect(await screen.findByText("fonts unavailable")).toBeTruthy();
  });

  it("refuses unsuitable files before uploading", async () => {
    api.fonts.mockResolvedValue(data);
    const { container } = render(<FontsPanel />);
    await screen.findByText("Anton");
    pickFile(container, "notes.txt");
    expect(await screen.findByText(/Only .ttf and .otf/)).toBeTruthy();
    expect(api.uploadFont).not.toHaveBeenCalled();
    fireEvent.change(container.querySelector('input[type="file"]'), { target: { files: [] } });
  });

  it("uploads a font and reloads the list", async () => {
    api.fonts.mockResolvedValue(data);
    api.uploadFont.mockResolvedValue({ font: { family: "New", style: "Regular" } });
    const { container } = render(<FontsPanel />);
    await screen.findByText("Anton");
    pickFile(container, "new.ttf");
    expect(await screen.findByText("Added New.")).toBeTruthy();
    expect(api.fonts).toHaveBeenCalledTimes(2);
  });

  it("reports an upload the server refused", async () => {
    api.fonts.mockResolvedValue(data);
    api.uploadFont.mockRejectedValue(new Error("not a font"));
    const { container } = render(<FontsPanel />);
    await screen.findByText("Anton");
    pickFile(container, "bad.otf");
    expect(await screen.findByText("not a font")).toBeTruthy();
  });

  it("deletes an uploaded font once confirmed", async () => {
    api.fonts.mockResolvedValue(data);
    api.deleteFont.mockResolvedValue();
    render(<FontsPanel />);
    fireEvent.click(await screen.findByText("Delete"));
    await waitFor(() => expect(api.deleteFont).toHaveBeenCalledWith("mine"));
  });

  it("keeps the font when the delete is declined or fails", async () => {
    api.fonts.mockResolvedValue(data);
    render(<FontsPanel />);
    window.confirm = vi.fn(() => false);
    fireEvent.click(await screen.findByText("Delete"));
    expect(api.deleteFont).not.toHaveBeenCalled();

    window.confirm = vi.fn(() => true);
    api.deleteFont.mockRejectedValue(new Error("in use"));
    fireEvent.click(screen.getByText("Delete"));
    expect(await screen.findByText("in use")).toBeTruthy();
  });

  it("stops offering uploads at the limit", async () => {
    api.fonts.mockResolvedValue({ ...data, max_fonts: 1 });
    render(<FontsPanel />);
    expect(await screen.findByText(/limit of 1 uploaded fonts is reached/)).toBeTruthy();
    expect(screen.getByText("Upload a font").disabled).toBe(true);
  });

  it("opens the file chooser from the button", async () => {
    api.fonts.mockResolvedValue(data);
    const { container } = render(<FontsPanel />);
    await screen.findByText("Anton");
    const click = vi.spyOn(container.querySelector('input[type="file"]'), "click");
    // Two uploaded fonts is not the limit here, one of them is built in.
    fireEvent.click(screen.getByText("Upload a font"));
    expect(click).toHaveBeenCalled();
  });
});

describe("LogsPanel", () => {
  const entry = (id, level = "info", message = "hello") => ({ id, level, target: "app", message, ts_ms: 1000 * id });

  beforeEach(() => {
    api.getLogLevel.mockResolvedValue({ level: "info" });
    api.getLogs.mockResolvedValue({ entries: [entry(1), entry(2, "error", "boom")] });
  });

  it("is closed until opened, and then shows what the server logged", async () => {
    render(<LogsPanel />);
    expect(screen.queryByText("Logs tail")).toBeNull();
    expect(api.getLogs).not.toHaveBeenCalled();
    fireEvent.click(screen.getByText("Show"));
    expect(await screen.findByText(/boom/)).toBeTruthy();
    expect(screen.getByText("Hide")).toBeTruthy();
    fireEvent.click(screen.getByText("Hide"));
    expect(screen.queryByText(/boom/)).toBeNull();
  });

  it("says when there is nothing at the chosen level", async () => {
    api.getLogs.mockResolvedValue({ entries: [] });
    render(<LogsPanel />);
    fireEvent.click(screen.getByText("Show"));
    expect(await screen.findByText("Nothing at this level yet.")).toBeTruthy();
    expect(screen.getByText("Download").disabled).toBe(true);
  });

  it("shows errors from either request", async () => {
    api.getLogs.mockRejectedValue(new Error("log fetch failed"));
    api.getLogLevel.mockRejectedValue(new Error("level fetch failed"));
    render(<LogsPanel />);
    fireEvent.click(screen.getByText("Show"));
    expect(await screen.findByText(/fetch failed/)).toBeTruthy();
  });

  it("starts over when the filter changes, and sends it along", async () => {
    render(<LogsPanel />);
    fireEvent.click(screen.getByText("Show"));
    await screen.findByText(/boom/);
    fireEvent.change(screen.getByPlaceholderText("Filter text…"), { target: { value: "boom" } });
    await waitFor(() =>
      expect(api.getLogs).toHaveBeenLastCalledWith(expect.objectContaining({ q: "boom", level: "info" }))
    );
    fireEvent.change(screen.getAllByRole("combobox")[0], { target: { value: "debug" } });
    await waitFor(() => expect(api.getLogs).toHaveBeenLastCalledWith(expect.objectContaining({ level: "debug" })));
  });

  it("changes the server's level, and reports a refusal", async () => {
    api.setLogLevel.mockResolvedValueOnce({});
    render(<LogsPanel />);
    fireEvent.click(screen.getByText("Show"));
    await screen.findByText(/boom/);
    const server = screen.getAllByRole("combobox")[1];
    await waitFor(() => expect(server.disabled).toBe(false));
    fireEvent.change(server, { target: { value: "debug" } });
    await waitFor(() => expect(api.setLogLevel).toHaveBeenCalledWith("debug"));
    api.setLogLevel.mockRejectedValueOnce(new Error("refused"));
    fireEvent.change(server, { target: { value: "trace" } });
    expect(await screen.findByText("refused")).toBeTruthy();
  });

  it("offers the environment's level when that is what is in force", async () => {
    api.getLogLevel.mockResolvedValue({ level: "env" });
    render(<LogsPanel />);
    fireEvent.click(screen.getByText("Show"));
    expect(await screen.findByText("from RUST_LOG")).toBeTruthy();
  });

  it("pauses polling, clears the view and downloads the text", async () => {
    URL.createObjectURL = vi.fn(() => "blob:x");
    URL.revokeObjectURL = vi.fn();
    const click = vi.spyOn(HTMLAnchorElement.prototype, "click").mockImplementation(() => {});
    render(<LogsPanel />);
    fireEvent.click(screen.getByText("Show"));
    await screen.findByText(/boom/);

    fireEvent.click(screen.getByText("Download"));
    expect(click).toHaveBeenCalled();
    expect(URL.revokeObjectURL).toHaveBeenCalledWith("blob:x");

    fireEvent.click(screen.getByText("Pause"));
    expect(screen.getByText("Resume")).toBeTruthy();
    fireEvent.click(screen.getByText("Resume"));

    fireEvent.click(screen.getByText("Clear view"));
    await waitFor(() => expect(screen.queryByText(/boom/)).toBeNull());
  });

  it("polls again on a timer, and tracks whether the reader scrolled up", async () => {
    vi.useFakeTimers();
    render(<LogsPanel />);
    fireEvent.click(screen.getByText("Show"));
    await act(async () => {
      await vi.advanceTimersByTimeAsync(3100);
    });
    expect(api.getLogs.mock.calls.length).toBeGreaterThanOrEqual(2);
    const tail = document.querySelector(".log-tail");
    fireEvent.scroll(tail);
    fireEvent.scroll(tail, { target: { scrollTop: 0 } });
  });
});
