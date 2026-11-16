// @vitest-environment jsdom
//
// The Look controls in the Captions box, and the styled caption over the player.
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { useState } from "react";
import { afterEach, describe, expect, it } from "vitest";
import { editorSeed } from "./sequenceClip.js";
import { api } from "./api.js";
import { captionMetrics } from "./captionPreview.js";
import { LookControls } from "./LookControls.jsx";
import { lookForRequest } from "./textLook.js";

afterEach(cleanup);

/** The controls with a real `look` state, and the request body that state would send. */
function Harness({ initial = null }) {
  const [look, setLook] = useState(initial);
  return (
    <>
      <LookControls look={look} setLook={setLook} />
      <output data-testid="sent">{JSON.stringify(lookForRequest(look))}</output>
    </>
  );
}
const sent = () => JSON.parse(screen.getByTestId("sent").textContent);

describe("Look controls", () => {
  it("start with the plain subtitle look and show nothing else", () => {
    render(<Harness />);
    expect(screen.getByLabelText("Look").value).toBe("");
    expect(sent()).toBeNull();
    expect(screen.queryByLabelText("Colour")).toBeNull();
  });

  it("choose a preset, then colour, alignment, entrance and word reveal", () => {
    render(<Harness />);
    fireEvent.change(screen.getByLabelText("Look"), { target: { value: "block" } });
    expect(sent()).toEqual({ preset: "block", align: "center", anim: "none", reveal: "none" });
    expect(screen.getByLabelText("Colour").value).toBe("#ffe600");

    fireEvent.change(screen.getByLabelText("Colour"), { target: { value: "#00ff00" } });
    fireEvent.change(screen.getByLabelText("Align"), { target: { value: "left" } });
    fireEvent.change(screen.getByLabelText("Comes in"), { target: { value: "pop" } });
    fireEvent.click(screen.getByLabelText("Words appear as they are said"));
    expect(sent()).toEqual({ preset: "block", align: "left", anim: "pop", reveal: "word", color: "#00ff00" });

    // a picked colour can be dropped for the preset's own
    fireEvent.click(screen.getByText("preset colour"));
    expect(sent()).not.toHaveProperty("color");
  });

  it("place the text on a spot, and put it back to the bottom", () => {
    render(<Harness initial={{ preset: "classic" }} />);
    expect(screen.queryByText(/Across/)).toBeNull();
    fireEvent.click(screen.getByLabelText("Place the text myself"));
    expect(sent()).toMatchObject({ x: 0.5, y: 0.8 });
    fireEvent.change(screen.getByLabelText(/Across/), { target: { value: "20" } });
    fireEvent.change(screen.getByLabelText(/Down/), { target: { value: "35" } });
    expect(sent()).toMatchObject({ x: 0.2, y: 0.35 });
    fireEvent.click(screen.getByLabelText("Place the text myself"));
    expect(sent()).not.toHaveProperty("x");
    expect(sent()).not.toHaveProperty("y");
  });

  it("switching preset keeps the place and motion but not a colour picked for the old one", () => {
    render(<Harness initial={{ preset: "block", color: "#123456", x: 0.3, y: 0.4, anim: "fade" }} />);
    fireEvent.change(screen.getByLabelText("Look"), { target: { value: "neon" } });
    const s = sent();
    expect(s).toMatchObject({ preset: "neon", x: 0.3, y: 0.4, anim: "fade" });
    expect(s).not.toHaveProperty("color");
  });

  it("go back to plain subtitles", () => {
    render(<Harness initial={{ preset: "neon" }} />);
    fireEvent.change(screen.getByLabelText("Look"), { target: { value: "" } });
    expect(sent()).toBeNull();
  });
});

describe("where the look reaches the rest of the editor", () => {
  it("a stored clip reopens with its look", () => {
    const look = { preset: "horror", reveal: "word" };
    expect(editorSeed({ caption: { mode: "source", look } }).look).toEqual(look);
    expect(editorSeed({ caption: { mode: "source" } }).look).toBeNull();
    expect(editorSeed({ caption: { mode: "meme", look } }).look).toBeNull();
  });

  it("the preview request carries it as look_json", () => {
    const url = api.previewUrl("7", { start: 0, end: 1000, width: 480, look: { preset: "neon" } });
    expect(new URL(url, "http://x").searchParams.get("look_json")).toBe('{"preset":"neon"}');
    expect(api.previewUrl("7", { start: 0, end: 1000, width: 480 })).not.toContain("look_json");
  });

  it("a preset's own text size sets the overlay size", () => {
    const plain = captionMetrics({ mode: "source", width: 640, height: 360 });
    const block = captionMetrics({ mode: "source", width: 640, height: 360, baseSize: 36 });
    expect(block.fontSize / plain.fontSize).toBeCloseTo(36 / 16, 5);
  });
});
