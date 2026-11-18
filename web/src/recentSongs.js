// The songs picked lately, so a song used before is one click away. Kept in the browser (it is
// a convenience for whoever is using it, not shared); where the browser won't store anything the
// list just doesn't outlive the page.

const KEY = "memearr.recentSongs";
export const MAX_RECENT = 8;

const text = (v) => (typeof v === "string" ? v : "");

/** A song as it is kept: only what the list shows and what picking it needs. */
function clean(t) {
  if (!t || typeof t.id !== "string" || !t.id) return null;
  return {
    id: t.id,
    title: text(t.title),
    artist: text(t.artist),
    album: text(t.album),
    duration_s: Number.isFinite(t.duration_s) ? t.duration_s : 0,
    cover_art: text(t.cover_art),
  };
}

/** The remembered songs, newest first (an empty list when there are none or they can't be read). */
export function loadRecent(store) {
  try {
    const list = JSON.parse(store?.getItem(KEY) || "[]");
    return (Array.isArray(list) ? list : []).map(clean).filter(Boolean).slice(0, MAX_RECENT);
  } catch (_) {
    return [];
  }
}

function save(store, list) {
  try {
    store?.setItem(KEY, JSON.stringify(list));
  } catch (_) {
    /* storage unavailable: not remembered */
  }
}

/** `list` with `track` put first (once), the oldest dropped past `MAX_RECENT`; also stored. */
export function rememberSong(store, list, track) {
  const t = clean(track);
  if (!t) return list;
  const next = [t, ...list.filter((x) => x.id !== t.id)].slice(0, MAX_RECENT);
  save(store, next);
  return next;
}

/** Forget them all. */
export function clearRecent(store) {
  save(store, []);
  return [];
}
