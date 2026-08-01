import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";

test("selected settings tab keeps a transparent background", async () => {
  const css = await readFile(new URL("./styles.css", import.meta.url), "utf8");
  const selected = css.match(/\.settings-nav-row\.selected\s*\{([^}]*)\}/)?.[1];

  assert.ok(selected, "settings selected-state rule is missing");
  assert.doesNotMatch(selected, /\bbackground(?:-color)?\s*:/);
  assert.match(selected, /color:\s*var\(--accent\)/);
  assert.doesNotMatch(selected, /font-weight\s*:/);
});

test("selected file source avoids accent-tinted backgrounds", async () => {
  const css = await readFile(new URL("./styles.css", import.meta.url), "utf8");
  const selected = css.match(/\.source-filter\.selected\s*\{([^}]*)\}/)?.[1];
  assert.ok(selected, "file-source selected-state rule is missing");
  assert.doesNotMatch(selected, /var\(--accent\).*background|background:[^;]*var\(--accent\)/);
});

test("conversation information actions use compact horizontal controls", async () => {
  const css = await readFile(new URL("./styles.css", import.meta.url), "utf8");
  const actions = css.match(/\.info-actions\s*\{([^}]*)\}/)?.[1];
  const button = css.match(/\.info-actions button\s*\{([^}]*)\}/)?.[1];
  assert.match(actions, /display:\s*flex/);
  assert.match(actions, /gap:/);
  assert.match(button, /min-height:\s*32px/);
  assert.match(button, /width:\s*auto/);
});

test("file kind tabs keep selected text styling without a tinted fill", async () => {
  const css = await readFile(new URL("./styles.css", import.meta.url), "utf8");
  const active = css.match(/\.kind-chip\.active\s*\{([^}]*)\}/)?.[1];
  assert.ok(active, "file kind active-state rule is missing");
  assert.match(active, /background:\s*transparent/);
  assert.doesNotMatch(active, /color-mix\([^)]*var\(--accent\)/);
});

test("capture pin view makes the document surfaces transparent", async () => {
  const css = await readFile(new URL("./styles.css", import.meta.url), "utf8");
  assert.match(css, /capture-view-transparent[\s\S]*background:\s*transparent\s*!important/);
});

test("capture pin window can commit edits through the pin command ACL", async () => {
  const capability = JSON.parse(
    await readFile(
      new URL("../../src-tauri/capabilities/capture-pin.json", import.meta.url),
      "utf8",
    ),
  );
  assert.ok(capability.permissions.includes("allow-pin-capture"));
});

test("conversation presence dots distinguish online and offline peers", async () => {
  const css = await readFile(new URL("./styles.css", import.meta.url), "utf8");
  const online = css.match(/\.conversation-presence\.online\s*\{([^}]*)\}/)?.[1];
  const offline = css.match(/\.conversation-presence\.offline\s*\{([^}]*)\}/)?.[1];

  assert.match(online, /background:\s*var\(--success\)/);
  assert.match(offline, /background:\s*var\(--muted\)/);
});
