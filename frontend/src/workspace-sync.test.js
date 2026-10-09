import assert from "node:assert/strict";
import test from "node:test";
import { WorkspaceSyncLoader } from "./workspace-sync.js";

const baseline = () => ({ self: { id: "me" }, settings: {}, capabilities: {}, conversations: [{ id: "one" }], devices: [], files: [], transfers: [] });

test("workspace deltas replace collections, remove deleted records, and reset on restart", async () => {
  const replies = [
    { cursor: "1", reset: true, changes: baseline() },
    { cursor: "2", reset: false, changes: { transfers: [{ id: "upload", bytes_transferred: 7 }] } },
    { cursor: "3", reset: false, changes: { conversations: [] } },
    { cursor: "new", reset: true, changes: baseline() },
  ];
  const cursors = [];
  const loader = new WorkspaceSyncLoader(async (cursor) => { cursors.push(cursor); return replies.shift(); }, () => assert.fail("unexpected fallback"), () => false);
  const initial = await loader.getSnapshot();
  const progress = await loader.getSnapshot();
  assert.equal(progress.conversations, initial.conversations);
  assert.equal(initial.transfers.length, 0, "previous snapshots stay immutable");
  assert.equal(progress.transfers[0].bytes_transferred, 7);
  assert.deepEqual((await loader.getSnapshot()).conversations, []);
  assert.deepEqual((await loader.getSnapshot()).transfers, []);
  assert.deepEqual(cursors, [null, "1", "2", "3"]);
});

test("refreshes arriving during a read share one follow-up, with the new cursor", async () => {
  let resolve;
  const cursors = [];
  const loader = new WorkspaceSyncLoader((cursor) => {
    cursors.push(cursor);
    if (cursors.length === 1) return new Promise((done) => { resolve = done; });
    return Promise.resolve({ cursor: "2", reset: false, changes: { conversations: [] } });
  }, () => assert.fail("unexpected fallback"), () => false);
  const first = loader.getSnapshot();
  const queued = Array.from({ length: 100 }, () => loader.getSnapshot());
  resolve({ cursor: "1", reset: true, changes: baseline() });
  assert.equal((await first).conversations.length, 1);
  const results = await Promise.all(queued);
  assert.ok(results.every((result) => result.conversations.length === 0));
  assert.deepEqual(cursors, [null, "1"]);
});

test("older backends fall back once while transient failures retain the cursor", async () => {
  let calls = 0;
  let full = 0;
  const old = new WorkspaceSyncLoader(async () => { calls++; throw { status: 404 }; }, async () => { full++; return baseline(); }, (e) => e.status === 404);
  await old.getSnapshot();
  await old.getSnapshot();
  assert.equal(calls, 1);
  assert.equal(full, 2);
  const loader = new WorkspaceSyncLoader(async () => { throw new Error("offline"); }, () => assert.fail("must not hide outage"), () => false);
  await assert.rejects(loader.getSnapshot(), /offline/);
  assert.equal(loader.disabled, false);
  assert.equal(loader.cursor, null);
});
