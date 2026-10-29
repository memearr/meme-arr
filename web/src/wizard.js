// Pure helpers for the first-run setup wizard.

/** The last folder name of a Plex location; a bare Windows drive ("D:\") -> "d". */
export function locationLabel(location) {
  const drive = /^([A-Za-z]):[\\/]*$/.exec(location.trim());
  if (drive) return drive[1].toLowerCase();
  const parts = location.split(/[\\/]+/).filter(Boolean);
  return parts.length ? parts[parts.length - 1] : "";
}

/**
 * Guess which mounted folder holds a Plex location: the one whose name
 * matches its last folder (case-insensitively). For a bare drive "D:\" also
 * accepts "d" / "drive-d" / "drived". Returns "" when nothing matches
 * unambiguously -- better blank than a wrong guess the user doesn't notice.
 */
export function suggestContainerPath(location, containerDirs) {
  const label = locationLabel(location).toLowerCase();
  if (!label) return "";
  const isDrive = label.length === 1 && /^[A-Za-z]:/.test(location.trim());
  const wanted = isDrive ? [label, `drive${label}`, `drive-${label}`, `drive_${label}`] : [label];
  const hits = containerDirs.filter((d) => {
    const base = d.split("/").filter(Boolean).pop()?.toLowerCase();
    return wanted.includes(base);
  });
  return hits.length === 1 ? hits[0] : "";
}

/**
 * One mapping row per distinct folder of every movie/TV library, prefilled
 * from `existing` mappings, else a suggestion.
 */
export function buildMappingRows(libraries, containerDirs, existing = []) {
  const seen = new Set();
  const rows = [];
  for (const lib of libraries) {
    if (lib.type !== "movie" && lib.type !== "show") continue;
    for (const loc of lib.locations || []) {
      if (seen.has(loc)) continue;
      seen.add(loc);
      const have = existing.find((m) => m.plex_prefix === loc);
      rows.push({
        library: lib.title,
        plex_prefix: loc,
        container_path: have?.container_path || suggestContainerPath(loc, containerDirs),
      });
    }
  }
  return rows;
}

/** Rows ready to save: only those with a container path filled in. */
export function mappingsToSave(rows) {
  return rows
    .filter((r) => r.container_path.trim())
    .map((r) => ({ plex_prefix: r.plex_prefix, container_path: r.container_path.trim(), label: r.library }));
}

/** Media servers the app can index from, with the wording each needs. */
export const SERVERS = {
  plex: {
    label: "Plex",
    urlPlaceholder: "http://192.168.1.10:32400",
    secretLabel: "Token",
    secretHint: "X-Plex-Token",
    help: "The token is in Plex → any item → Get Info → View XML → the X-Plex-Token in the URL.",
  },
  jellyfin: {
    label: "Jellyfin",
    urlPlaceholder: "http://192.168.1.10:8096",
    secretLabel: "API key",
    secretHint: "Jellyfin API key",
    help: "Create an API key in Jellyfin → Dashboard → Advanced → API Keys.",
  },
};

/**
 * The settings fields for the chosen server. A blank secret is omitted so the
 * server keeps the saved one; the other server's fields are never sent, so
 * switching servers doesn't touch its stored credentials.
 */
export function serverPayload({ server, url, secret }) {
  const kind = server === "jellyfin" ? "jellyfin" : "plex";
  const p = { media_server: kind, [`${kind}_url`]: url };
  if (secret) p[kind === "plex" ? "plex_token" : "jellyfin_api_key"] = secret;
  return p;
}
