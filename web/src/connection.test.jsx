import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import { ConnectionPanel, connectionRows } from "./ConnectionPanel.jsx";

const base = {
  https: true,
  host: "memes.example.com",
  prefix: "/apps",
  app_url: "https://memes.example.com/apps",
  oidc_callback: "https://memes.example.com/apps/api/auth/oidc/callback",
  plex_callback: "https://memes.example.com/apps/api/auth/plex/callback",
  peer: "172.18.0.5",
  client_ip: "198.51.100.7",
  forwarded_trusted: true,
  public_url: false,
};

describe("connectionRows", () => {
  const rows = (c) => Object.fromEntries(connectionRows(c));
  it("describes a connection through an HTTPS proxy", () => {
    const r = rows(base);
    expect(r["Connection"]).toBe("HTTPS");
    expect(r["This app's address"]).toBe("https://memes.example.com/apps");
    expect(r["Client address (for login limits)"]).toBe("198.51.100.7");
    expect(r["Forwarding headers"]).toContain("believed");
  });
  it("says when forwarding headers are ignored or PUBLIC_URL is used", () => {
    expect(rows({ ...base, forwarded_trusted: false })["Forwarding headers"]).toContain("ignored");
    expect(rows({ ...base, public_url: true })["Forwarding headers"]).toContain("PUBLIC_URL");
  });
  it("copes with an address it could not work out", () => {
    const r = rows({ ...base, https: false, host: null, prefix: "", app_url: null });
    expect(r["Connection"]).toBe("plain HTTP");
    expect(r["Address the browser used"]).toBe("unknown");
    expect(r["Path prefix added by a proxy"]).toBe("none");
  });
});

describe("ConnectionPanel", () => {
  it("offers a Check button before anything is shown", () => {
    const html = renderToStaticMarkup(<ConnectionPanel />);
    expect(html).toContain("Check this connection");
    expect(html).toContain(">Check<");
    expect(html).not.toContain("Nothing looks wrong");
  });
});
