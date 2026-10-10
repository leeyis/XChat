import { useEffect, useRef, useState } from "react";
import { formatSize } from "./xchat.js";
import { StabilityIcon } from "./TaskCenter.jsx";

export function downloadReport(value, name) {
  const url=URL.createObjectURL(new Blob([JSON.stringify(value,null,2)],{type:"application/json"}));
  const link=document.createElement("a");link.href=url;link.download=name;link.click();setTimeout(()=>URL.revokeObjectURL(url),1000);
}

export default function ConnectionDiagnostics({ state, workspace }) {
  const [peer,setPeer]=useState(""),[addresses,setAddresses]=useState(false),[busy,setBusy]=useState(false),[report,setReport]=useState(null),[error,setError]=useState("");
  const epoch=useRef(0),alive=useRef(true);
  useEffect(()=>{alive.current=true;return()=>{alive.current=false;epoch.current++;};},[]);
  const check=async()=>{const token=++epoch.current;setBusy(true);setError("");const result=await workspace.dispatch({type:"diagnostics.run",request:{peer_id:peer||null,include_addresses:addresses}});if(!alive.current||token!==epoch.current)return;setBusy(false);if(result.ok)setReport(result.data);else setError(result.error.message);};
  const change=fn=>{epoch.current++;setBusy(false);setReport(null);setError("");fn();};
  const hasError=report?.checks.some(c=>c.state==="error"),hasWarning=report?.checks.some(c=>["warning","unknown"].includes(c.state));
  return <section className="settings-section stability-diagnostics" id="settings-diagnostics">
    <div className="stability-heading"><div><h2>连接诊断</h2><p>分开检查发现、连通和设备身份，找到消息或文件无法送达的原因。</p></div><button disabled={!report||busy} onClick={()=>downloadReport(report,`XChat-连接诊断-${report.checked_at}.json`)}>导出诊断</button></div>
    <div className="diagnostic-controls"><label>检查对象<select value={peer} disabled={busy} onChange={e=>change(()=>setPeer(e.target.value))}><option value="">本机网络与存储</option>{state.devices.map(d=><option key={d.id} value={d.id}>{d.remark||d.name||d.id}</option>)}</select></label><button className="primary-button" disabled={busy} onClick={check}>{busy?"正在检查…":"重新检查"}</button></div>
    <label className="diagnostic-privacy"><input type="checkbox" checked={addresses} disabled={busy} onChange={e=>change(()=>setAddresses(e.target.checked))}/>显示并导出网络地址（默认隐藏）</label>
    {error&&<p className="stability-error" role="alert">{error}</p>}
    <div className={`diagnostic-summary ${hasError?"error":hasWarning?"warning":""}`}><StabilityIcon name="diagnostics"/><div><b>{busy?"正在检查连接":!report?"准备好后开始检查":hasError?"发现需要处理的问题":hasWarning?"检查完成，有项目需要留意":"检查完成，当前状态正常"}</b><p>{report?`检查于 ${new Date(report.checked_at*1000).toLocaleString()} · ${addresses?"包含网络地址":"已隐藏网络地址"}`:"检查不会发送聊天内容，也不会修改防火墙设置。"}</p></div></div>
    {report&&<><div className="diagnostic-checks">{report.checks.map((check,index)=><article key={check.key}><i className={check.state}>{check.state==="ok"?"✓":check.state==="error"?"!":index+1}</i><div><h3>{check.label}</h3><p>{check.detail}</p>{check.key==="storage"&&report.free_bytes!=null&&<small>可用空间 {formatSize(report.free_bytes)}</small>}</div><span>{({ok:"正常",warning:"留意",error:"需处理",unknown:"未确认"})[check.state]}</span></article>)}</div><details className="diagnostic-details"><summary>查看检查详情</summary>{report.endpoint&&<p>当前地址：{report.endpoint}</p>}<p>{report.interfaces.length} 个网络接口 · {report.interfaces.filter(i=>i.enabled).length} 个参与自动发现</p>{report.interfaces.map((item,index)=><div key={index}>{item.name||`接口 ${index+1}`} · {({physical_lan:"局域网",mesh_vpn:"组网 VPN",proxy_tun:"代理虚拟网卡",virtual_machine:"虚拟机网卡",unknown:"其他"})[item.category]||item.category} · {item.enabled?"参与发现":"未参与发现"}{item.addresses?.map(a=><span key={a.ipv4}> · {a.ipv4}/{a.prefix_length}</span>)}</div>)}</details></>}
    <p className="diagnostic-note">设备身份不一致时停止发送，请核对联系人与固定地址。诊断报告只包含检查结果，不包含聊天正文、文件内容或远程授权。</p>
  </section>;
}
