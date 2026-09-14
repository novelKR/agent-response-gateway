import {spawn} from 'node:child_process';
import {once} from 'node:events';
import {fileURLToPath} from 'node:url';
import {join} from 'node:path';
const root=fileURLToPath(new URL('../../',import.meta.url));
export function fixturePreset(value) {
  if(!['all','usage'].includes(value))throw new Error('Use fixture preset all or usage');
  return value;
}
export async function startApiFixture({preset='all',signal}={}) {
  preset=fixturePreset(preset);
  const target=join(root,'target');
  const build=spawn('cargo',['build','--locked','--target-dir',target,'-p','gateway-management-api','--example','web_fixture'],{cwd:root,stdio:['ignore','pipe','pipe'],signal});
  build.stdout.resume();build.stderr.resume();
  const [code]=await once(build,'exit');
  if(code!==0)throw new Error('Locked API fixture build failed; inspect cargo build for the web_fixture example');
  const env={};
  for(const key of ['PATH','SystemRoot','SYSTEMROOT','WINDIR','TEMP','TMP'])if(process.env[key])env[key]=process.env[key];
  // Keep terminal signals on the launcher; the owned pipe drives graceful fixture exit.
  const child=spawn(join(target,'debug','examples',process.platform==='win32'?'web_fixture.exe':'web_fixture'),['--api-only','--preset',preset],{cwd:root,env,stdio:['pipe','pipe','pipe'],signal,detached:true,windowsHide:true});
  child.stderr.resume();
  child.stdin.on('error',()=>{});
  const exited=once(child,'exit');
  // Observe rejection even during readiness; the caller still receives the failure.
  exited.catch(()=>{});
  let stopping;
  const stop=()=>stopping??=(async()=>{
    child.stdin.end();
    let forced=false;
    const timer=setTimeout(()=>{forced=true;child.kill('SIGKILL');},5000);
    try {const [code,signal]=await exited;return {code,signal,forced};} finally {clearTimeout(timer);}
  })();
  try {
    const origin=await new Promise((resolve,reject)=>{
      let text='';
      const timer=setTimeout(()=>reject(Error('API fixture readiness timed out')),10000);
      const read=chunk=>{
        text+=chunk;
        if(text.length>4096){clearTimeout(timer);reject(Error('Invalid fixture readiness'));return;}
        if(!text.includes('\n'))return;
        clearTimeout(timer);child.stdout.off('data',read);
        const value=text.split('\n')[0].trim();
        if(!/^http:\/\/127\.0\.0\.1:[1-9][0-9]{0,4}$/.test(value)||Number(new URL(value).port)>65535)reject(Error('Invalid fixture origin'));
        else resolve(value);
      };
      child.stdout.on('data',read);
      exited.then(()=>{clearTimeout(timer);reject(Error('API fixture ended before readiness'));},error=>{clearTimeout(timer);reject(error);});
    });
    return Object.freeze({origin,preset,exited,stop});
  } catch(error) {await stop().catch(()=>{});throw error;}
}
