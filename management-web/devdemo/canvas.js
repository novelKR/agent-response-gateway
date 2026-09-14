import {createApp} from 'vue';
import App from '../src/App.vue';
import {createClient} from '../src/api.mjs';
import '../src/style.css';
import {baseTime,demoSchema,scenarioOf,syntheticTransport,pages} from './scenarios.mjs';
import {loadDemoConfig} from './config.mjs';
let app,view,config;
const configRequest=new AbortController();
function receive(event) {
  if(event.origin!==location.origin||event.source!==parent||parent===window)return;
  const value=event.data;
  if(!config||value?.schema!==demoSchema||!['mount','present'].includes(value.kind)||!['en','ko'].includes(value.language)||!['light','dark','system'].includes(value.theme)||!pages.includes(value.page))return;
  if(config.mode==='synthetic'){try{scenarioOf(value.scenario);}catch{return;}}
  else if(value.scenario!=='fixture-'+config.preset)return;
  if(value.kind==='mount') {
    app?.unmount();
    const fetcher=config.mode==='synthetic'?syntheticTransport(value.scenario):undefined;
    app=createApp(App,{clientFactory:target=>createClient(target,fetcher),clock:config.mode==='synthetic'?()=>baseTime:()=>Date.now(),persistPreferences:false,initialPage:value.page,initialLanguage:value.language,initialTheme:value.theme});
    view=app.mount('#canvas');
  } else view?.present(value);
}
window.addEventListener('message',receive);
function home(event) {
  if(event.button!==0||event.ctrlKey||event.metaKey||event.altKey||event.shiftKey||!event.target.closest?.('a.brand'))return;
  event.preventDefault();parent.postMessage({schema:demoSchema,kind:'home'},location.origin);
}
document.addEventListener('click',home);
if(import.meta.hot)import.meta.hot.dispose(()=>{configRequest.abort();window.removeEventListener('message',receive);document.removeEventListener('click',home);app?.unmount();});
try {
  config=await loadDemoConfig(configRequest.signal);
  if(!configRequest.signal.aborted)parent.postMessage({schema:demoSchema,kind:'ready'},location.origin);
} catch {
  if(!configRequest.signal.aborted)document.querySelector('#canvas').textContent='DevDemo configuration unavailable. No synthetic fallback was selected.';
}
