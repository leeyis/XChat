import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";

test("opening a conversation keeps the viewport pinned while images finish loading", async () => {
  const app = await readFile(new URL("./App.jsx", import.meta.url), "utf8");

  assert.match(app, /addEventListener\(\s*["']load["'][\s\S]*?,\s*true\s*\)/);
  assert.match(app, /requestAnimationFrame/);
});

test("message quick actions bridge the visual gap without losing hover", async () => {
  const css = await readFile(new URL("./styles.css", import.meta.url), "utf8");

  assert.match(css, /\.message-body-line::after\s*\{[^}]*width:\s*8px/s);
  assert.match(css, /\.message\.sent\s+\.message-body-line::after\s*\{/);
});

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

test("group quick actions use the approved full-width vertical layout", async () => {
  const css = await readFile(new URL("./styles.css", import.meta.url), "utf8");
  const layouts = [...css.matchAll(/\.group-quick-actions\s*\{([^}]*)\}/g)];
  const finalLayout = layouts.at(-1)?.[1];

  assert.ok(finalLayout, "group quick-action layout rule is missing");
  assert.match(finalLayout, /grid-template-columns:\s*1fr/);
  assert.match(
    css,
    /\.drawer-setting-list\s*>\s*button\s*\{[^}]*height:\s*40px;[^}]*border-right:\s*0;/s,
  );
});

test("chat feedback colors and focused controls match the approved palette", async () => {
  const css = await readFile(new URL("./styles.css", import.meta.url), "utf8");
  assert.match(css, /\.conversation-row\.selected,[\s\S]*?background:\s*#16ad6f/);
  assert.match(css, /\.bubble\s*\{[^}]*background:\s*#9df29f/s);
  assert.match(css, /\.message\.sent \.bubble\s*\{[^}]*background:\s*#9df29f/s);
  assert.match(css, /\.group-setting-row button\.danger\s*\{[^}]*background:\s*var\(--danger\)/s);
  assert.match(css, /\.forward-note:focus-visible\s*\{[^}]*outline:\s*2px solid var\(--accent\)/s);
  assert.match(css, /\.forward-list\s*\{[^}]*overflow-y:\s*auto/s);
  assert.match(css, /\.forward-list::-webkit-scrollbar-thumb\s*\{[^}]*background:\s*transparent/s);
});

test("message actions stay compact and anchored to the message body", async () => {
  const [app, css] = await Promise.all([
    readFile(new URL("./App.jsx", import.meta.url), "utf8"),
    readFile(new URL("./styles.css", import.meta.url), "utf8"),
  ]);

  assert.match(
    app,
    /message-body-line[\s\S]*message-body-content[\s\S]*message-quick-actions/,
  );
  assert.match(
    css,
    /\.message-body-line\s*\{[^}]*position:\s*relative;[^}]*align-items:\s*flex-end;[^}]*gap:\s*8px;/s,
  );
  assert.match(
    css,
    /\.message-body-content\s*\{[^}]*max-width:\s*100%;/s,
  );
  assert.match(
    css,
    /\.message-quick-actions\s*\{[^}]*position:\s*absolute;[^}]*bottom:\s*0;[^}]*left:\s*calc\(100% \+ 8px\);/s,
  );
  assert.match(
    css,
    /\.message\.sent \.message-quick-actions\s*\{[^}]*right:\s*calc\(100% \+ 8px\);[^}]*left:\s*auto;/s,
  );
  assert.match(css, /\.message-stack\s*\{[^}]*max-width:\s*65%;/s);
  assert.doesNotMatch(css, /max-width:\s*calc\(100% - 103px\)/);
  assert.match(
    css,
    /\.message-quick-actions button\s*\{[^}]*width:\s*31px;[^}]*height:\s*30px;/s,
  );
  assert.match(css, /\.message-body-content:has\(\.message-image\)\s*\{[^}]*width:\s*320px;/s);
  assert.match(css, /\.message-body-content:has\(\.message-file\)\s*\{[^}]*width:\s*300px;/s);
});

test("direct conversation actions use full-width icon rows and centered state actions", async () => {
  const [app, css] = await Promise.all([
    readFile(new URL("./App.jsx", import.meta.url), "utf8"),
    readFile(new URL("./styles.css", import.meta.url), "utf8"),
  ]);
  assert.match(app, /direct-info-actions[\s\S]*?<Icon name="edit"[\s\S]*?<Icon name="trash"/);
  assert.match(app, /direct-info-actions drawer-setting-list/);
  assert.match(css, /\.drawer-setting-list\s*\{[^}]*border-block:\s*1px solid var\(--border\)/s);
  assert.match(css, /\.conversation-state-actions\s*\{[^}]*justify-content:\s*center/s);
});

test("conversation drawers use fixed-height full-width setting rows on every desktop platform", async () => {
  const [app, css] = await Promise.all([
    readFile(new URL("./App.jsx", import.meta.url), "utf8"),
    readFile(new URL("./styles.css", import.meta.url), "utf8"),
  ]);

  assert.match(app, /group-quick-actions drawer-setting-list/);
  assert.match(app, /direct-info-actions drawer-setting-list/);
  assert.match(
    css,
    /\.drawer-setting-list\s*>\s*button\s*\{[^}]*width:\s*100%;[^}]*height:\s*40px;[^}]*min-height:\s*40px;[^}]*max-height:\s*40px;[^}]*flex:\s*0 0 40px;/s,
  );
  assert.match(
    css,
    /\.conversation-state-actions\s*\{[^}]*height:\s*48px;[^}]*flex:\s*0 0 48px;[^}]*justify-content:\s*center/s,
  );
});

test("direct drawer keeps WeChat-style sections compact and the footer visible", async () => {
  const [app, css] = await Promise.all([
    readFile(new URL("./App.jsx", import.meta.url), "utf8"),
    readFile(new URL("./styles.css", import.meta.url), "utf8"),
  ]);
  const direct = app.slice(
    app.indexOf("function LegacyInfoPanel"),
    app.indexOf("function GroupManageModal"),
  );

  assert.equal(
    direct.match(/drawer-section-label/g)?.length,
    2,
    "direct drawer needs device-info and conversation-management section labels",
  );
  assert.match(
    css,
    /\.info-panel \.info-kv\s*\{[^}]*flex:\s*0 0 auto;[^}]*grid-auto-rows:\s*32px;[^}]*align-content:\s*start;/s,
  );
  assert.match(
    css,
    /\.info-panel \.info-kv\s*>\s*div\s*\{[^}]*height:\s*32px;[^}]*min-height:\s*32px;[^}]*max-height:\s*32px;/s,
  );
  assert.match(
    css,
    /\.conversation-state-actions\s*\{[^}]*position:\s*sticky;[^}]*bottom:\s*0;/s,
  );
});

test("conversation list scrollbar is narrow and hidden until hover", async () => {
  const css = await readFile(new URL("./styles.css", import.meta.url), "utf8");
  const listRules = [...css.matchAll(/\.list-scroll\s*\{([^}]*)\}/g)];
  const finalListRule = listRules.at(-1)?.[1];

  assert.match(finalListRule, /scrollbar-color:\s*transparent transparent/);
  assert.match(css, /\.list-scroll:hover\s*\{[^}]*scrollbar-color:\s*color-mix/);
  assert.match(css, /\.list-scroll:hover::?-webkit-scrollbar\s*\{\s*width:\s*3px/);
  assert.match(css, /\.list-scroll::?-webkit-scrollbar-thumb\s*\{[^}]*background:\s*transparent/);
});

test("quote action icon keeps the exact prototype geometry", async () => {
  const [app, prototype] = await Promise.all([
    readFile(new URL("./App.jsx", import.meta.url), "utf8"),
    readFile(new URL("../../ui-ref/xchat-desktop-prototype.html", import.meta.url), "utf8"),
  ]);
  const outline = 'd="M4 5h16v12H8l-4 3Z"';
  const lines = 'd="M8 9h8M8 13h5"';

  assert.ok(app.includes(outline) && app.includes(lines));
  assert.ok(prototype.includes(outline) && prototype.includes(lines));
});

test("quote preview and sent messages follow the WeChat reference layout", async () => {
  const [app, css] = await Promise.all([
    readFile(new URL("./App.jsx", import.meta.url), "utf8"),
    readFile(new URL("./styles.css", import.meta.url), "utf8"),
  ]);
  const composer = app.slice(app.indexOf("function Composer"), app.indexOf("function ForwardModal"));
  const messageLayout = app.slice(app.indexOf("visibleMessages.map"), app.indexOf("<Composer"));
  const preview = css.match(/\.quote-preview\s*\{([^}]*)\}/)?.[1];
  const quoted = css.match(/\.quoted-block\s*\{([^}]*)\}/)?.[1];
  const sent = css.match(/\.message\.sent \.quoted-block\s*\{([^}]*)\}/)?.[1];

  assert.ok(composer.indexOf("<textarea") < composer.indexOf('className="quote-preview"'));
  assert.ok(composer.indexOf('className="quote-preview"') < composer.indexOf('className="compose-toolbar"'));
  assert.ok(messageLayout.indexOf('className="bubble"') < messageLayout.indexOf('className="quoted-block"'));
  assert.match(preview, /background:\s*transparent/);
  assert.match(preview, /border-left:\s*2px/);
  assert.match(quoted, /background:\s*transparent/);
  assert.match(quoted, /-webkit-line-clamp:\s*2/);
  assert.match(sent, /border-right:\s*2px/);
  assert.match(sent, /border-left:\s*0/);
  assert.match(css, /\.quote-preview-close\s*\{[^}]*width:\s*18px;[^}]*height:\s*18px;[^}]*border-radius:\s*50%/s);
});

test("quoted messages pass the stable target field used by conversation navigation", async () => {
  const app = await readFile(new URL("./App.jsx", import.meta.url), "utf8");
  const chat = app.slice(app.indexOf("function ChatWorkspace"), app.indexOf("function HostWorkspace"));

  assert.match(chat, /targetClientMessageId:\s*messageId/);
  assert.doesNotMatch(chat, /\n\s+messageId:\s*messageId/);
});

test("all checkbox-like controls use the shared polished square style", async () => {
  const css = await readFile(new URL("./styles.css", import.meta.url), "utf8");
  const checkbox = css.match(/input\[type="checkbox"\]\s*\{([^}]*)\}/)?.[1];
  const selected = css.match(/input\[type="checkbox"\]:checked\s*\{([^}]*)\}/)?.[1];
  const uncheckedHover = css.match(/input\[type="checkbox"\]:hover:not\(:disabled\):not\(:checked\)\s*\{([^}]*)\}/)?.[1];
  const checkedHover = css.match(/input\[type="checkbox"\]:checked:hover:not\(:disabled\)\s*\{([^}]*)\}/)?.[1];
  const forward = css.match(/\.forward-check\s*\{([^}]*)\}/)?.[1];

  assert.match(checkbox, /appearance:\s*none/);
  assert.match(checkbox, /width:\s*22px/);
  assert.match(checkbox, /border-radius:\s*6px/);
  assert.match(selected, /background:\s*var\(--accent\)/);
  assert.match(uncheckedHover, /background:\s*color-mix/);
  assert.match(checkedHover, /background:\s*var\(--accent-hover\)/);
  assert.match(forward, /border-radius:\s*6px/);
});

test("about card uses the application logo and the group snapshot exposes its creator", async () => {
  const [app, css, workspace] = await Promise.all([
    readFile(new URL("./App.jsx", import.meta.url), "utf8"),
    readFile(new URL("./styles.css", import.meta.url), "utf8"),
    readFile(new URL("../../src-tauri/src/workspace.rs", import.meta.url), "utf8"),
  ]);

  assert.match(app, /<img className="about-logo" src="\/app-icon\.png" alt="Xchat" \/>/);
  assert.match(css, /\.about-logo\s*\{[^}]*object-fit:\s*cover/s);
  assert.match(workspace, /pub created_by:\s*Option<String>/);
  assert.match(workspace, /created_by:\s*record\.created_by/);
});

test("every prop passed to Icon is actually destructured by Icon", async () => {
  const app = await readFile(new URL("./App.jsx", import.meta.url), "utf8");
  const accepted = app.match(/function Icon\(\{([^}]*)\}/)?.[1];

  assert.ok(accepted, "Icon signature is missing");
  const known = new Set(accepted.split(",").map((part) => part.split("=")[0].trim()));
  // Icon 渲染时读到未声明的 prop 会抛 ReferenceError，而 Icon 出现在几乎每个界面上，
  // 于是整个应用白屏。Vite 不做作用域检查，所以这里守住。
  for (const [, tag] of app.matchAll(/<Icon\s([^>]*)\/?>/g)) {
    for (const [, prop] of tag.matchAll(/(?:^|\s)([a-z]\w*)=/g)) {
      assert.ok(known.has(prop), `Icon 收到未声明的 prop: ${prop}`);
    }
  }
});

test("user-visible version sources stay synchronized at 0.1.5", async () => {
  const [packageJson, tauriConfig, cargoToml, app, android] = await Promise.all([
    readFile(new URL("../../package.json", import.meta.url), "utf8"),
    readFile(new URL("../../src-tauri/tauri.conf.json", import.meta.url), "utf8"),
    readFile(new URL("../../src-tauri/Cargo.toml", import.meta.url), "utf8"),
    readFile(new URL("./App.jsx", import.meta.url), "utf8"),
    readFile(new URL("../../src-tauri/gen/android/app/build.gradle.kts", import.meta.url), "utf8"),
  ]);

  assert.equal(JSON.parse(packageJson).version, "0.1.5");
  assert.equal(JSON.parse(tauriConfig).version, "0.1.5");
  assert.match(cargoToml, /^version = "0\.1\.5"$/m);
  assert.match(app, /:\s*"0\.1\.5";/);
  assert.match(android, /versionName[^\n]*"0\.1\.5"/);
});
