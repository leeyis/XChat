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

test("download settings expose the approved maximum parallel channel control", async () => {
  const app = await readFile(new URL("./App.jsx", import.meta.url), "utf8");
  const downloadSettings = app.slice(
    app.indexOf('id="settings-download"'),
    app.indexOf('id="settings-network"'),
  );

  assert.match(app, /maxParallelChannels:\s*"最大并行通道"/);
  assert.match(
    app,
    /兼顾兼容性与资源占用。保存后对新开始的传输生效；旧版设备会自动使用 4 个通道。/,
  );
  assert.match(downloadSettings, /form\.max_parallel_channels/);
  assert.match(
    downloadSettings,
    /change\("max_parallel_channels",\s*Number\(event\.target\.value\)\)/,
  );
  for (const value of [4, 8, 16]) {
    assert.match(downloadSettings, new RegExp(`value=\\{${value}\\}`));
  }
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

test("every settings section is still rendered by the settings list", async () => {
  const app = await readFile(new URL("./App.jsx", import.meta.url), "utf8");
  const groupsRaw = app.match(/const SETTINGS_GROUPS = \[([\s\S]*?)\];/)?.[1];
  const sections = app.match(/settingsSections: \{([\s\S]*?)\n    \}/)?.[1];

  assert.ok(groupsRaw, "SETTINGS_GROUPS is missing");
  assert.ok(sections, "settingsSections labels are missing");

  // 先把行注释去掉：否则在数组里留一句 // "shortcut" 暂时移除
  // 就能让断言通过，而那一节其实已经不再渲染。
  const groups = groupsRaw.replace(/\/\/[^\n]*/g, "");
  // 取数组里的字符串字面量，而不是对整个源码做子串匹配
  const listed = [...groups.matchAll(/"([a-z_]+)"/g)].map((match) => match[1]);
  const declared = [...sections.matchAll(/^\s{6}(\w+):\s*\{/gm)].map((match) => match[1]);

  assert.ok(declared.length >= 6, `设置分节数量异常：${declared.length}`);

  // 「我的」页改用 SETTINGS_GROUPS 分组渲染之后，漏写一个 id 就等于
  // 把它从桌面端的设置列表里也删掉了。快捷键就踩过这个坑。
  for (const id of declared) {
    assert.ok(
      listed.includes(id),
      `设置分节 "${id}" 没有出现在 SETTINGS_GROUPS 里`,
    );
  }
  // 反向也要查：写了不存在的 id 同样是错的
  for (const id of listed) {
    assert.ok(
      declared.includes(id),
      `SETTINGS_GROUPS 里的 "${id}" 不是有效的设置分节`,
    );
  }
  assert.equal(new Set(listed).size, listed.length, "SETTINGS_GROUPS 里有重复项");
});

test("narrow-screen stack pages always offer a way back", async () => {
  const [app, css] = await Promise.all([
    readFile(new URL("./App.jsx", import.meta.url), "utf8"),
    readFile(new URL("./styles.css", import.meta.url), "utf8"),
  ]);

  // 窄屏下压栈页会把底部 Tab 栏整个收起，返回键是唯一出口。
  // no-selection 空态原本没有返回键：删掉最后一个设备后 devices 里找不到它，
  // 页面就停在「请选择」上，Tab 栏没有、列表被盖住，只能重启。
  const empties = [
    ...app.matchAll(/<main className="workspace [\w-]+ no-selection">([\s\S]*?)<\/main>/g),
  ];
  assert.ok(empties.length >= 2, `no-selection 空态数量异常：${empties.length}`);
  for (const [, body] of empties) {
    assert.match(body, /className="mobile-back/, "no-selection 空态缺少返回键");
  }

  // 头部得真的留出 56px 一行，否则返回键会被空态挤掉
  assert.match(
    css,
    /\.chat-workspace\.no-selection,[\s\S]*?grid-template-rows:\s*56px minmax\(0,\s*1fr\)/,
  );

  // 压栈页收起 Tab 栏的规则必须和「列表二选一」同时存在，
  // 否则会出现「Tab 栏没了、列表还在被盖着」的组合
  assert.match(css, /\.app-shell:not\(\.mobile-list\) \.rail\s*\{[^}]*display:\s*none/);
  assert.match(css, /\.app-shell:not\(\.mobile-list\) \.list-pane,[\s\S]*?display:\s*none/);
});

test("message bubbles stay legible in dark theme", async () => {
  const css = await readFile(new URL("./styles.css", import.meta.url), "utf8");
  // 扫描所有给气泡上文字的规则，而不是只看固定那两条。
  // 只看两条的话，后面再补一条 .message .bubble { color: var(--fg) }
  // 就能让断言照过、而深色下的 bug 原样回来。
  const bubbleRules = [...css.matchAll(/([^{}]+)\{([^}]*)\}/g)].filter(
    ([, selector, body]) =>
      !selector.includes("@") &&
      /\.bubble\b/.test(selector) &&
      /(?:^|[;\s])color\s*:/.test(body),
  );

  assert.ok(
    bubbleRules.length >= 2,
    `涉及气泡文字的规则数量异常：${bubbleRules.length}`,
  );

  for (const [, selector, body] of bubbleRules) {
    // 深色主题把 --fg 换成近白色 oklch(.93 .005 75)，
    // 落在 #9df29f 上对比度只剩 1.09:1，整条消息等于看不见。
    // 钉死成 #221c15 后两个主题都是 12.56:1。
    assert.doesNotMatch(
      body,
      /color:\s*var\(--fg\)/,
      `${selector.trim()} 的气泡文字跟随了主题`,
    );
    assert.match(
      body,
      /color:\s*#221c15/,
      `${selector.trim()} 的气泡文字色不是钉死的 #221c15`,
    );
  }
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

test("pending messages to an offline peer say they are waiting, not sending", async () => {
  const app = await readFile(new URL("./App.jsx", import.meta.url), "utf8");
  const body = app.match(/function statusLabel\([^)]*\)\s*\{([\s\S]*?)\n\}/)?.[1];

  assert.ok(body, "statusLabel is missing");
  // 文件传输状态必须能覆盖旧数据里错误的 sent 消息状态。
  assert.match(body, /messageDeliveryStatus\(message,\s*!group\s*&&\s*peerOffline\)/);
  assert.match(body, /deliveryStatus\s*===\s*"waiting_peer"/);
  assert.match(body, /labels\.status\.waiting_peer/);
  // 调用点必须真的把离线状态传进来，否则这条分支永远走不到
  assert.match(app, /statusLabel\(message,[^)]*peer\?\.is_offline\)/);
});

test("fixed-address UI tests identity before saving and explains offline safety", async () => {
  const app = await readFile(new URL("./App.jsx", import.meta.url), "utf8");
  const modal = app.match(/function EndpointModal\([^)]*\)\s*\{([\s\S]*?)\n\}\n\nfunction RemarkModal/)?.[1];

  assert.ok(modal, "EndpointModal is missing");
  assert.match(modal, /type:\s*"device\.testEndpoint"/);
  assert.match(modal, /expectedDeviceId:\s*testResult\.identity\.device_id/);
  assert.match(modal, /labels\.endpointHelper/);
  assert.match(app, /测试只核对设备身份，不会发送聊天内容/);
  assert.match(app, /对方已离线/);
  assert.match(app, /消息暂不发送，对方上线后自动发送。/);
});

test("offline toasts do not render as errors", async () => {
  const css = await readFile(new URL("./styles.css", import.meta.url), "utf8");
  const rule = css.match(/\.toast\.warning i\s*\{([^}]*)\}/)?.[1];

  assert.ok(rule, ".toast.warning rule is missing");
  assert.match(rule, /var\(--warning\)/);
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

test("user-visible version sources stay synchronized at 0.1.6", async () => {
  const [packageJson, tauriConfig, cargoToml, app, android] = await Promise.all([
    readFile(new URL("../../package.json", import.meta.url), "utf8"),
    readFile(new URL("../../src-tauri/tauri.conf.json", import.meta.url), "utf8"),
    readFile(new URL("../../src-tauri/Cargo.toml", import.meta.url), "utf8"),
    readFile(new URL("./App.jsx", import.meta.url), "utf8"),
    readFile(new URL("../../src-tauri/gen/android/app/build.gradle.kts", import.meta.url), "utf8"),
  ]);

  assert.equal(JSON.parse(packageJson).version, "0.1.6");
  assert.equal(JSON.parse(tauriConfig).version, "0.1.6");
  assert.match(cargoToml, /^version = "0\.1\.6"$/m);
  assert.match(app, /:\s*"0\.1\.6";/);
  assert.match(android, /versionName[^\n]*"0\.1\.6"/);
});
