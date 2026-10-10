import { useEffect, useRef, useState } from "react";
import { formatSize } from "./xchat.js";
import { StabilityIcon } from "./TaskCenter.jsx";
import StabilityDialog from "./StabilityDialog.jsx";
import "./backup.css";

const finished=job=>job&&["done","failed","cancelled"].includes(job.status);

export default function BackupPanel({ workspace }) {
  const native=Boolean(globalThis.window?.__TAURI__);
  const [overview,setOverview]=useState(null),[modal,setModal]=useState(null),[job,setJob]=useState(null),[error,setError]=useState(""),[busy,setBusy]=useState(false);
  const [attachments,setAttachments]=useState(false),[settings,setSettings]=useState(true),[restoreSettings,setRestoreSettings]=useState(false),[directory,setDirectory]=useState("");
  const alive=useRef(true),lock=useRef(false),picker=useRef(null),pollEpoch=useRef(0);
  const load=async(resume=false)=>{
    const result=await workspace.dispatch({type:"backup.overview"});if(!alive.current)return;
    if(result.ok){setOverview(result.data);if(resume&&result.data.active){setJob(result.data.active);setModal("job");}}
    else setError(result.error.message);
  };
  useEffect(()=>{alive.current=true;void load(true);return()=>{alive.current=false;pollEpoch.current++;};},[workspace]);
  useEffect(()=>{
    if(!job||job.status!=="running")return;
    const token=++pollEpoch.current;let waiting=false;
    const poll=async()=>{if(waiting)return;waiting=true;try{
      const result=await workspace.dispatch({type:"backup.status",id:job.id});
      if(!alive.current||token!==pollEpoch.current)return;
      if(result.ok){setJob(result.data);if(finished(result.data)){void load();if(result.data.status==="done"&&result.data.kind==="restore")void workspace.dispatch({type:"refresh"});}}
      else setError(result.error.message);
    }finally{waiting=false;}};
    const timer=setInterval(poll,800);void poll();return()=>{clearInterval(timer);pollEpoch.current++;};
  },[job?.id,job?.status,workspace]);
  const run=async action=>{if(lock.current)return;lock.current=true;setBusy(true);setError("");try{
    const result=await workspace.dispatch(action);if(!alive.current)return;
    if(!result.ok)throw new Error(result.error.message);
    if(result.data?.cancelled)return;
    setJob(result.data);setRestoreSettings(false);setModal("job");
  }catch(e){if(alive.current)setError(e.message);}finally{lock.current=false;if(alive.current)setBusy(false);}};
  const close=async()=>{
    if(busy)return;
    if(modal==="job"&&job&&!finished(job)){
      if(!job.cancellable)return;
      const result=await workspace.dispatch({type:"backup.cancel",id:job.id});
      if(!result.ok){setError(result.error.message);return;}
      if(alive.current){setJob(result.data);if(result.data.status!=="cancelled")return;}
    }
    setModal(null);setError("");
  };
  const prepare=backupId=>{setError("");if(backupId||native)void run({type:"backup.prepare",backupId});else picker.current.click();};
  const selectDirectory=async()=>{const result=await workspace.dispatch({type:"settings.pickPath",title:"选择备份保存目录"});if(result.ok&&result.data)setDirectory(Array.isArray(result.data)?result.data[0]:result.data);};
  const result=job?.result;
  const statusTitle=job?.status==="preview"?"确认恢复范围":job?.status==="done"?(job.kind==="create"?"备份已完成":"恢复完成"):job?.status==="failed"?"本次操作未完成":job?.status==="cancelled"?"本次操作已取消":job?.kind==="restore"?"正在校验与恢复":"正在创建备份";
  return <section className="settings-section stability-backup" id="settings-backup">
    <div className="stability-heading"><div><h2>备份与恢复</h2><p>把聊天记录和偏好保存在自己手里。</p></div><button disabled={busy||job?.status==="running"} onClick={()=>prepare()}>从备份恢复</button></div>
    {!modal&&error&&<p className="stability-error" role="alert">{error}</p>}
    <div className="backup-hero"><div><span className="backup-badge">本地备份</span><h3>给重要记录留一份副本</h3><p>选择要保存的内容，完成后检查备份是否完整。<br/>本机可用空间 {overview?.free_bytes!=null?formatSize(overview.free_bytes):"暂未获取"}</p><button className="primary-button" disabled={!overview||busy||job?.status==="running"} onClick={()=>{setError("");setModal("create");}}><StabilityIcon name="backup"/>创建备份</button></div><StabilityIcon name="backup"/></div>
    <div className="backup-list-heading"><h3>备份记录</h3><small>保存在本机 · {overview?.backups.length||0} 份</small></div>
    {overview?.backups.map(record=><article className="backup-record" key={record.id}><StabilityIcon name="backup"/><div><b>{record.name}</b><small>聊天记录{record.settings?"、偏好设置":""}{record.attachments?"、本地附件":""} · {record.available?"已完成完整性校验":"文件已移动或删除"}</small></div><span>{new Date(record.created_at*1000).toLocaleDateString()}<small>{formatSize(record.bytes)}</small></span><div className="stability-actions"><button disabled={!record.available||busy||job?.status==="running"} onClick={()=>prepare(record.id)}>恢复预览</button>{!native&&record.available&&<a href={`/api/backups/${encodeURIComponent(record.id)}/download`} download>下载</a>}</div></article>)}
    {overview&&!overview.backups.length&&<p className="backup-empty">还没有备份，创建后会显示在这里。</p>}
    <div className="backup-notes"><div><h3>哪些内容可以备份？</h3><p>聊天记录、联系人备注和偏好设置。已保存在本机的附件可单独选择；未下载的文件不会出现在备份中。</p></div><div><h3>恢复前先确认</h3><p>先校验，再预览新增记录。已有记录保留，重复项跳过；恢复的历史不会自动重发。</p></div></div>
    <input hidden type="file" accept=".xchatbackup" ref={picker} onChange={event=>{const file=event.target.files?.[0];event.target.value="";if(file)void run({type:"backup.prepare",file});}}/>
    {modal==="create"&&<StabilityDialog title="创建本地备份" onClose={busy?undefined:close} actions={<><button disabled={busy} onClick={close}>取消</button><button className="primary-button" disabled={busy} onClick={()=>run({type:"backup.create",request:{include_attachments:attachments,include_settings:settings,directory:native?(directory||overview.directory):null}})}>开始备份</button></>}>
      <p>选择本次需要保存的内容。</p><label className="backup-option"><input type="checkbox" checked disabled/><span>聊天记录与联系人备注<small>{overview.messages.toLocaleString()} 条记录 · 数据库约 {formatSize(overview.database_bytes)}</small></span></label>
      <label className="backup-option"><input type="checkbox" checked={settings} onChange={e=>setSettings(e.target.checked)}/><span>偏好设置<small>不包含设备身份、快捷键和远程授权。</small></span></label>
      <label className="backup-option"><input type="checkbox" checked={attachments} onChange={e=>setAttachments(e.target.checked)}/><span>本机已保存的附件<small>{overview.attachments} 个文件 · 额外约 {formatSize(overview.attachment_bytes)}</small></span></label>
      <div className="backup-location"><span>保存位置</span><p>{native?(directory||overview.directory):"先保存在服务所在电脑，完成后可下载到此浏览器。"}</p>{native&&<button onClick={selectDirectory}>更换位置</button>}</div>
      <p className="backup-muted">备份可能包含私人聊天内容，请妥善保管。</p>{error&&<p role="alert" className="stability-error">{error}</p>}
    </StabilityDialog>}
    {modal==="job"&&job&&<StabilityDialog title={statusTitle} onClose={!busy&&(job.cancellable||finished(job))?close:undefined} actions={job.status==="preview"?<><button disabled={busy} onClick={close}>取消</button><button className="primary-button" disabled={busy} onClick={()=>run({type:"backup.restore",id:job.id,includeSettings:restoreSettings})}>确认合并恢复</button></>:finished(job)?<><button onClick={close}>完成</button>{job.status==="done"&&job.kind==="create"&&!native&&<a className="primary-button" href={`/api/backups/${encodeURIComponent(result.backup_id)}/download`} download>下载备份</a>}</>:<button disabled={!job.cancellable||busy} onClick={close}>{job.cancellable?"取消本次操作":"正在合并，请稍候"}</button>}>
      {job.status==="preview"?<><div className="backup-success"><b>备份完整，可以恢复</b><p>创建于 {new Date(result.created_at*1000).toLocaleString()}</p></div><dl className="backup-preview"><dt>新增聊天记录</dt><dd>{result.new_messages} 条</dd><dt>已存在的记录</dt><dd>{result.duplicates} 条 · 跳过重复</dd><dt>当前记录</dt><dd>全部保留</dd><dt>本地附件</dt><dd>{result.attachments} 个</dd></dl><label className="backup-option"><input type="checkbox" checked={restoreSettings} disabled={!result.settings} onChange={e=>setRestoreSettings(e.target.checked)}/><span>同时恢复偏好设置<small>{result.settings?"默认保留当前设置；设备身份、快捷键和远程授权不迁移。":"这份备份未包含偏好设置。"}</small></span></label><p className="backup-muted">恢复前会自动保留当前数据副本。历史消息不会自动重新发送。</p></>:job.status==="done"?<div className="backup-success"><b>{job.kind==="create"?"已通过完整性校验":`已新增 ${result.added} 条记录`}</b><p>{job.kind==="create"?`${result.messages} 条聊天记录 · ${formatSize(result.bytes)}`:`${result.duplicates} 条重复记录已跳过，原有记录均保留。`}</p><p>{job.kind==="create"?result.path:result.preferences_restored?"已恢复选定偏好；本机设备身份与快捷键保持原样。":"当前偏好设置保持原样。"}</p>{job.kind==="restore"&&<><p>恢复前的数据副本已保留，历史消息不会自动重发。</p><small>{result.pre_restore_copy}</small></>}</div>:job.status==="failed"||job.status==="cancelled"?<p className="stability-error" role="alert">{job.error||"操作已取消，当前聊天记录未改变。"}</p>:<><p role="status">{job.phase}…</p><progress className="backup-progress" max={job.total_bytes||1} value={job.total_bytes?Math.min(job.completed_bytes,job.total_bytes):undefined}/><p className="backup-muted">{job.total_bytes?`${formatSize(job.completed_bytes)} / ${formatSize(job.total_bytes)}`:"正在处理，请稍候"}</p><p>{job.kind==="create"?"完成并校验后才会加入备份记录。":"校验失败不会更改当前聊天记录；合并期间请保持 XChat 开启。"}</p></>}
      {error&&<p role="alert" className="stability-error">{error}</p>}
    </StabilityDialog>}
  </section>;
}
