// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { api } from "./api.js";
import { NavidromeSettingsPanel } from "./NavidromeSettingsPanel.jsx";

afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
});

const settings = (over = {}) => ({ configured: false, url: null, user: null, password_set: false, ...over });

describe("Settings -> Navidrome", () => {
  it("offers the address, user and password, with nothing to test or remove yet", async () => {
    vi.spyOn(api, "getNavidromeSettings").mockResolvedValue(settings());
    render(<NavidromeSettingsPanel />);
    expect(screen.getByText("Navidrome address")).toBeTruthy();
    expect(screen.getByText("User name")).toBeTruthy();
    expect(screen.getByText("Password")).toBeTruthy();
    expect(screen.queryByText("Test")).toBeNull();
    expect(screen.queryByText("Remove")).toBeNull();
    // Save waits for an address and a user
    expect(screen.getByText("Save").disabled).toBe(true);
  });

  it("saves, never shows the password back, and says it connected", async () => {
    vi.spyOn(api, "getNavidromeSettings").mockResolvedValue(settings());
    const put = vi.spyOn(api, "putNavidromeSettings").mockResolvedValue(
      settings({ configured: true, url: "http://navi:4533", user: "me", password_set: true, check: { ok: true, version: "0.52.5" } })
    );
    render(<NavidromeSettingsPanel />);
    const boxes = document.querySelectorAll("input");
    fireEvent.change(boxes[0], { target: { value: "http://navi:4533" } });
    fireEvent.change(boxes[1], { target: { value: "me" } });
    fireEvent.change(boxes[2], { target: { value: "sesame" } });
    fireEvent.click(screen.getByText("Save"));
    await waitFor(() => expect(screen.getByRole("status").textContent).toContain("Connected to Navidrome 0.52.5."));
    expect(put).toHaveBeenCalledWith({ url: "http://navi:4533", user: "me", password: "sesame" });
    expect(boxes[2].value).toBe("");
    expect(boxes[2].placeholder).toContain("saved");
    expect(screen.getByText("Test")).toBeTruthy();
  });

  it("keeps the saved password when none is typed, and shows why a connection failed", async () => {
    vi.spyOn(api, "getNavidromeSettings").mockResolvedValue(settings({ configured: true, url: "http://navi:4533", user: "me", password_set: true }));
    const put = vi.spyOn(api, "putNavidromeSettings").mockResolvedValue(
      settings({ configured: true, url: "http://navi:4533", user: "me", password_set: true, check: { ok: false, error: "Wrong username or password" } })
    );
    render(<NavidromeSettingsPanel />);
    await waitFor(() => expect(document.querySelectorAll("input")[0].value).toBe("http://navi:4533"));
    fireEvent.click(screen.getByText("Save"));
    await waitFor(() => expect(screen.getByRole("status").textContent).toContain("Wrong username or password"));
    expect(put.mock.calls[0][0].password).toBeUndefined();
  });
});
