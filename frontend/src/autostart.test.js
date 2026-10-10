import assert from "node:assert/strict";
import test from "node:test";
import { createXChatModule, HttpWsAdapter, runtimeCapabilities, TauriAdapter } from "./xchat.js";

test("autostart uses the local system state, preserves chat on read failure, and reports failed saves", async () => {
  const descriptors = Object.fromEntries(["navigator", "window"].map((key) =>
    [key, Object.getOwnPropertyDescriptor(globalThis, key)]));
  let workspace;
  let enabled = false;
  let readFails = false;
  let writeFails = false;
  let ignoreWrite = false;
  let cursor = 0;
  const calls = [];
  const tauri = {
    core: {
      async invoke(command, args) {
        calls.push(command);
        if (command === "sync_workspace") return {
          cursor: String(++cursor), reset: cursor === 1,
          changes: cursor === 1 ? {
            self: { id: "self", name: "Me" }, devices: [], conversations: [],
            files: [], transfers: [], settings: { language: "zh-CN" }, capabilities: {},
          } : { settings: { language: "zh-CN" } },
        };
        if (command === "get_autostart_enabled") {
          if (readFails) throw new Error("System settings unavailable");
          return enabled;
        }
        if (command === "set_autostart_enabled") {
          if (writeFails) throw new Error("Permission denied");
          if (!ignoreWrite) enabled = args.enabled;
        }
      },
    },
    event: { listen: async () => () => {} },
  };
  try {
    Object.defineProperty(globalThis, "navigator", { configurable: true, value: { userAgent: "Windows NT 10.0" } });
    Object.defineProperty(globalThis, "window", { configurable: true, value: { __TAURI__: tauri } });
    workspace = createXChatModule();
    assert.equal((await workspace.dispatch({ type: "bootstrap" })).ok, true);
    assert.equal(workspace.getSnapshot().settings.autostart_enabled, false);
    assert.equal(calls.includes("set_autostart_enabled"), false, "opening the app must not enable autostart");

    const save = (value) => workspace.dispatch({ type: "settings.patch", patch: { autostart_enabled: value } });
    assert.equal((await save(true)).ok, true);
    assert.equal(enabled, true);
    assert.equal(workspace.getSnapshot().settings.autostart_enabled, true);
    assert.equal(workspace.getSnapshot().notices.at(-1).kind, "success");
    await workspace.dispatch({ type: "refresh" });
    assert.equal(workspace.getSnapshot().settings.autostart_enabled, true, "database snapshots must not reset native preferences");
    assert.equal((await save(false)).ok, true);
    assert.equal(enabled, false);

    writeFails = true;
    assert.equal((await save(false)).ok, true, "already-disabled autostart does not attempt a registry deletion");
    assert.equal((await save(true)).ok, false);
    assert.equal(workspace.getSnapshot().settings.autostart_enabled, false);
    assert.equal(workspace.getSnapshot().notices.at(-1).kind, "error");
    writeFails = false;
    ignoreWrite = true;
    assert.equal((await save(true)).ok, false, "a write must be confirmed against system state");
    ignoreWrite = false;

    enabled = true;
    await workspace.dispatch({ type: "refresh" });
    assert.equal(workspace.getSnapshot().settings.autostart_enabled, true, "external system changes are reflected");
    readFails = true;
    assert.equal((await workspace.dispatch({ type: "refresh" })).ok, true);
    assert.equal(workspace.getSnapshot().phase, "ready");
    assert.equal(workspace.getSnapshot().settings.autostart_enabled, null);
    assert.equal(workspace.getSnapshot().settings.autostart_error, true);
    readFails = false;
    await workspace.dispatch({ type: "refresh" });
    assert.equal(workspace.getSnapshot().settings.autostart_enabled, true);
    assert.equal(workspace.getSnapshot().settings.autostart_error, false);

    await workspace.dispatch({ type: "shutdown" });
    workspace = null;
    for (const userAgent of ["Linux; Android 14", "iPhone; CPU iPhone OS 18 like Mac OS X"]) {
      Object.defineProperty(globalThis, "navigator", { configurable: true, value: { userAgent } });
      assert.equal(runtimeCapabilities("tauri", { autostart: true }).autostart, false);
      await assert.rejects(new TauriAdapter(tauri).patchSettings({ autostart_enabled: true }, {}));
    }
    assert.equal(runtimeCapabilities("web", { autostart: true }).autostart, false);
    await assert.rejects(new HttpWsAdapter().patchSettings({ autostart_enabled: true }, {}));
  } finally {
    await workspace?.dispatch({ type: "shutdown" });
    for (const [key, descriptor] of Object.entries(descriptors)) {
      if (descriptor) Object.defineProperty(globalThis, key, descriptor);
      else delete globalThis[key];
    }
  }
});
