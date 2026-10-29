#!/usr/bin/env node
/**
 * Build the mock library, run meme-arr against a pretend Plex, and take the
 * screenshots used in the README and docs.
 *
 *   node dev/mock/screenshots.cjs [--out docs/images]
 *
 * Needs: python, ffmpeg/ffprobe on PATH, a built binary (cargo build, or APP_BIN)
 * and built frontend (cd web && npm run build, or WEB_DIR), and the dev dependency
 * `npm i --no-save playwright-core` (any folder on NODE_PATH works) plus a
 * Chromium-based browser: set BROWSER to its executable (Edge, Chrome, Chromium).
 *
 * Environment (all optional): APP_BIN, WEB_DIR, BROWSER, APP_PORT (18090),
 * MOCK_PORT (32400), DEMO_PASSWORD (demo-password-123).
 */
const { spawn, spawnSync } = require("child_process");
const fs = require("fs");
const os = require("os");
const path = require("path");

const root = path.resolve(__dirname, "..", "..");
const outDir = path.resolve(root, (process.argv.includes("--out") ? process.argv[process.argv.indexOf("--out") + 1] : "docs/images"));
const mediaDir = path.join(__dirname, "media");
const APP_PORT = process.env.APP_PORT || "18090";
const MOCK_PORT = process.env.MOCK_PORT || "32400";
const PASSWORD = process.env.DEMO_PASSWORD || "demo-password-123";
const BASE = `http://127.0.0.1:${APP_PORT}`;
const exe = process.platform === "win32" ? ".exe" : "";
// The newest build of the app (release or debug), unless APP_BIN says otherwise.
const APP_BIN = process.env.APP_BIN || ["release", "debug"]
  .map((d) => path.join(root, "target", d, "meme-arr" + exe))
  .filter(fs.existsSync)
  .sort((a, b) => fs.statSync(b).mtimeMs - fs.statSync(a).mtimeMs)[0];
const WEB_DIR = process.env.WEB_DIR || path.join(root, "web", "dist");
const BROWSER = process.env.BROWSER || ["C:/Program Files (x86)/Microsoft/Edge/Application/msedge.exe", "C:/Program Files/Google/Chrome/Application/chrome.exe", "/usr/bin/chromium", "/usr/bin/google-chrome"].find(fs.existsSync);

const children = [];
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
function die(msg) { console.error("error: " + msg); cleanup(); process.exit(1); }
function cleanup() { for (const c of children) { try { c.kill(); } catch (_) { /* gone */ } } }
process.on("exit", cleanup);

async function api(method, url, body, cookie) {
  const res = await fetch(BASE + url, { method, headers: { "Content-Type": "application/json", ...(cookie ? { Cookie: cookie } : {}) }, body: body ? JSON.stringify(body) : undefined });
  const text = await res.text();
  let json = null; try { json = JSON.parse(text); } catch (_) { /* not json */ }
  return { status: res.status, json, text, headers: res.headers };
}

/**
 * The screenshots are of a demo, not of this machine: show Docker-style paths
 * instead of wherever the mock media happens to live, and leave out the two
 * health warnings that are about this dev box (the GitHub update check can't see a
 * private repo; gifsicle isn't installed here). Applied to every page in a context.
 */
async function demoPolish(context) {
  await context.route("**/api/version", async (route) => {
    const res = await route.fetch();
    const j = await res.json();
    await route.fulfill({ response: res, json: { ...j, checked: true, error: null, latest: j.current, update_available: false } });
  });
  await context.route(/\/api\/health(\/check)?$/, async (route) => {
    const res = await route.fetch();
    const j = await res.json();
    if (Array.isArray(j.issues)) j.issues = j.issues.filter((i) => !["gifsicle_missing", "update_check_failed"].includes(i.kind));
    await route.fulfill({ response: res, json: j });
  });
}

/** Swap each [from, to] pair in everything the page shows (the local media folder becomes /media, ...). */
async function scrubPaths(page, pairs) {
  await page.evaluate((pairs) => {
    const needles = pairs.map((p) => p[0]);
    const fix = (t) => pairs.reduce((acc, [from, to]) => acc.split(from).join(to), t);
    // the Tasks table remembers the health check's own count, from before the two dev-box warnings were left out
    for (const el of document.querySelectorAll("*")) if (el.children.length === 0 && /^0 error\(s\), \d+ warning\(s\)$/.test(el.textContent.trim())) el.textContent = "no problems found";
    const walker = document.createTreeWalker(document.body, NodeFilter.SHOW_TEXT);
    for (let n = walker.nextNode(); n; n = walker.nextNode()) if (needles.some((x) => n.nodeValue.includes(x))) n.nodeValue = fix(n.nodeValue);
    for (const el of document.querySelectorAll("input, textarea")) if (needles.some((x) => (el.value || "").includes(x))) el.value = fix(el.value);
  }, pairs);
}

async function main() {
  if (!APP_BIN) die("no app binary: run `cargo build` or set APP_BIN");
  console.log("app: " + APP_BIN);
  if (!fs.existsSync(path.join(WEB_DIR, "index.html"))) die(`no built frontend at ${WEB_DIR}: run \`npm run build\` in web/ or set WEB_DIR`);
  if (!BROWSER) die("set BROWSER to a Chromium-based browser executable");
  let chromium;
  try { ({ chromium } = require("playwright-core")); } catch (_) { die("playwright-core isn't installed: `npm i --no-save playwright-core`"); }

  // 1. the mock library on disk
  const py = process.platform === "win32" ? "python" : "python3";
  console.log("building the mock library…");
  const gen = spawnSync(py, [path.join(__dirname, "gen_media.py"), "--out", mediaDir], { stdio: "inherit" });
  if (gen.status !== 0) die("gen_media.py failed (is ffmpeg on PATH?)");

  // 2. a pretend Plex
  const mock = spawn(py, [path.join(__dirname, "mock_plex.py"), "--port", MOCK_PORT, "--manifest", path.join(mediaDir, "manifest.json")], { stdio: "inherit" });
  children.push(mock);

  // 3. the app, on empty data
  const data = fs.mkdtempSync(path.join(os.tmpdir(), "memearr-demo-data-"));
  const output = fs.mkdtempSync(path.join(os.tmpdir(), "memearr-demo-out-"));
  const app = spawn(APP_BIN, [], {
    stdio: "ignore",
    env: { ...process.env, APP_VERSION: process.env.DEMO_VERSION || "v0.20.0", BIND_ADDR: `127.0.0.1:${APP_PORT}`, DATA_DIR: data, OUTPUT_DIR: output, WEB_DIR, FONTS_DIR: path.join(root, "assets", "fonts"), INDEX_ON_START: "false", RUST_LOG: "warn" },
  });
  children.push(app);
  for (let i = 0; i < 80; i++) {
    try { if ((await fetch(BASE + "/healthz")).ok) break; } catch (_) { /* not up yet */ }
    if (i === 79) die("the app didn't start");
    await sleep(250);
  }

  // 4. a brand-new install, set up through the browser like a real first run
  fs.mkdirSync(outDir, { recursive: true });
  const abs = mediaDir.replace(/\\/g, "/");
  const browser = await chromium.launch({ executablePath: BROWSER, headless: true });
  const ctx = await browser.newContext({ viewport: { width: 1280, height: 800 }, colorScheme: "light" });
  await demoPolish(ctx);
  const page = await ctx.newPage();
  // what the pictures should say instead of this machine's paths and loopback address
  const pairs = [[abs, "/media"], [abs.replace(/\//g, "\\"), "/media"], [`http://127.0.0.1:${MOCK_PORT}`, "http://192.168.1.10:32400"], [`http://127.0.0.1:${APP_PORT}`, "http://192.168.1.20:8089"]];
  const shot = async (name, opts = {}) => {
    await page.waitForTimeout(500);
    await scrubPaths(page, pairs);
    await page.screenshot({ path: path.join(outDir, name + ".png"), ...opts });
    console.log("  " + name + ".png");
  };
  // (the search box is cleared on every other tab so old queries don't linger in the pictures)
  const nav = async (label) => { await page.click(`.navitem:has(.nlabel:text-is("${label}"))`); await page.fill("form.search input", ""); };
  const search = async (q, mode) => {
    if (mode) await page.click(`form.search .seg button:has-text("${mode}")`);
    await page.fill("form.search input", q);
    await page.click('form.search button[type="submit"]');
    await page.waitForTimeout(900);
  };

  console.log("screenshots:");
  console.log(" first run:");
  await page.goto(BASE + "/");
  await page.waitForSelector('input[type="password"]');
  await shot("setup-password");
  const pw = page.locator('input[type="password"]');
  await pw.nth(0).fill(PASSWORD);
  await pw.nth(1).fill(PASSWORD);
  await page.click('button[type="submit"]');
  await page.waitForSelector(".wizard");
  await shot("wizard-connect");
  await page.selectOption(".wizard select", "jellyfin");
  await shot("wizard-jellyfin");
  await page.selectOption(".wizard select", "plex");
  await page.fill('.wizard input[placeholder="http://192.168.1.10:32400"]', `http://127.0.0.1:${MOCK_PORT}`);
  await page.fill('.wizard input[type="password"]', "demo-token");
  await page.click('.wizard button:has-text("Test connection")');
  await page.waitForSelector(".wizard .good");
  await shot("wizard-connected");
  await page.click('.wizard button:has-text("Next")');
  await page.waitForSelector(".wizard-table");
  for (const row of await page.locator(".wizard-table tbody tr").all()) {
    const lib = (await row.locator("td").first().textContent()).trim();
    await row.locator("input").fill(`${abs}/${lib}`);
  }
  await shot("wizard-mapping");
  await page.click('.wizard button:has-text("Save & check")');
  await page.waitForSelector(".wizard .good:has-text('Saved')", { timeout: 20000 }).catch(async () => {
    die("the wizard didn't save; it says: " + ((await page.locator(".wizard").innerText()).replace(/\s+/g, " ").slice(0, 400)));
  });
  await shot("wizard-done");
  await page.click('.wizard button:has-text("Start indexing")');
  await page.waitForSelector("form.search");
  const cookies = await ctx.cookies();
  const cookie = cookies.map((c) => `${c.name}=${c.value}`).join("; ");
  for (let i = 0; i < 120; i++) {
    await sleep(500);
    const st = (await api("GET", "/api/index/status", null, cookie)).json;
    if (st && st.status && !st.status.running && st.status.finished_at) { console.log(`   indexed ${st.status.done} items, ${st.status.errors} errors`); break; }
  }
  await sleep(1500);
  await page.reload();
  await page.waitForSelector("form.search");
  await page.waitForTimeout(1200);
  await shot("first-index");

  console.log(" tour:");
  // search
  await search("soup");
  await page.waitForSelector("button.result");
  await shot("search-dialogue");
  // filter chips: narrow to a show first, then search the dialogue inside it
  await page.click(".addfilter-btn");
  await page.fill('.addfilter-pop input:not([type="checkbox"])', "Orbital Diner");
  await shot("search-add-filter");
  await page.click('.addfilter-pop button[type="submit"]');
  await page.waitForTimeout(900);
  await search("soup");
  await page.waitForSelector("button.result");
  await shot("search-filter-chips");
  await page.click(".scope-chip-x");
  await page.waitForTimeout(900);
  await search('soup show:"Orbital Diner" -ep:docking');
  await page.waitForSelector(".search-understood");
  await shot("search-operators");
  await page.click(".search-tips-btn");
  await shot("search-tips");
  await page.keyboard.press("Escape");
  await search("soup");
  await page.click(".search-scope .libfilter-btn");
  await page.locator('.libfilter-pop label:has-text("TV Shows") input').check();
  await page.locator('.libfilter-pop label:has-text("Cartoons") input').check();
  await page.waitForTimeout(900);
  await shot("search-library-picker");
  await page.locator('.libfilter-pop label:has-text("All libraries") input').check();
  await page.keyboard.press("Escape");
  await search('show:"Lantern Cove" season:2 -ep:gull', "Title");
  await page.waitForSelector("button.result");
  await shot("search-title");
  await page.click('form.search .seg button:has-text("Dialogue")');

  // a font to pick from (a tiny made-up one, dev/mock/fonts)
  await fetch(BASE + "/api/fonts?name=PosterBlock.ttf", {
    method: "POST",
    headers: { cookie, "Content-Type": "application/octet-stream" },
    body: fs.readFileSync(path.join(__dirname, "fonts", "PosterBlock.ttf")),
  });

  // editor
  await search('"zero gravity"');
  await page.waitForSelector("button.result");
  await page.locator("button.result").first().click();
  await page.waitForSelector("video");
  await page.waitForFunction(() => { const v = document.querySelector("video"); return v && v.readyState >= 2; }, null, { timeout: 90000 }).catch(() => {});
  await page.selectOption(".font-controls select", "PosterBlock.ttf");
  await page.locator(".font-controls input[type=range]").evaluate((el) => {
    const set = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value").set;
    set.call(el, "130");
    el.dispatchEvent(new Event("input", { bubbles: true }));
  });
  await shot("editor", { fullPage: true });
  await page.waitForTimeout(300);
  await page.locator(".font-controls").locator("xpath=..").screenshot({ path: path.join(outDir, "editor-fonts.png") });
  console.log("  editor-fonts.png");

  // sequence: three clips from different shows
  for (const q of ['"zero gravity"', '"small fire"', '"tide for you"']) {
    await search(q);
    await page.waitForSelector("button.result");
    await page.locator("button.result").first().click();
    await page.waitForSelector("button:has-text('Add to sequence')");
    await page.click("button:has-text('Add to sequence')");
    await page.waitForTimeout(400);
  }
  await nav("Sequence");
  await page.waitForTimeout(1500);
  await shot("sequence");

  // library, wanted
  await nav("Library");
  await page.waitForTimeout(1500);
  await shot("library");
  await page.click('[aria-label="Subtitle status"] button:has-text("Missing subtitles")');
  await page.waitForTimeout(1200);
  await shot("library-missing");
  await page.click('[aria-label="Subtitle status"] button:has-text("All")');
  await nav("Wanted");
  await page.waitForSelector(".wanted-table");
  await shot("wanted");
  await page.click(".wanted-library .libfilter-btn");
  await page.locator('.wanted-library .libfilter-pop label:has-text("TV Shows") input').check();
  await page.locator('.wanted-library .libfilter-pop label:has-text("Movies") input').check();
  await page.waitForTimeout(900);
  await shot("wanted-library-picker");
  await page.keyboard.press("Escape");
  await page.locator('.wanted-library .libfilter-pop label:has-text("All libraries") input').check().catch(() => {});
  await page.locator("body").click({ position: { x: 5, y: 5 } });

  // online subtitle search for "Tideline" (Plex will keep the download itself)
  await page.fill("input.filter:not(.wanted-library)", "Tideline");
  await page.waitForTimeout(900);
  await page.click('.wanted-table tbody tr:has-text("Tideline") button.secondary');
  await page.waitForSelector(".wanted-search-row .subsearch-form");
  await page.click(".wanted-search-row .subsearch-form button");
  await page.waitForSelector(".wanted-search-row .subsearch-table");
  await shot("subtitle-search");
  await page.click('.wanted-search-row .subsearch-table tbody tr:first-child button:has-text("Download")');
  await page.waitForSelector(".wanted-search-row .good", { timeout: 15000 });
  await shot("subtitle-download");

  // two more requests that never arrive, via the API
  const wanted = (await api("GET", "/api/wanted?q=&limit=50", null, cookie)).json.items;
  for (const title of ["A Study in Beige", "Nap Time Negotiations"]) {
    const it = wanted.find((w) => w.title === title);
    if (it) await api("POST", `/api/media/${it.rating_key}/subtitles/download`, { key: `/library/metadata/${it.rating_key}/subtitles/slow-1`, title: `${title.replace(/ /g, ".")}.720p.HDTV.x264-LOL`, provider: "Podnapisi", language: "en" }, cookie);
  }
  await page.click('.wanted [role=tab]:has-text("Requests")');
  await page.waitForSelector(".subreq-table");
  await page.waitForTimeout(600);
  // Re-index the one Plex kept; diagnose one that's still waiting
  await page.click('.subreq-table tbody tr:has-text("Tideline") button:has-text("Re-index")');
  await page.waitForFunction(() => /Indexed/.test(document.querySelector(".subreq-table")?.textContent || ""), null, { timeout: 15000 }).catch(() => {});
  await page.click('.subreq-table tbody tr:has-text("A Study in Beige") button:has-text("Diagnose")');
  await page.waitForSelector(".diag-findings");
  await page.click('.diag button:has-text("Show the details")').catch(() => {});
  await shot("requests-diagnose", { fullPage: true });

  // system, settings, activity
  await nav("System");
  await page.waitForTimeout(2500);
  await shot("system", { fullPage: true });
  await nav("Settings");
  await page.waitForSelector("text=Library path mappings");
  await page.waitForSelector(".fonts-panel li");
  await shot("settings", { fullPage: true });
  await page.locator(".fonts-panel").screenshot({ path: path.join(outDir, "settings-fonts.png") });
  console.log("  settings-fonts.png");
  await nav("Activity");
  await page.waitForTimeout(1500);
  await shot("activity");

  // dark theme and a phone
  const dark = await browser.newContext({ viewport: { width: 1280, height: 800 }, colorScheme: "dark" });
  await demoPolish(dark);
  const dp = await dark.newPage();
  await dp.goto(BASE + "/");
  await dp.fill('input[type="password"]', PASSWORD);
  await dp.click('button[type="submit"]');
  await dp.waitForSelector("form.search");
  await dp.fill("form.search input", 'bad feeling show:"Lantern Cove"');
  await dp.click('form.search button[type="submit"]');
  await dp.waitForSelector("button.result");
  await dp.waitForTimeout(800);
  await dp.screenshot({ path: path.join(outDir, "search-dark.png") });
  console.log("  search-dark.png");
  const phone = await browser.newContext({ viewport: { width: 390, height: 844 }, colorScheme: "light", deviceScaleFactor: 2 });
  await demoPolish(phone);
  const pp = await phone.newPage();
  await pp.goto(BASE + "/");
  await pp.fill('input[type="password"]', PASSWORD);
  await pp.click('button[type="submit"]');
  await pp.waitForSelector("form.search");
  await pp.fill("form.search input", "door");
  await pp.click('form.search button[type="submit"]');
  await pp.waitForSelector("button.result");
  await pp.waitForTimeout(800);
  await pp.screenshot({ path: path.join(outDir, "search-phone.png") });
  console.log("  search-phone.png");

  await browser.close();
  console.log(`done: ${fs.readdirSync(outDir).filter((f) => f.endsWith(".png")).length} images in ${outDir}`);
}

main().then(() => { cleanup(); process.exit(0); }, (e) => { console.error(e); cleanup(); process.exit(1); });
