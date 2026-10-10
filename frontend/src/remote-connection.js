// ICE may briefly disconnect while interfaces/candidate pairs change. Screen and
// voice must not be torn down just because the optional input channel closes.
export function watchRemoteConnection(media, {
  reconnectMs=8000, setTimer=setTimeout, clearTimer=clearTimeout,
}={}) {
  let timer=null,retryTimer=null,retries=0,closed=false;
  const cancel=()=>{if(timer!==null)clearTimer(timer);timer=null;};
  const live=()=>!closed&&!media.closed;
  const release=()=>{
    media.inputSuspended=true;media.inputQueue=[];media.holdingInput=false;media.heldInputs.clear();
    if(media.session.grant)void media.signal({type:media.session.local_host?'control':'release_control',...(media.session.local_host?{allow:false}:{})}).catch(()=>{});
  };
  const retryInput=()=>{
    if(!live()||!media.session.initiator||media.pc.connectionState!=='connected'||media.channel?.readyState!=='closed'||retryTimer!==null||retries>=3)return;
    retryTimer=setTimer(()=>{
      retryTimer=null;
      if(!live()||media.pc.connectionState!=='connected'||media.channel?.readyState!=='closed')return;
      retries++;
      try{media.bindChannel(media.pc.createDataChannel('xchat-control',{ordered:true}));}
      catch{retryInput();}
    },500*2**retries);
  };
  const stateChanged=()=>{
    if(!live())return;
    const state=media.pc.connectionState;
    if(state==='connected') {
      cancel();
      retryInput();
      void media.signal({type:'ready'}).catch(error=>{if(live())media.fail(error);});
    } else if(state==='disconnected') {
      release();
      if(timer===null)timer=setTimer(()=>{
        timer=null;
        if(live()&&media.pc.connectionState==='disconnected')media.fail(new Error('远程连接已中断，请重新发起协助'));
      },reconnectMs);
    } else if(state==='failed'||state==='closed') {
      cancel();release();media.fail(new Error('远程连接已中断，请重新发起协助'));
    }
    if(live())media.changed();
  };
  media.pc.onconnectionstatechange=stateChanged;
  return {
    bind(channel) {
      const current=()=>live()&&media.channel===channel;
      channel.onopen=()=>{if(current()){retries=0;media.changed({inputError:''});}};
      channel.onclose=()=>{
        if(!current())return;
        release();
        // Revocation travels over the authenticated signalling path, which is
        // independent from SCTP. The other media tracks can remain connected.
        media.changed({inputError:'远程操作通道已断开，已收回控制；画面与语音仍可继续'});
        retryInput();
      };
    },
    dispose(){closed=true;cancel();if(retryTimer!==null)clearTimer(retryTimer);retryTimer=null;media.pc.onconnectionstatechange=null;},
  };
}
