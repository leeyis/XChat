import test from "node:test";
import assert from "node:assert/strict";
import { parseCaptureColor } from "./capture-sources.js";

test("clipboard RGB and HEX colors normalize to the same opaque color card", () => {
  for (const value of ["#f0a", " #ff00AA\n", "rgb(255, 0, 170)", "RGB(255 0 170)", "rgb(100% 0% 66.6667%)"]) {
    assert.equal(parseCaptureColor(value), "#FF00AA", value);
  }
  assert.equal(parseCaptureColor("rgb(24.2,172.1,113.4)"), "#18AC71");
});

test("clipboard color detection never consumes an ordinary note or invalid channels", () => {
  for (const value of ["Use #f0a for the button", "#12", "#12345g", "rgb(256,0,0)", "rgb(-1,0,0)", "rgb(101%,0%,0%)", "rgb(1,2)", "rgb(1,2,3,4)", "rgb(1 2 3 / .5)", "rgba(1,2,3,.5)", "rgb(Infinity,0,0)", "rgb(1,,2)", null]) {
    assert.equal(parseCaptureColor(value), null, String(value));
  }
});
