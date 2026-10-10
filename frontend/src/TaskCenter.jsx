import { useCallback, useEffect, useRef, useState } from "react";
import { formatSize, conversationPreview } from "./xchat.js";
import { TASK_FINISHED, TASK_LABELS, taskState, taskSummary, taskCanCancel, taskCanRetry } from "./task-model.js";
import "./stability.css";

export function StabilityIcon({ name="tasks" }) {
  return <svg viewBox="0 0 24 24" width="22" height="22" aria-hidden="true" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round">{name === "tasks" ? <><rect x="8" y="2" width="8" height="4" rx="1.5"/><path d="M8 4H6a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V6a2 2 0 0 0-2-2h-2m-8 10 3 3 5-6"/></> : name === "diagnostics" ? <path d="M2 12h4l3-8 6 16 3-8h4"/> : <><path d="M6 3h8l4 4v14H6zM14 3v5h4M9 13h6M9 17h4"/></>}</svg>;
}

export default function TaskCenter({ state, workspace, onConversation, onDiagnostics }) {
  const [tasks,setTasks] = useState([]), [cursor,setCursor] = useState(null), [kind,setKind] = useState("all"), [filter,setFilter] = useState("all"), [query,setQuery] = useState("");
  const [selected,setSelected] = useState(null), [checked,setChecked] = useState([]), [busy,setBusy] = useState(false), [error,setError] = useState("");
  const lock=useRef(false), alive=useRef(true), picker=useRef(null), pickTask=useRef(null), sequence=useRef(0), loadedPages=useRef(1);
  const load=useCallback(async(before=null)=>{
    const token=++sequence.current;
    const result=await workspace.dispatch({type:"tasks.list",before});
    if(!alive.current || token!==sequence.current)return;
    if(!result.ok){setError(result.error.message);return;}
    const page={tasks:[...result.data.tasks],next_before:result.data.next_before};
    if(!before)for(let index=1;index<loadedPages.current&&page.next_before;index++){
      const next=await workspace.dispatch({type:"tasks.list",before:page.next_before});
      if(!alive.current||token!==sequence.current)return;
      if(!next.ok){setError(next.error.message);return;}
      page.tasks.push(...next.data.tasks);page.next_before=next.data.next_before;
    }
    if(before)loadedPages.current++;
    setTasks(current=>before ? [...current.filter(t=>!page.tasks.some(n=>n.message.id===t.message.id)),...page.tasks] : page.tasks);
    setCursor(page.next_before);setError("");
  },[workspace]);
  useEffect(()=>{alive.current=true;void load();const timer=setInterval(()=>{if(!document.hidden&&!lock.current)void load();},4000);return()=>{alive.current=false;sequence.current++;clearInterval(timer);};},[load]);
  useEffect(()=>{const close=event=>{if(event.key==="Escape")setSelected(null);};document.addEventListener("keydown",close);return()=>document.removeEventListener("keydown",close);},[]);
  const act=async(task,action,peerId=null)=>{
    const result=await workspace.dispatch({type:"tasks.act",request:{message_id:task.message.id,action,peer_id:peerId}});
    if(!result.ok)throw new Error(result.error.message);
  };
  const run=async fn=>{if(lock.current)return;lock.current=true;setBusy(true);setError("");try{await fn();await load();}catch(e){setError(e.message);}finally{lock.current=false;if(alive.current)setBusy(false);}};
  const replace=task=>{pickTask.current=task;if(state.runtime==="tauri" || globalThis.__TAURI__)void run(async()=>{const result=await workspace.dispatch({type:"tasks.replaceSource",messageId:task.message.id});if(!result.ok)throw new Error(result.error.message);});else picker.current.click();};
  const title=task=>task.message.msg_type==="voice" ? "语音消息" : conversationPreview(task.message.content);
  const recipient=task=>state.conversations.find(c=>c.id===task.message.conversation_id)?.title || task.recipients[0]?.name || "未知会话";
  const visible=tasks.filter(t=>(kind==="all"||(kind==="file")===["file","voice"].includes(t.message.msg_type))&&(filter==="all"||(filter==="active" ? ["running","waiting"].includes(taskState(t)) : filter===taskState(t)))&&`${title(t)} ${recipient(t)} ${t.recipients.map(r=>r.name).join(" ")}`.toLocaleLowerCase().includes(query.trim().toLocaleLowerCase()));
  const detail=tasks.find(t=>t.message.id===selected);
  const cancel=items=>{if(!items.length)return;void run(async()=>{for(const task of items)await act(task,"cancel");setChecked([]);});};
  const operation=(task,compact=false)=><div className="stability-actions" onClick={e=>e.stopPropagation()}>
    {taskCanRetry(task,state.self.id)&&<button disabled={busy} onClick={()=>run(()=>act(task,"retry"))}>{["file","voice"].includes(task.message.msg_type)?"重试":"继续确认"}</button>}
    {["file","voice"].includes(task.message.msg_type)&&taskCanRetry(task,state.self.id)&&<button disabled={busy} onClick={()=>replace(task)}>重新选择</button>}
    {taskCanCancel(task)&&<button disabled={busy} onClick={()=>cancel([task])}>{task.recipients.length>1?"取消未送达":"取消"}</button>}
    {!compact&&<button onClick={()=>onConversation(task.message.conversation_id,{targetMessageId:task.message.id})}>查看会话</button>}
  </div>;
  return <div className="task-center">
    <aside className="task-sidebar"><h2>任务中心</h2><div className="task-sidebar-content"><h3>每一次发送，都有着落</h3><p>汇总来自所有会话的任务</p>{[["all","全部任务"],["message","消息"],["file","文件与语音"]].map(([id,label])=><button key={id} className={kind===id?"selected":""} onClick={()=>setKind(id)}><StabilityIcon name={id==="all"?"tasks":"file"}/>{label}</button>)}<hr/><b>任务会被保留</b><p>退出后重新打开 XChat，可以继续未完成的发送。</p><p>对方离线时会等待上线，无需反复点击发送。</p><button className="text-action" onClick={onDiagnostics}>打开连接诊断</button></div></aside>
    <main className="task-main"><header className="stability-heading"><div><h2>任务中心</h2><p>查看发送状态，处理需要你介入的任务。</p></div><button disabled={busy||!tasks.some(t=>taskState(t)==="finished")} onClick={()=>run(async()=>{for(const task of tasks.filter(t=>taskState(t)==="finished"))await act(task,"dismiss");setTasks(t=>t.filter(item=>taskState(item)!=="finished"));})}>清理已结束记录</button></header>
      <div className="task-stats">{[["running","正在传输"],["waiting","等待连接"],["attention","需要关注"],["finished","已结束"]].map(([id,label])=><button key={id} onClick={()=>setFilter(id)}><b className={id==="attention"?"danger-text":""}>{tasks.filter(t=>taskState(t)===id).length}</b><span>{label}</span></button>)}</div>
      <div className="task-filter">{[["all","全部"],["active","进行中"],["attention","需要关注"],["finished","已结束"]].map(([id,label])=><button key={id} aria-pressed={filter===id} onClick={()=>setFilter(id)}>{label}</button>)}<input aria-label="搜索任务" placeholder="文件、内容或设备" value={query} onChange={e=>setQuery(e.target.value)}/></div>
      {error&&<div className="stability-error" role="alert">{error}</div>}
      <div className={`task-body${detail?" with-detail":""}`}><div className="task-table"><div className="task-table-head"><span>任务</span><span>发送给 / 来自</span><span>当前状态</span><span>操作</span></div>{visible.map(task=><article className={`task-row${selected===task.message.id?" selected":""}`} key={task.message.id}>
        <div className="task-title"><input type="checkbox" aria-label={`选择 ${title(task)}`} checked={checked.includes(task.message.id)} onChange={e=>setChecked(ids=>e.target.checked?[...ids,task.message.id]:ids.filter(id=>id!==task.message.id))}/><button onClick={()=>setSelected(task.message.id)}><span className="task-file-icon"><StabilityIcon name="file"/></span><span><b>{title(task)}</b><small>{task.message.msg_type==="voice"?"语音":task.message.msg_type==="file"?"文件":"消息"} · {new Date(task.message.timestamp*1000).toLocaleString()}</small></span></button></div>
        <button className="task-recipient" onClick={()=>setSelected(task.message.id)}><b>{recipient(task)}</b><small>{task.recipients.length} 位收件人</small></button>
        <button className={`task-state ${taskState(task)}`} onClick={()=>setSelected(task.message.id)}><b>• {taskSummary(task)}</b>{task.message.file_size>0&&<><progress max={task.recipients.reduce((n,r)=>n+r.bytes_total,0)||1} value={task.recipients.reduce((n,r)=>n+r.bytes_transferred,0)}/><small>{formatSize(task.recipients.reduce((n,r)=>n+r.bytes_transferred,0))} / {formatSize(task.recipients.reduce((n,r)=>n+r.bytes_total,0))}</small></>}{!task.source_available&&task.message.msg_type==="file"&&taskCanRetry(task,state.self.id)&&<small>源文件暂不可用</small>}</button>{operation(task,true)}
      </article>)}{!visible.length&&<p className="stability-empty">{query||filter!=="all"?"没有符合条件的任务":"暂时没有发送任务"}</p>}{cursor&&<button className="task-more" disabled={busy} onClick={()=>run(()=>load(cursor))}>加载更早任务</button>}</div>
      {detail&&<aside className="task-detail"><header><h3>任务详情</h3><button aria-label="关闭任务详情" onClick={()=>setSelected(null)}>×</button></header><p className="task-detail-state">{taskSummary(detail)}</p><h3>{title(detail)}</h3><p>继续发送只处理未送达对象。取消会停止后续尝试，已写出的内容可能仍被对方接收。</p><h4>收件人进度</h4>{detail.recipients.map((row,index)=><div className="task-recipient-detail" key={row.transfer_id||`${row.peer_id}:${index}`}><b>{row.name}</b><span>{TASK_LABELS[row.state]||row.state}</span>{row.error&&<small className="danger-text">{row.error}</small>}{row.state==="cancelled"&&row.delivered_at&&<small>取消后收到送达回执；内容已在对方设备上</small>}{row.attempt_count>0&&<small>已尝试 {row.attempt_count} 次{row.next_retry_at>0&&!TASK_FINISHED.has(row.state)?` · 下次 ${new Date(row.next_retry_at*1000).toLocaleTimeString()}`:""}</small>}</div>)}{operation(detail)}</aside>}
      </div><footer className="task-footer"><span>{checked.length?`已选择 ${checked.length} 项`:`已加载 ${tasks.length} 项 · 进度以对方确认的数据为准`}</span>{checked.length>0&&<div className="stability-actions"><button disabled={busy||!tasks.some(t=>checked.includes(t.message.id)&&taskCanRetry(t,state.self.id))} onClick={()=>run(async()=>{for(const task of tasks.filter(t=>checked.includes(t.message.id)&&taskCanRetry(t,state.self.id)))await act(task,"retry");setChecked([]);})}>继续可恢复项</button><button disabled={busy} onClick={()=>cancel(tasks.filter(t=>checked.includes(t.message.id)&&taskCanCancel(t)))}>取消未完成项</button></div>}</footer>
    </main><input hidden type="file" ref={picker} onChange={e=>{const file=e.target.files?.[0],task=pickTask.current;e.target.value="";if(file&&task)void run(async()=>{const result=await workspace.dispatch({type:"tasks.replaceSource",messageId:task.message.id,file});if(!result.ok)throw new Error(result.error.message);});}}/>
  </div>;
}
