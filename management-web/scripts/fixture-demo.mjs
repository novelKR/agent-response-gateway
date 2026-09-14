import {startApiFixture} from './api-fixture.mjs';
import {startDemoServer} from './devdemo-server.mjs';
export async function startFixtureDemo({port=43142,preset='all',signal}={}) {
  const fixture=await startApiFixture({preset,signal});
  let server;
  try {server=await startDemoServer({port,fixture});}
  catch(error){await fixture.stop();throw error;}
  let closing=false,resolveFinished;
  const finished=new Promise(resolve=>{resolveFinished=resolve;});
  let shutdown;
  const close=(unexpected=false)=>shutdown??=(async()=>{
    closing=true;
    const results=await Promise.allSettled([server.close(),fixture.stop()]);
    const failed=results.some(result=>result.status==='rejected')||results[1].value?.code!==0||results[1].value?.forced;
    const result={unexpected:unexpected||Boolean(failed)};
    resolveFinished(result);return result;
  })();
  fixture.exited.then(()=>{if(!closing)close(true);},()=>{if(!closing)close(true);});
  if(signal?.aborted)await close();
  return Object.freeze({url:server.url,fixture,finished,close:()=>close()});
}
