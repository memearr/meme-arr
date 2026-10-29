// Turns a GET /api/health report into what the sidebar badge and the
// System tab's summary row show. Info-level issues (e.g. "update available")
// are listed but never counted as problems.

/**
 * `{ count, level, label }`: `count` is errors + warnings, `level` the worst
 * of them ("error" | "warning" | null), `label` a one-line summary.
 */
export function summarizeHealth(report) {
  if (!report || report.checked_at == null) {
    return { count: 0, level: null, label: "Checking…" };
  }
  const issues = report.issues || [];
  const errors = issues.filter((i) => i.level === "error").length;
  const warnings = issues.filter((i) => i.level === "warning").length;
  const count = errors + warnings;
  if (count === 0) return { count, level: null, label: "No problems found" };
  const parts = [];
  if (errors) parts.push(`${errors} error${errors === 1 ? "" : "s"}`);
  if (warnings) parts.push(`${warnings} warning${warnings === 1 ? "" : "s"}`);
  return { count, level: errors ? "error" : "warning", label: parts.join(", ") };
}
