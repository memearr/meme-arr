// Pure helpers for the "Find subtitles online" UI (no React, so they're testable).

/**
 * Languages offered in the picker. The codes are the ones the server maps to
 * Jellyfin's three-letter codes (src/jellyfin.rs `language_code`) and passes
 * straight to Plex, so keep this list inside that table.
 */
export const LANGUAGES = [
  ["en", "English"],
  ["es", "Spanish"],
  ["fr", "French"],
  ["de", "German"],
  ["it", "Italian"],
  ["pt", "Portuguese"],
  ["nl", "Dutch"],
  ["sv", "Swedish"],
  ["da", "Danish"],
  ["no", "Norwegian"],
  ["fi", "Finnish"],
  ["pl", "Polish"],
  ["ru", "Russian"],
  ["ja", "Japanese"],
  ["ko", "Korean"],
  ["zh", "Chinese"],
  ["ar", "Arabic"],
  ["tr", "Turkish"],
  ["cs", "Czech"],
  ["el", "Greek"],
  ["he", "Hebrew"],
  ["hi", "Hindi"],
  ["hu", "Hungarian"],
  ["ro", "Romanian"],
  ["uk", "Ukrainian"],
  ["vi", "Vietnamese"],
  ["th", "Thai"],
  ["id", "Indonesian"],
];

export const DEFAULT_LANGUAGE = "en";

export function languageLabel(code) {
  return LANGUAGES.find(([c]) => c === code)?.[1] ?? code;
}

/** A saved language choice, but only if it's one we still offer. */
export function validLanguage(code) {
  return LANGUAGES.some(([c]) => c === code) ? code : DEFAULT_LANGUAGE;
}

/**
 * How to present the capability check. Mirrors the Health panel's shape:
 * what's wrong, then how to fix it.
 *
 * @returns {{ok: boolean, title: string, detail: string, fix: string|null}}
 */
export function capabilityView(cap) {
  if (!cap) return { ok: false, title: "Checking…", detail: "", fix: null };
  if (cap.supported) {
    const name = cap.server === "jellyfin" ? "Jellyfin" : "Plex";
    const ver = cap.version ? ` ${cap.version}` : "";
    const detail =
      cap.server === "jellyfin"
        ? "Search uses the subtitle provider plugins installed in Jellyfin (for example OpenSubtitles). " +
          "Jellyfin can't tell us up front whether one is set up, so an empty result usually means none is."
        : cap.version_checked
          ? `Plex${ver} supports subtitle search (needs ${cap.minimum_version} or newer).`
          : `Couldn't read Plex's version, so search is attempted anyway; Plex's own answer decides.`;
    return { ok: true, title: `Available via ${name}${ver}`, detail, fix: null };
  }
  const reason = cap.reason || "Subtitle search isn't available.";
  return { ok: false, title: "Not available", detail: reason, fix: fixFor(reason) };
}

/** Fix-it guidance for the server's reason text (health-check style). */
export function fixFor(reason) {
  if (/not configured/i.test(reason)) return "Open Settings and connect your media server first.";
  if (/too old/i.test(reason)) return "Update Plex Media Server to the version named above or newer.";
  if (/only available for plex/i.test(reason)) return "Online subtitle search isn't supported for this media server.";
  if (/couldn't read|unreachable|reach/i.test(reason))
    return "Check the server is up and reachable from this container (System → Health).";
  return "Check System → Health for more detail.";
}

/**
 * Best candidates first: a file-hash match is the most reliable kind, then
 * more downloads, then the server's own order. Never mutates the input.
 */
export function sortCandidates(results) {
  // Jellyfin reports a download count, Plex a match score; either ranks
  // "more people used this" higher.
  const popularity = (c) => c.downloads ?? c.score ?? -1;
  return results
    .map((r, i) => ({ r, i }))
    .sort((a, b) => {
      const h = Number(!!b.r.hash_match) - Number(!!a.r.hash_match);
      if (h) return h;
      const d = popularity(b.r) - popularity(a.r);
      if (d) return d;
      return a.i - b.i;
    })
    .map((x) => x.r);
}

/**
 * Short labels for a candidate's flags. A `hash_match` is Jellyfin's file-hash
 * match or Plex's "perfect match", so the label follows the server.
 */
export function candidateBadges(c, server) {
  const out = [];
  if (c.hash_match) out.push(server === "plex" ? "Perfect match" : "Hash match");
  if (c.hearing_impaired) out.push("SDH");
  if (c.forced) out.push("Forced");
  if (c.ai_translated) out.push("AI translated");
  if (c.machine_translated) out.push("Machine translated");
  return out;
}

/** How popular a result is, in the server's own terms (or null). */
export function popularityText(c) {
  if (c.downloads != null) return `${c.downloads.toLocaleString()} downloads`;
  if (c.score != null) return `score ${c.score.toLocaleString()}`;
  return null;
}

/** "canAutoSync" / "ThreeLetterISOLanguageName" -> "Can auto sync" / "Three letter ISO language name". */
export function humanizeKey(key) {
  const spaced = key
    .replace(/([a-z0-9])([A-Z])/g, "$1 $2")
    .replace(/([A-Z]+)([A-Z][a-z])/g, "$1 $2")
    .replace(/[_-]+/g, " ")
    .trim();
  return spaced.charAt(0).toUpperCase() + spaced.slice(1).toLowerCase().replace(/\biso\b/g, "ISO");
}

/** The server's extra facts about a result as sorted label/value rows. */
export function detailEntries(details) {
  return Object.entries(details || {})
    .map(([k, v]) => ({ label: humanizeKey(k), value: String(v) }))
    .sort((a, b) => a.label.localeCompare(b.label));
}

/** What to tell the user when a search returned nothing. */
export function emptyMessage(note, language) {
  return note || `No ${languageLabel(language)} subtitles found.`;
}

/** What happens after a download is accepted: it's async, then needs indexing. */
export function followUpMessage(server) {
  const name = server === "jellyfin" ? "Jellyfin" : "Plex";
  return (
    `${name} fetches it in the background. Give it a moment, then index this item so the ` +
    `new subtitle becomes searchable.`
  );
}

/**
 * Turn the per-item re-index result into something to show.
 * `unchanged` is the interesting one: nothing new was found, either in a
 * subtitle file next to the video or (Plex) in the copy the server keeps, so
 * the server is still fetching it or stored it somewhere this app can't read.
 *
 * @returns {{ok: boolean, message: string, done: boolean}} `done` = the item now has searchable lines.
 */
export function reindexView(result) {
  if (!result) return { ok: false, done: false, message: "" };
  if (result.status === "indexed" && result.lines > 0) {
    const n = result.lines.toLocaleString();
    return { ok: true, done: true, message: `Indexed ${n} subtitle line${result.lines === 1 ? "" : "s"} — it's searchable now.` };
  }
  if (result.status === "indexed") {
    return {
      ok: false,
      done: false,
      message: "Re-checked, but the subtitle file has no usable text lines (is it an image-based subtitle?).",
    };
  }
  return {
    ok: false,
    done: false,
    message:
      "No new subtitle found yet. The server may still be fetching it (try again in a moment), " +
      "or it stored it somewhere this app can't read. Use Diagnose to see which.",
  };
}

/** A wanted-list row's title: the episode (with its own title) or the movie. */
export function wantedTitle(item) {
  if (item.show) {
    const se =
      item.season != null && item.episode != null
        ? ` S${String(item.season).padStart(2, "0")}E${String(item.episode).padStart(2, "0")}`
        : "";
    return { main: item.show + se, sub: item.title };
  }
  return { main: item.year ? `${item.title} (${item.year})` : item.title, sub: null };
}

/** The line under the wanted list about items it deliberately leaves out. */
export function unreachableNote(count) {
  if (!count) return null;
  const noun = count === 1 ? "item isn't" : "items aren't";
  return `${count.toLocaleString()} more ${noun} listed because the file can't be reached — a subtitle won't help until its path mapping is fixed (System → Health).`;
}

/** The language last used, if storage works. */
export function loadLanguage(storage) {
  try {
    return validLanguage(storage?.getItem("memearr:subtitle-language"));
  } catch (_) {
    return DEFAULT_LANGUAGE;
  }
}

export function saveLanguage(storage, code) {
  try {
    storage?.setItem("memearr:subtitle-language", code);
  } catch (_) {
    /* private mode / blocked: the choice just isn't remembered */
  }
}

// ---- Requests (downloads this app has asked the server for) -----------------

/** Waiting this long without the file appearing is worth a different hint. */
export const SLOW_REQUEST_SECS = 10 * 60;

/**
 * How a request is shown: the pill, and a one-line hint of what to do.
 *
 * @param {{state: string, lines?: number}} r  a request from GET /api/subtitles/requests
 * @param {number} ageSecs                     seconds since it was requested
 * @returns {{label: string, tone: "done"|"queued"|"failed", hint: string, canReindex: boolean}}
 */
export function requestStatus(r, ageSecs) {
  if (r.state === "indexed") {
    const n = (r.lines ?? 0).toLocaleString();
    return { label: `Indexed · ${n} line${r.lines === 1 ? "" : "s"}`, tone: "done", hint: "", canReindex: true };
  }
  if (r.state === "item_missing") {
    return {
      label: "Item not indexed",
      tone: "failed",
      hint: "This item is no longer in the index (its library may have been unmapped or rescanned).",
      canReindex: false,
    };
  }
  return {
    label: "Waiting",
    tone: "queued",
    hint:
      ageSecs < SLOW_REQUEST_SECS
        ? "Give the server a moment to fetch it, then re-index."
        : "Still nothing found. Re-index to check again, or use Diagnose to see why.",
    canReindex: true,
  };
}

/** The item a request is for, as a title + subtitle line. */
export function requestItemTitle(r) {
  return wantedTitle({ show: r.show, season: r.season, episode: r.episode, title: r.item_title ?? r.rating_key, year: r.year });
}

/** What was asked for: "Release.name · OpenSubtitles · en". */
export function requestedSubtitleText(r) {
  return [r.subtitle, r.provider, r.language].filter(Boolean).join(" · ");
}

/** Counts for the tab badge and the "clear finished" button. */
export function requestCounts(requests) {
  const waiting = requests.filter((r) => r.state === "waiting").length;
  const indexed = requests.filter((r) => r.state === "indexed").length;
  return { total: requests.length, waiting, indexed };
}

// ---- Diagnosis (why hasn't a requested subtitle become searchable?) ---------

const FINDING_ICON = { ok: "✓", info: "ℹ", warn: "⚠", error: "✕" };

/** The symbol shown beside a finding of this level. */
export function findingIcon(level) {
  return FINDING_ICON[level] ?? "•";
}

/** The most serious level among the findings (error > warn > info > ok), or "info" for none. */
export function worstLevel(findings) {
  const order = ["error", "warn", "info", "ok"];
  for (const l of order) if ((findings || []).some((f) => f.level === l)) return l;
  return "info";
}

/** How a file in the video's folder is described: what the indexer makes of it. */
export function folderFileNote(f) {
  if (f.picked) return "used by the indexer";
  if (!f.matches_video) return "ignored: name doesn't start with the video's name";
  if (!f.indexable) return "ignored: image-based, can't be searched as text";
  return "not chosen (another file was preferred)";
}

/** A server-listed subtitle, one line: "English (SRT) · English · srt · external". */
export function serverStreamText(s) {
  return [s.name, s.language, s.format, s.external ? "external" : "embedded"].filter(Boolean).join(" · ");
}
