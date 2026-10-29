// Explains a failed "is there a newer version?" check in words, because a
// hover-only tooltip is invisible on touch screens and easy to miss.

/**
 * @param {string|null|undefined} error  the backend's error text, if any
 * @param {boolean} unreachable          the request to *this app's* backend failed
 * @returns {{reason: string, fix: string}}
 */
export function updateFailureHelp(error, unreachable = false) {
  if (unreachable) {
    return { reason: "Couldn't reach the app's server.", fix: "Check that the container is running, then try again." };
  }
  const reason = (error || "").trim() || "The update check failed.";
  if (/\b404\b/.test(reason)) {
    return {
      reason,
      fix:
        "GitHub answers 404 for a private repository unless a token is supplied. Set GITHUB_TOKEN in the " +
        "container (read access to the repo), or make the repo public.",
    };
  }
  if (/\b(401|403)\b/.test(reason)) {
    return {
      reason,
      fix: "GitHub rejected the token, or it has no read access to this repo (or hit a rate limit). Check GITHUB_TOKEN.",
    };
  }
  return { reason, fix: "Check the container can reach api.github.com (System → Health has more detail)." };
}
