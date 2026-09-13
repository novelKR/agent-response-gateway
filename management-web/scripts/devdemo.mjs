import {parseArgs} from 'node:util';
import {startDemoServer} from './devdemo-server.mjs';
const {values}=parseArgs({options:{port:{type:'string',default:'43142'},'parent-stdin':{type:'boolean',default:false}}});
let server,closing=false;
async function close(){if(closing)return;closing=true;await server?.close();}
process.on('SIGINT',()=>close().then(()=>process.exit(0)));
process.on('SIGTERM',()=>close().then(()=>process.exit(0)));
try {
  server=await startDemoServer({port:values.port});
  console.log('DevDemo synthetic: '+server.url);
  if(values['parent-stdin']){process.stdin.resume();process.stdin.once('end',()=>close().then(()=>process.exit(0)));}
} catch {console.error('DevDemo could not start. Check the pinned Node version and available loopback port.');process.exitCode=1;await close();}
