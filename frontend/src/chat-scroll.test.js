import assert from "node:assert/strict";
import test from "node:test";
import { createChatScrollController } from "./chat-scroll.js";

function viewportFixture(count = 40, maximumRounding = 0) {
  let items = Array.from({ length: count }, (_, index) => ({ key: String(index), height: 40 }));
  let position = 0;
  const viewport = {
    clientHeight: 300,
    top: 56,
    writes: [],
    get scrollHeight() { return items.reduce((sum, item) => sum + item.height, 0); },
    get scrollTop() { return position; },
    set scrollTop(value) { position = Math.max(0, Math.min(value, this.scrollHeight - this.clientHeight - maximumRounding)); },
    getBoundingClientRect() { return { top: this.top }; },
    scrollTo(options) { this.writes.push(options); this.scrollTop = options.top; },
    querySelectorAll() {
      let offset = 0;
      return items.map((item) => {
        const start = offset;
        offset += item.height;
        return {
          dataset: { messageKey: item.key },
          getBoundingClientRect: () => ({
            top: this.top + start - this.scrollTop,
            bottom: this.top + start + item.height - this.scrollTop,
          }),
        };
      });
    },
    update(change) { items = change(items); this.scrollTop = this.scrollTop; },
    offset(key) {
      const row = this.querySelectorAll().find((item) => item.dataset.messageKey === key);
      return row.getBoundingClientRect().top - this.top;
    },
  };
  return viewport;
}

test("opening a conversation and its initially empty history follow the latest message", () => {
  const viewport = viewportFixture(0);
  const scroll = createChatScrollController();
  scroll.reconcile(viewport, "first");
  viewport.update(() => Array.from({ length: 20 }, (_, index) => ({ key: String(index), height: 40 })));
  scroll.reconcile(viewport, "first");
  assert.equal(viewport.scrollTop, 500);
  viewport.scrollTop = 80;
  scroll.capture(viewport);
  scroll.reconcile(viewport, "second");
  assert.equal(viewport.scrollTop, 500);
  assert.ok(viewport.writes.every((item) => item.behavior === "auto"));
});

test("new messages follow the bottom only when the reader was already there", () => {
  const viewport = viewportFixture();
  const scroll = createChatScrollController();
  scroll.reconcile(viewport, "chat");
  viewport.update((items) => [...items, { key: "new", height: 40 }]);
  scroll.reconcile(viewport, "chat");
  assert.equal(viewport.scrollTop, 1340);

  viewport.scrollTop = 400;
  scroll.capture(viewport);
  viewport.update((items) => [...items, { key: "another", height: 40 }]);
  scroll.reconcile(viewport, "chat");
  assert.equal(viewport.scrollTop, 400);
  assert.equal(viewport.offset("10"), 0);
});

test("loading older messages retains the same visible message and offset", () => {
  const viewport = viewportFixture();
  const scroll = createChatScrollController();
  scroll.reconcile(viewport, "chat");
  viewport.scrollTop = 405;
  scroll.capture(viewport);
  viewport.update((items) => [{ key: "older", height: 120 }, ...items]);
  scroll.reconcile(viewport, "chat");
  assert.equal(viewport.scrollTop, 525);
  assert.equal(viewport.offset("10"), -5);
});

test("transfer and image geometry changes keep a historical reading anchor", () => {
  const viewport = viewportFixture();
  const scroll = createChatScrollController();
  scroll.reconcile(viewport, "chat");
  viewport.scrollTop = 405;
  scroll.capture(viewport);
  viewport.update((items) => items.map((item) => item.key === "4" ? { ...item, height: 240 } : item));
  scroll.resize(viewport);
  assert.equal(viewport.offset("10"), -5);
  assert.equal(viewport.scrollTop, 605);
  const writes = viewport.writes.length;
  scroll.resize(viewport);
  scroll.reconcile(viewport, "chat");
  assert.equal(viewport.writes.length, writes, "unchanged progress geometry needs no scroll write");
});

test("late image loads keep a following reader at the bottom", () => {
  const viewport = viewportFixture();
  const scroll = createChatScrollController();
  scroll.reconcile(viewport, "chat");
  viewport.update((items) => items.map((item) => item.key === "4" ? { ...item, height: 240 } : item));
  scroll.resize(viewport);
  assert.equal(viewport.scrollTop, 1500);
});

test("a rounded DOM maximum does not repeatedly scroll an already pinned reader", () => {
  const viewport = viewportFixture(40, 1);
  const scroll = createChatScrollController();
  scroll.reconcile(viewport, "chat");
  assert.equal(viewport.scrollTop, 1299);
  const writes = viewport.writes.length;
  for (let update = 0; update < 20; update += 1) {
    scroll.capture(viewport);
    scroll.reconcile(viewport, "chat");
    scroll.resize(viewport);
  }
  assert.equal(viewport.writes.length, writes);
  viewport.update((items) => [...items, { key: "new", height: 40 }]);
  scroll.reconcile(viewport, "chat");
  assert.equal(viewport.scrollTop, 1339, "new messages still follow the reachable bottom");
});

test("deleted anchors fall back to a surviving visible neighbor", () => {
  const viewport = viewportFixture();
  const scroll = createChatScrollController();
  scroll.reconcile(viewport, "chat");
  viewport.scrollTop = 405;
  scroll.capture(viewport);
  const neighborOffset = viewport.offset("11");
  viewport.update((items) => items.filter((item) => item.key !== "10"));
  scroll.reconcile(viewport, "chat");
  assert.equal(viewport.offset("11"), neighborOffset);
  assert.ok(viewport.scrollTop < viewport.scrollHeight - viewport.clientHeight);
});

test("viewport resize restores the reader but follows the bottom for a pinned reader", () => {
  const viewport = viewportFixture();
  const scroll = createChatScrollController();
  scroll.reconcile(viewport, "chat");
  viewport.scrollTop = 405;
  scroll.capture(viewport);
  viewport.clientHeight = 240;
  viewport.top = 114;
  scroll.resize(viewport);
  assert.equal(viewport.offset("10"), -5);
  viewport.scrollTop = viewport.scrollHeight;
  scroll.capture(viewport);
  viewport.clientHeight = 180;
  scroll.resize(viewport);
  assert.equal(viewport.scrollTop, 1420);
});

test("a pending explicit history jump does not get pulled to the bottom", () => {
  const viewport = viewportFixture();
  const scroll = createChatScrollController();
  scroll.reconcile(viewport, "chat", { hold: true });
  scroll.resize(viewport);
  assert.equal(viewport.scrollTop, 0);
  viewport.scrollTop = 600;
  scroll.capture(viewport);
  scroll.reconcile(viewport, "chat");
  viewport.update((items) => [...items, { key: "new", height: 40 }]);
  scroll.reconcile(viewport, "chat");
  assert.equal(viewport.scrollTop, 600);
});
