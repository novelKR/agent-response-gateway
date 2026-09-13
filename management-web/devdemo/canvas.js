import {createApp} from 'vue';
import App from '../src/App.vue';
import {createClient} from '../src/api.mjs';
import '../src/style.css';
import {baseTime,demoSchema,scenarioOf,syntheticTransport,pages} from './scenarios.mjs';
let app,view;
function receive(event) {
  if(event.origin!==location.origin||event.source!==parent||parent===window)return;
  const value=event.data;
  if(value?.schema!==demoSchema||!['mount','present'].includes(value.kind)||!['en','ko'].includes(value.language)||!['light','dark','system'].includes(value.theme)||!pages.includes(value.page))return;
  try{scenarioOf(value.scenario);}catch{return;}
  if(value.kind==='mount') {
    app?.unmount();
    const fetcher=syntheticTransport(value.scenario);
    app=createApp(App,{clientFactory:target=>createClient(target,fetcher),clock:()=>baseTime,persistPreferences:false,initialPage:value.page,initialLanguage:value.language,initialTheme:value.theme});
    view=app.mount('#canvas');
  } else view?.present(value);
}
window.addEventListener('message',receive);
function home(event) {
  if(event.button!==0||event.ctrlKey||event.metaKey||event.altKey||event.shiftKey||!event.target.closest?.('a.brand'))return;
  event.preventDefault();parent.postMessage({schema:demoSchema,kind:'home'},location.origin);
}
document.addEventListener('click',home);
parent.postMessage({schema:demoSchema,kind:'ready'},location.origin);
if(import.meta.hot)import.meta.hot.dispose(()=>{window.removeEventListener('message',receive);document.removeEventListener('click',home);app?.unmount();});
