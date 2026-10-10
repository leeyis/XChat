import test from "node:test";
import assert from "node:assert/strict";
import {remotePoint,remoteInputAllowed,remoteQuality} from "./remote-model.js";
test("remote pointer excludes letterboxing and maps the whole selected screen",()=>{
 const rect={left:20,top:50,width:1000,height:1000};
 assert.equal(remotePoint(rect,1920,1080,30,60),null);
 assert.deepEqual(remotePoint(rect,1920,1080,520,550),{x:.5,y:.5});
 assert.equal(remotePoint(rect,1920,1080,Infinity,550),null);
 assert.deepEqual(remotePoint({left:0,top:0,width:1080,height:1920},1080,1920,1080,1920),{x:1,y:1});
});
test("permission never survives pause, ending, ownership loss or revocation",()=>{
 const session={phase:"active",local_host:false,paused:false,grant:"this-session"};
 assert.equal(remoteInputAllowed(session),true);
 for(const patch of [{phase:"waiting"},{phase:"disconnected"},{local_host:true},{paused:true},{grant:null}])assert.equal(remoteInputAllowed({...session,...patch}),false);
 assert.equal(remoteInputAllowed(session,false),false);
 assert.equal(remoteQuality({fps:120,preset:"fluent"}).maxFramerate,30);
 assert.equal(remoteQuality({}).maxFramerate,30);
 for(const fps of [10,20,30,60])assert.equal(remoteQuality({fps}).maxFramerate,fps);
});
