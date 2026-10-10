// Host-reported capture diagnostics stay distinct from the receiver's measured
// RTP statistics. Unknown keys can never overwrite FPS, RTT, loss or encryption.
const fields={captureMs:"hostCaptureMs",bridgeMs:"hostBridgeMs",canvasMs:"hostCanvasMs",encodeMs:"hostEncodeMs"};
function hostInfo(value) {
  if(!value||typeof value!=="object")return null;
  const result={};
  for(const [key,limit] of Object.entries({os:64,os_version:128,architecture:64,client_version:32,cpu:256})){
    if(typeof value[key]==="string"&&value[key].length<=limit)result[key]=value[key];
  }
  if(Number.isSafeInteger(value.memory_bytes)&&value.memory_bytes>0)result.memory_bytes=value.memory_bytes;
  return Object.keys(result).length&&JSON.stringify(result).length<=1024?result:null;
}
export function readRemoteTelemetry(data) {
  if(typeof data!=="string"||data.length>2048)return null;
  let message;try{message=JSON.parse(data);}catch{return null;}
  if(!message||message.type!=="xchat-metrics"||message.version!==1||!message.metrics||typeof message.metrics!=="object")return null;
  const source=message.metrics,result={};
  if(["DXGI","GDI"].includes(source.captureBackend))result.hostCaptureBackend=source.captureBackend;
  for(const [key,target] of Object.entries(fields))if(Number.isFinite(source[key])&&source[key]>=0&&source[key]<=60000)result[target]=source[key];
  for(const key of ["sourceWidth","sourceHeight"])if(Number.isInteger(source[key])&&source[key]>0&&source[key]<=32768)result[`host${key[0].toUpperCase()}${key.slice(1)}`]=source[key];
  if(typeof source.codec==="string"&&source.codec.length<=32)result.hostCodec=source.codec;
  if(typeof source.encoderImplementation==="string"&&source.encoderImplementation.length<=100)result.hostEncoderImplementation=source.encoderImplementation;
  if(typeof source.powerEfficientEncoder==="boolean")result.hostPowerEfficientEncoder=source.powerEfficientEncoder;
  const system=hostInfo(message.hostInfo);
  if(system)result.hostInfo=system;
  return result;
}
export function remoteTelemetry(metrics={},localInfo) {
  const keys=["captureBackend","sourceWidth","sourceHeight",...Object.keys(fields),"codec","encoderImplementation","powerEfficientEncoder"];
  const selected={};
  for(const key of keys){
    const value=metrics[key];
    if(typeof value==="string"&&value.length<=(key==="encoderImplementation"?100:32))selected[key]=value;
    else if(typeof value==="number"&&Number.isFinite(value)&&value>=0&&value<=60000)selected[key]=value;
    else if(key==="powerEfficientEncoder"&&typeof value==="boolean")selected[key]=value;
  }
  const packet={type:"xchat-metrics",version:1,metrics:selected,hostInfo:hostInfo(localInfo)};
  if(JSON.stringify(packet).length>2048)packet.hostInfo=null;
  return JSON.stringify(packet);
}
