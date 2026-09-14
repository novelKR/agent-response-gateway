import {parseArgs} from 'node:util';
import {startFixtureDemo} from './fixture-demo.mjs';
const {values}=parseArgs({options:{port:{type:'string',default:'43142'},preset:{type:'string',default:'all'},'parent-stdin':{type:'boolean',default:false}}});
const controller=new AbortController();
let running,closing=false,shutdown;
function close(){return shutdown??=(async()=>{closing=true;const result=await running?.close();controller.abort();return result?.unexpected?1:0;})();}
process.on('SIGINT',()=>close().then(code=>process.exit(code)));
process.on('SIGTERM',()=>close().then(code=>process.exit(code)));
if(values['parent-stdin']){process.stdin.resume();process.stdin.once('end',()=>close().then(code=>process.exit(code)));}
try {
  console.log('Building the locked API fixture...');
  running=await startFixtureDemo({port:values.port,preset:values.preset,signal:controller.signal});
  console.log('DevDemo API fixture: '+running.url);
  const result=await running.finished;
  if(result.unexpected){console.error('The owned API fixture stopped. DevDemo has closed without a synthetic fallback.');process.exitCode=1;}
} catch {if(!closing){console.error('API fixture mode could not start. Check Rust, the preset and the available loopback port.');process.exitCode=1;}await close();}
