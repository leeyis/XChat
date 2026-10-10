import test from "node:test";
import assert from "node:assert/strict";
import {taskState, taskSummary, taskCanCancel, taskCanRetry} from "./task-model.js";

test("partial group delivery only exposes unfinished work and preserves cancelled terminal state",()=>{
  const task={message:{sender_id:"self",msg_type:"text"},recipients:[{state:"completed"},{state:"unconfirmed"},{state:"cancelled",delivered_at:123}]};
  assert.equal(taskState(task),"attention");assert.equal(taskSummary(task),"部分送达 · 1 / 3");
  assert.ok(taskCanCancel(task));assert.ok(taskCanRetry(task,"self"));
  task.recipients[1].state="completed";
  assert.equal(taskState(task),"finished");assert.equal(taskCanCancel(task),false);
  assert.equal(taskCanRetry(task,"self"),false);
});

test("file retry cannot race an active recipient or operate on received files",()=>{
  const task={message:{sender_id:"self",msg_type:"voice",file_status:"failed"},recipients:[{state:"failed"},{state:"transferring"}]};
  assert.equal(taskCanRetry(task,"self"),false);
  task.recipients[1].state="completed";assert.ok(taskCanRetry(task,"self"));
  assert.equal(taskCanRetry(task,"recipient"),false);
});
