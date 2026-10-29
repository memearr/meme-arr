import { api } from "./api.js";
import { applyTheme, nextTheme, storedTheme } from "./theme.js";
import { updateFailureHelp } from "./update.js";
import { useEffect, useState } from "react";

const NAV = [
  { id: "make", label: "Make", icon: "🎬" },
  { id: "sequence", label: "Sequence", icon: "🔗" },
  { id: "library", label: "Library", icon: "🎞️" },
  { id: "wanted", label: "Wanted", icon: "🔎" },
  { id: "activity", label: "Activity", icon: "📊" },
  { id: "settings", label: "Settings", icon: "⚙️" },
  { id: "system", label: "System", icon: "❤️" },
];

export function Sidebar({ view, setView, index, sequenceCount, health, onLogout }) {
  const stats = index?.stats || {};
  const running = index?.status?.running;
  return (
    <aside className="sidebar">
      <div className="brand">
        <span className="logo">🎬</span>
        <span className="brand-name">
          meme<span className="arr">-arr</span>
        </span>
      </div>
      <nav className="nav">
        {NAV.map((n) => (
          <button
            key={n.id}
            className={"navitem" + (view === n.id ? " on" : "")}
            onClick={() => setView(n.id)}
          >
            <span className="nic">{n.icon}</span>
            <span className="nlabel">{n.label}</span>
            {n.id === "sequence" && sequenceCount > 0 && (
              <span className="navbadge">{sequenceCount}</span>
            )}
            {n.id === "system" && health.count > 0 && (
              <span className={`navbadge health-${health.level}`} title={health.label}>
                {health.count}
              </span>
            )}
          </button>
        ))}
      </nav>
      <div className="sidefoot">
        <div className="foot-row">
          <span className={running ? "dot live" : "dot"} />
          <span>{running ? "Indexing…" : `${stats.cue_count || 0} lines indexed`}</span>
        </div>
        <VersionLine />
        <ThemeToggle />
        <button type="button" className="link logout-link" onClick={onLogout}>
          Log out
        </button>
      </div>
    </aside>
  );
}

const THEME_LABELS = { auto: "Auto", light: "Light", dark: "Dark" };

function ThemeToggle() {
  const [theme, setTheme] = useState(storedTheme);
  return (
    <button
      type="button"
      className="link theme-toggle"
      title="Theme: Auto follows your system. Click to cycle Auto → Light → Dark."
      onClick={() => setTheme(applyTheme(nextTheme(theme)))}
    >
      Theme: {THEME_LABELS[theme]}
    </button>
  );
}

function VersionLine() {
  const [v, setV] = useState(null);
  const [checking, setChecking] = useState(false);
  // Distinguishes "never checked yet" (no UI shown at all, same as before)
  // from "we tried and the request itself didn't even come back" -- a
  // failure one level below the backend's own checked:false (which means
  // the backend reached out to GitHub and GitHub said no).
  const [fetchFailed, setFetchFailed] = useState(false);
  const [showWhy, setShowWhy] = useState(false);

  async function check() {
    setChecking(true);
    setFetchFailed(false);
    try {
      setV(await api.version());
    } catch (_) {
      setFetchFailed(true);
    } finally {
      setChecking(false);
    }
  }
  useEffect(() => {
    check();
  }, []);

  if (!v && !fetchFailed) return null;
  const why = updateFailureHelp(v?.error, fetchFailed);
  return (
    <div className="version">
      {v && <span className="ver">{v.current}</span>}
      {v?.update_available ? (
        <a className="upd" href={v.releases_url} target="_blank" rel="noreferrer">
          update → {v.latest}
        </a>
      ) : v?.checked ? (
        <span className="uptodate">up to date</span>
      ) : (
        <button
          type="button"
          className="warn linklike"
          aria-expanded={showWhy}
          title="Click for the reason"
          onClick={() => setShowWhy((w) => !w)}
        >
          ⚠ check failed
        </button>
      )}
      {v && !v.checked && (
        <a className="ghlink" href={v.releases_url} target="_blank" rel="noreferrer">
          releases ↗
        </a>
      )}
      <button className="icbtn labeled" title="Check for updates now" onClick={check} disabled={checking}>
        {checking ? "Checking…" : "↻ Check now"}
      </button>
      {showWhy && !v?.checked && (
        <div className="version-why" role="status">
          <div>{why.reason}</div>
          <div className="hint tiny">{why.fix}</div>
        </div>
      )}
    </div>
  );
}
