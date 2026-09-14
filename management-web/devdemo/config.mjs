export const fixtureReadCredential='synthetic-browser-read-key-01234567890123456789';
export async function loadDemoConfig(signal) {
  const controller=new AbortController(),timer=setTimeout(()=>controller.abort(),5000);
  const abort=()=>controller.abort();
  signal?.addEventListener('abort',abort,{once:true});
  if(signal?.aborted)controller.abort();
  try {
    const response=await fetch('/__devdemo/config',{cache:'no-store',redirect:'error',signal:controller.signal});
    if(!response.ok)throw Error('DevDemo configuration unavailable');
    const value=await response.json();
    if(value?.schema!=='gateway-devdemo/v1'||!(value.mode==='synthetic'&&value.preset===null||value.mode==='fixture'&&['all','usage'].includes(value.preset)))throw Error('Unsupported DevDemo configuration');
    return Object.freeze(value);
  } finally {clearTimeout(timer);signal?.removeEventListener('abort',abort);}
}
