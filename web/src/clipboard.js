// Copying text to the clipboard.
//
// `navigator.clipboard` only exists on secure pages (HTTPS or localhost). This
// app is often opened over plain HTTP on a home network, where it is simply
// missing, so a button that only used it did nothing. The older
// select-and-copy command works there too.

/** Copy `text`; resolves to whether it worked. */
export async function copyText(text) {
  try {
    if (typeof navigator !== "undefined" && navigator.clipboard && window.isSecureContext) {
      await navigator.clipboard.writeText(text);
      return true;
    }
  } catch (_) {
    /* fall through to the older way */
  }
  try {
    const box = document.createElement("textarea");
    box.value = text;
    box.setAttribute("readonly", "");
    box.style.cssText = "position:fixed;top:0;left:0;opacity:0;pointer-events:none";
    document.body.appendChild(box);
    box.select();
    box.setSelectionRange(0, text.length);
    const ok = document.execCommand("copy");
    box.remove();
    return ok;
  } catch (_) {
    return false;
  }
}

/** Select what is in `el` (an input, or any element's text), so Ctrl+C works. */
export function selectContents(el) {
  if (!el) return;
  if (typeof el.select === "function") {
    el.focus?.();
    el.select();
  } else if (typeof window !== "undefined" && window.getSelection) {
    const range = document.createRange();
    range.selectNodeContents(el);
    const sel = window.getSelection();
    sel.removeAllRanges();
    sel.addRange(range);
  }
}
