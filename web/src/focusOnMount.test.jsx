// @vitest-environment jsdom
import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
import { AuthScreen } from "./AuthScreen.jsx";

afterEach(cleanup);

describe("the field a form opens on", () => {
  it("has the focus when the login screen appears", () => {
    render(<AuthScreen mode="login" onDone={() => {}} />);
    expect(document.activeElement).toBe(screen.getByLabelText(/password/i));
  });
});
