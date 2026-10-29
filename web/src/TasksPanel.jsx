import { api } from "./api.js";
import { fmtDuration, fmtInterval, fmtRelative } from "./format.js";
import { useEffect, useState } from "react";

const INTERVAL_CHOICES = [900, 1800, 3600, 6 * 3600, 12 * 3600, 86400, 3 * 86400, 7 * 86400, 14 * 86400, 30 * 86400];

/** Preset intervals within a task's allowed range, plus its current one if custom. */
function intervalChoices(task) {
  const choices = INTERVAL_CHOICES.filter(
    (s) => s >= task.min_interval_secs && s <= task.max_interval_secs
  );
  return choices.includes(task.interval_secs)
    ? choices
    : [...choices, task.interval_secs].sort((a, b) => a - b);
}

export function TasksPanel() {
  const [tasks, setTasks] = useState(null);
  const [err, setErr] = useState(null);
  const [now, setNow] = useState(() => Math.floor(Date.now() / 1000));

  function load() {
    api
      .listTasks()
      .then((t) => {
        setTasks(t);
        setErr(null);
      })
      .catch((e) => setErr(e.message));
    setNow(Math.floor(Date.now() / 1000));
  }

  useEffect(() => {
    load();
    const t = setInterval(load, 5000);
    return () => clearInterval(t);
  }, []);

  async function runNow(id) {
    try {
      await api.runTask(id);
    } catch (e) {
      setErr(e.message);
    }
    load();
  }

  async function save(task, changes) {
    try {
      await api.updateTask(task.id, {
        enabled: task.enabled,
        interval_secs: task.interval_secs,
        ...changes,
      });
    } catch (e) {
      setErr(e.message);
    }
    load();
  }

  return (
    <div className="panel tasks-panel">
      <h3>Tasks</h3>
      <p className="hint tiny">
        Each runs on its own schedule. A restart only runs what's actually due — it no longer rescans
        the whole library every time.
      </p>
      {err && <p className="error">{err}</p>}
      {tasks == null ? (
        <p className="hint">Loading…</p>
      ) : (
        <table className="drill-table tasks-table">
          <thead>
            <tr>
              <th>Task</th>
              <th>Every</th>
              <th>Last run</th>
              <th>Took</th>
              <th>Next run</th>
              <th />
            </tr>
          </thead>
          <tbody>
            {tasks.map((t) => (
              <tr key={t.id} title={t.description}>
                <td>
                  <label className="task-name">
                    <input
                      type="checkbox"
                      checked={t.enabled}
                      onChange={(e) => save(t, { enabled: e.target.checked })}
                    />
                    {t.label}
                  </label>
                  <div className={t.last_error ? "error tiny" : "hint tiny"}>
                    {t.last_error ?? t.last_summary ?? ""}
                  </div>
                </td>
                <td>
                  <select
                    value={t.interval_secs}
                    onChange={(e) => save(t, { interval_secs: +e.target.value })}
                  >
                    {intervalChoices(t).map((s) => (
                      <option key={s} value={s}>
                        {fmtInterval(s)}
                      </option>
                    ))}
                  </select>
                </td>
                <td>{t.running ? "running…" : fmtRelative(t.last_started, now)}</td>
                <td>{fmtDuration(t.last_duration_ms)}</td>
                <td>{!t.enabled ? "disabled" : t.next_run <= now ? "due" : fmtRelative(t.next_run, now)}</td>
                <td>
                  <button
                    type="button"
                    className="secondary small"
                    disabled={t.running}
                    onClick={() => runNow(t.id)}
                  >
                    {t.running ? "…" : "Run now"}
                  </button>
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
    </div>
  );
}
