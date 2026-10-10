const finite=value=>typeof value==="number"&&Number.isFinite(value);
const difference=(current,prior,key)=>finite(current?.[key])&&finite(prior?.[key])&&current[key]>=prior[key]?current[key]-prior[key]:null;
const milliseconds=(total,count)=>total!=null&&count>0?1000*total/count:null;

// Use RTP counters and browser measurements, never the requested frame rate or
// capture-loop frequency. Missing hardware/delay information stays unknown.
export function remoteMediaMetrics(reports,localHost,previous) {
  const all=Array.from(reports.values()),direction=localHost?"outbound-rtp":"inbound-rtp";
  const streams=all.filter(item=>item.type===direction&&(item.kind||item.mediaType)==="video"&&!item.isRemote);
  const video=streams.sort((a,b)=>(b.bytesSent??b.bytesReceived??0)-(a.bytesSent??a.bytesReceived??0))[0];
  const transport=video?.transportId?reports.get(video.transportId):all.find(item=>item.type==="transport"&&item.selectedCandidatePairId);
  const pair=(transport?.selectedCandidatePairId&&reports.get(transport.selectedCandidatePairId))||all.find(item=>item.type==="candidate-pair"&&item.state==="succeeded"&&item.nominated);
  const local=pair&&reports.get(pair.localCandidateId),remote=pair&&reports.get(pair.remoteCandidateId);
  const codec=video&&reports.get(video.codecId);
  const prior=previous?.id===video?.id?previous:null;
  const duration=prior&&finite(video?.timestamp)&&video.timestamp>prior.timestamp?video.timestamp-prior.timestamp:null;
  const bytesKey=localHost?"bytesSent":"bytesReceived",framesKey=localHost?"framesEncoded":"framesDecoded";
  const frames=difference(video,prior,framesKey),bytes=difference(video,prior,bytesKey);
  const packets=difference(video,prior,"packetsReceived"),lost=difference(video,prior,"packetsLost");
  const info={
    fps:finite(video?.framesPerSecond)?video.framesPerSecond:duration&&frames!=null?Math.round(frames*10000/duration)/10:null,
    rtt:finite(pair?.currentRoundTripTime)?Math.round(pair.currentRoundTripTime*1000):null,
    width:video?.frameWidth??null,height:video?.frameHeight??null,bytes:video?.[bytesKey]??null,
    kbps:duration&&bytes!=null?Math.round(bytes*8/duration):null,
    codec:codec?.mimeType?.replace(/^video\//i,"")??null,
    codecParameters:codec?.sdpFmtpLine??null,
    encoderImplementation:video?.encoderImplementation??null,decoderImplementation:video?.decoderImplementation??null,
    powerEfficientEncoder:typeof video?.powerEfficientEncoder==="boolean"?video.powerEfficientEncoder:null,
    powerEfficientDecoder:typeof video?.powerEfficientDecoder==="boolean"?video.powerEfficientDecoder:null,
    encodeMs:milliseconds(difference(video,prior,"totalEncodeTime"),frames),
    decodeMs:milliseconds(difference(video,prior,"totalDecodeTime"),frames),
    jitterBufferMs:milliseconds(difference(video,prior,"jitterBufferDelay"),difference(video,prior,"jitterBufferEmittedCount")),
    processingMs:milliseconds(difference(video,prior,"totalProcessingDelay"),frames),
    lossPercent:packets!=null&&lost!=null&&packets+lost>0?Math.round(lost*10000/(packets+lost))/100:null,
    protocol:local?.protocol??remote?.protocol??null,
    path:local&&remote?(local.candidateType==="relay"||remote.candidateType==="relay"?"relay":"direct"):null,
    localAddress:local?.address??local?.ip??null,remoteAddress:remote?.address??remote?.ip??null,
    dtlsState:transport?.dtlsState??null,dtlsCipher:transport?.dtlsCipher??null,srtpCipher:transport?.srtpCipher??null,
    qualityLimitationReason:video?.qualityLimitationReason??null,
    framesDropped:video?.framesDropped??null,
  };
  return {info,sample:video?{...video}:null};
}
