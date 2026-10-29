// Thin wrapper over the backend JSON API.
import { withBase } from "./base.js";
import { libraryParams } from "./libraryFilter.js";

async function json(url, opts) {
  const res = await fetch(withBase(url), opts);
  if (!res.ok) {
    let msg = res.statusText;
    try {
      const body = await res.json();
      if (body.error) msg = body.error;
    } catch (_) {
      /* ignore */
    }
    // A session that expired (or was never there) mid-use -- let the app
    // shell know so it can drop back to the login screen, rather than every
    // caller having to special-case 401 itself.
    if (res.status === 401 && !url.startsWith("/api/auth/")) {
      window.dispatchEvent(new Event("memearr:unauthorized"));
    }
    throw new Error(msg);
  }
  return res.json();
}

// Fetch a binary (video/image) URL, surfacing the backend's actual error text
// instead of letting the browser's <video>/<img> element fail silently with a
// generic "unsupported format" message. On success, returns an object URL —
// caller is responsible for revoking it (URL.revokeObjectURL) when done.
async function fetchBlobUrl(url, signal) {
  const res = await fetch(withBase(url), { signal });
  if (!res.ok) {
    let msg = res.statusText;
    try {
      const body = await res.json();
      if (body.error) msg = body.error;
    } catch (_) {
      /* ignore */
    }
    throw new Error(msg);
  }
  const blob = await res.blob();
  if (blob.size === 0) throw new Error("The server sent an empty preview. Try Reload preview.");
  return URL.createObjectURL(blob);
}

export const api = {
  authStatus: () => json(`/api/auth/status`),
  authSetup: (password) =>
    json(`/api/auth/setup`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ password }),
    }),
  authLogin: (password) =>
    json(`/api/auth/login`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ password }),
    }),
  authLogout: () => json(`/api/auth/logout`, { method: "POST" }),
  authChangePassword: (current_password, new_password) =>
    json(`/api/auth/change-password`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ current_password, new_password }),
    }),
  getAuthentikSettings: () => json(`/api/settings/authentik`),
  putAuthentikSettings: (payload) =>
    json(`/api/settings/authentik`, {
      method: "PUT",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify(payload),
    }),
  deleteAuthentikSettings: () => json(`/api/settings/authentik`, { method: "DELETE" }),
  getPlexLoginSettings: () => json(`/api/settings/plex-login`),
  deletePlexLoginSettings: () => json(`/api/settings/plex-login`, { method: "DELETE" }),
  // `libraries`: names to limit the search to (empty = all); `offset`: how many
  // matches to skip, for "show more". Replies carry `has_more`.
  searchDialogue: (q, { libraries = [], offset = 0, limit = 50 } = {}) =>
    json(`/api/search?q=${encodeURIComponent(q)}&offset=${offset}&limit=${limit}${libraryParams(libraries)}`),
  searchTitle: (q, { libraries = [], offset = 0, limit = 50 } = {}) =>
    json(`/api/search/title?q=${encodeURIComponent(q)}&offset=${offset}&limit=${limit}${libraryParams(libraries)}`),
  media: (ratingKey) => json(`/api/media/${encodeURIComponent(ratingKey)}`),
  mediaSubtitles: (ratingKey, start, end) =>
    json(`/api/media/${encodeURIComponent(ratingKey)}/subtitles?start=${start}&end=${end}`),
  fetchBlobUrl,
  indexStatus: () => json(`/api/index/status`),
  reindex: () => json(`/api/index/reindex`, { method: "POST" }),
  listTasks: () => json(`/api/tasks`),
  getLogs: ({ level, q, after }) => {
    const p = new URLSearchParams({ level, q: q || "" });
    if (after != null) p.set("after", String(after));
    return json(`/api/logs?${p}`);
  },
  getLogLevel: () => json(`/api/settings/log-level`),
  setLogLevel: (level) =>
    json(`/api/settings/log-level`, {
      method: "PUT",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ level }),
    }),
  browseDirs: (path) => json(`/api/system/browse${path ? `?path=${encodeURIComponent(path)}` : ""}`),
  getTrustedProxies: () => json(`/api/settings/trusted-proxies`),
  setTrustedProxies: (trusted_proxies) =>
    json(`/api/settings/trusted-proxies`, {
      method: "PUT",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ trusted_proxies }),
    }),
  apiKeyStatus: () => json(`/api/settings/api-key`),
  generateApiKey: () => json(`/api/settings/api-key`, { method: "POST" }),
  revokeApiKey: () => json(`/api/settings/api-key`, { method: "DELETE" }),
  listBackups: () => json(`/api/backups`),
  createBackup: () => json(`/api/backups`, { method: "POST" }),
  restoreBackup: (text) =>
    json(`/api/backups/restore`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: text,
    }),
  health: () => json(`/api/health`),
  recheckHealth: () => json(`/api/health/check`, { method: "POST" }),
  runTask: (id) => json(`/api/tasks/${encodeURIComponent(id)}/run`, { method: "POST" }),
  updateTask: (id, schedule) =>
    json(`/api/tasks/${encodeURIComponent(id)}`, {
      method: "PUT",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify(schedule),
    }),
  wanted: (q = "", libraries = [], offset = 0, limit = 50) =>
    json(`/api/wanted?q=${encodeURIComponent(q)}&offset=${offset}&limit=${limit}${libraryParams(libraries)}`),
  reindexItem: (id) => json(`/api/media/${encodeURIComponent(id)}/reindex`, { method: "POST" }),
  subtitleCapability: () => json(`/api/subtitles/capability`),
  subtitleSearch: (id, language) =>
    json(`/api/media/${encodeURIComponent(id)}/subtitles/search?language=${encodeURIComponent(language)}`),
  // `meta` ({title, provider, language}) only describes the request in the
  // Requests list; the key is what's downloaded.
  subtitleDownload: (id, key, meta = {}) =>
    json(`/api/media/${encodeURIComponent(id)}/subtitles/download`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ key, ...meta }),
    }),
  subtitleDiagnose: (id) => json(`/api/media/${encodeURIComponent(id)}/subtitles/diagnose`),
  subtitleRequests: () => json(`/api/subtitles/requests`),
  deleteSubtitleRequest: (id) => json(`/api/subtitles/requests/${encodeURIComponent(id)}`, { method: "DELETE" }),
  clearFinishedSubtitleRequests: () => json(`/api/subtitles/requests?finished=true`, { method: "DELETE" }),
  version: () => json(`/api/version`),
  libraryStats: () => json(`/api/libraries/stats`),
  activityLog: (limit = 200) => json(`/api/activity/log?limit=${limit}`),
  clearActivityLog: () => json(`/api/activity/log`, { method: "DELETE" }),
  systemStorage: () => json(`/api/system/storage`),
  listExports: () => json(`/api/exports`),
  vacuumDatabase: () => json(`/api/system/vacuum`, { method: "POST" }),
  clearPreviewCache: () => json(`/api/system/clear-preview-cache`, { method: "POST" }),
  clearExports: () => json(`/api/system/clear-exports`, { method: "POST" }),
  libraryMissing: (library) =>
    json(`/api/libraries/missing?library=${encodeURIComponent(library)}`),
  libraryBrowse: (q, offset = 0, limit = 60, libraries = [], status = "all") =>
    json(
      `/api/library/browse?q=${encodeURIComponent(q)}&offset=${offset}&limit=${limit}` +
        `&status=${encodeURIComponent(status)}` +
        libraryParams(libraries)
    ),
  showEpisodes: (show) =>
    json(`/api/library/episodes?show=${encodeURIComponent(show)}`),
  startExport: (payload) =>
    json(`/api/export`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify(payload),
    }),
  startSequenceExport: (payload) =>
    json(`/api/sequence/export`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify(payload),
    }),
  job: (id) => json(`/api/jobs/${encodeURIComponent(id)}`),
  getSettings: () => json(`/api/settings`),
  saveSettings: (payload) =>
    json(`/api/settings`, {
      method: "PUT",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify(payload),
    }),
  deleteSettings: () => json(`/api/settings`, { method: "DELETE" }),
  testSettings: (payload) =>
    json(`/api/settings/test`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify(payload || {}),
    }),
  fonts: () => json(`/api/fonts`),
  uploadFont: (file) =>
    json(`/api/fonts?name=${encodeURIComponent(file.name)}`, {
      method: "POST",
      headers: { "Content-Type": "application/octet-stream" },
      body: file,
    }),
  deleteFont: async (id) => {
    const res = await fetch(withBase(`/api/fonts/${encodeURIComponent(id)}`), { method: "DELETE" });
    if (res.status === 401) window.dispatchEvent(new Event("memearr:unauthorized"));
    if (!res.ok) {
      let msg = res.statusText;
      try {
        const body = await res.json();
        if (body.error) msg = body.error;
      } catch (_) {
        /* ignore */
      }
      throw new Error(msg);
    }
  },
  previewUrl: (
    ratingKey,
    { start, end, width, fps, captionMode, top, bottom, fontSize, font, fontScale, offsetMs, editedCues }
  ) => {
    const params = new URLSearchParams({ start, end, width });
    if (fps != null) params.set("fps", fps);
    if (captionMode) params.set("caption_mode", captionMode);
    if (top) params.set("top", top);
    if (bottom) params.set("bottom", bottom);
    if (fontSize != null) params.set("font_size", fontSize);
    if (font) params.set("font", font);
    if (fontScale && fontScale !== 100) params.set("font_scale", fontScale);
    if (offsetMs) params.set("offset_ms", offsetMs);
    if (editedCues) params.set("cues_json", JSON.stringify(editedCues));
    return `/api/preview/${encodeURIComponent(ratingKey)}?${params.toString()}`;
  },
};
