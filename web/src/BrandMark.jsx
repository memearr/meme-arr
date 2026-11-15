import { withBase } from "./base.js";

/** The app's icon, next to its name in the sidebar and on the login screen. */
export function BrandMark({ size = 30 }) {
  return (
    <img
      className="logo"
      src={withBase("/icon-192.png")}
      width={size}
      height={size}
      alt=""
      aria-hidden="true"
    />
  );
}
