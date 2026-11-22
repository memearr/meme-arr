/** A ref callback that focuses an element when it first appears (e.g. the first field of a form
 *  the user just opened). Defined once so its identity is stable and it never re-fires on render. */
export function focusOnMount(el) {
  el?.focus();
}
