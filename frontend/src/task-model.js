export const TASK_FINISHED = new Set(["completed", "delivered", "read", "cancelled", "rejected", "expired"]);
export const TASK_LABELS = { queued:"等待发送",waiting_peer:"等待上线",waiting_connection:"等待连接",sending:"正在发送",transferring:"正在传输",receiving:"正在接收",uploading:"正在发送",downloading:"正在接收",offering:"等待接收",awaiting_acceptance:"等待接收",awaiting_ack:"等待确认",unconfirmed:"待确认送达",failed:"需要处理",cancelled:"已取消",cancelling:"正在取消",completed:"已完成",delivered:"已送达",read:"已读",rejected:"已拒绝",expired:"已过期" };
export function taskState(task) {
  const rows = task.recipients || [];
  if (!rows.length) return "finished";
  if (rows.every(r => TASK_FINISHED.has(r.state))) return "finished";
  if (rows.some(r => ["failed","unconfirmed"].includes(r.state))) return "attention";
  if (rows.some(r => ["transferring","sending","receiving","uploading","downloading"].includes(r.state))) return "running";
  return "waiting";
}
export function taskSummary(task) {
  const rows = task.recipients || [];
  const delivered = rows.filter(r => ["completed","delivered","read"].includes(r.state)).length;
  if (rows.length > 1 && delivered > 0 && delivered < rows.length) return `部分送达 · ${delivered} / ${rows.length}`;
  if (rows.every(r => r.state === "cancelled")) return "已取消";
  return taskState(task) === "finished" ? "已结束" : TASK_LABELS[rows.find(r => !TASK_FINISHED.has(r.state))?.state] || "等待处理";
}
export function taskCanCancel(task) { return task.recipients?.some(r => !TASK_FINISHED.has(r.state) && !["failed","cancelling"].includes(r.state)); }
export function taskCanRetry(task, selfId) {
  if (![selfId,"me"].includes(task.message.sender_id)) return false;
  if (["file","voice"].includes(task.message.msg_type)) return ["failed","cancelled"].includes(task.message.file_status) && task.recipients.every(r => TASK_FINISHED.has(r.state) || r.state === "failed");
  return task.recipients.some(r => !r.delivered_at && ["waiting_connection","unconfirmed","failed","cancelled"].includes(r.state));
}
