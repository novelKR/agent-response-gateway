<script setup>
import {ref,computed,watch,onMounted,onUnmounted} from 'vue';
import {scenarios,scenarioOf,pages,demoSchema} from './scenarios.mjs';
import {messages} from '../src/i18n.mjs';
import {createTheme} from '../src/theme.mjs';
import '../src/tokens.css';
import {loadDemoConfig,fixtureReadCredential} from './config.mjs';
const scenario=ref('standalone'),page=ref('overview'),language=ref('ko'),theme=ref('light'),width=ref('1280'),frame=ref(null),ready=ref(false);
const config=ref(null),configurationFailed=ref(false);
const configRequest=new AbortController();
const synthetic=computed(()=>config.value?.mode==='synthetic');
const availablePages=computed(()=>scenario.value==='team'||config.value?.preset==='usage'?['usage']:scenario.value==='embedded'?pages.filter(p=>!['usage','activity'].includes(p)):pages);
const themeController=createTheme({root:document.documentElement,media:matchMedia('(prefers-color-scheme: dark)')});
watch(theme,value=>themeController.set(value));
watch(language,value=>{document.documentElement.lang=value;},{immediate:true});
const t=key=>messages[language.value][key]||key;
function send(kind='present'){if(ready.value)frame.value.contentWindow.postMessage({schema:demoSchema,kind,scenario:scenario.value,page:page.value,language:language.value,theme:theme.value},location.origin);}
function selectScenario(){if(synthetic.value){page.value=scenarioOf(scenario.value).page;send('mount');}}
function receive(event){
  if(event.origin!==location.origin||event.source!==frame.value?.contentWindow||event.data?.schema!==demoSchema)return;
  if(event.data.kind==='ready'){ready.value=true;send('mount');}
  else if(event.data.kind==='home'){page.value=availablePages.value[0];send();}
}
onMounted(async()=>{
  window.addEventListener('message',receive);
  try {
    const value=await loadDemoConfig(configRequest.signal);
    if(configRequest.signal.aborted)return;
    if(value.mode==='fixture'){scenario.value='fixture-'+value.preset;page.value=value.preset==='usage'?'usage':'overview';}
    config.value=value;
  } catch {if(!configRequest.signal.aborted)configurationFailed.value=true;}
});
onUnmounted(()=>{configRequest.abort();window.removeEventListener('message',receive);themeController.dispose();});
</script>
<template>
<div class="demo-shell">
<header class="demo-tools">
  <div><strong>Gateway DevDemo</strong> <span class="demo-badge">{{ !config ? (language==='ko'?'개발 구성 확인 중':'Loading development configuration') : synthetic ? (language==='ko'?'개발 전용 · 합성 데이터':'Development only · Synthetic data') : (language==='ko'?'실제 관리 API · 합성 fixture':'Actual management API · Synthetic fixture') }}</span></div>
  <p>{{ language==='ko'?'동일한 운영 UI를 재현하는 개발 화면입니다. 실제 Gateway·provider를 실행하지 않습니다.':'Shared production views with synthetic responses. No Gateway or provider is running.' }}</p>
  <p v-if="configurationFailed" role="alert">{{ language==='ko'?'개발 구성을 확인할 수 없습니다. 합성 모드로 전환하지 않습니다.':'Development configuration is unavailable. No synthetic fallback was selected.' }}</p>
  <div v-if="config" class="demo-controls">
    <label>{{ language==='ko'?'시나리오':'Scenario' }}<select v-model="scenario" :disabled="!synthetic" @change="selectScenario"><template v-if="synthetic"><option v-for="item in scenarios" :key="item.id" :value="item.id">{{ item[language] }}</option></template><option v-else :value="scenario">{{ config.preset==='usage'?(language==='ko'?'실제 API · 사용량 전용':'Actual API · Usage only'):(language==='ko'?'실제 API · 전체 조회':'Actual API · Full read') }}</option></select></label>
    <label>{{ language==='ko'?'화면':'Page' }}<select v-model="page" @change="send()"><option v-for="item in pages" :key="item" :value="item" :disabled="!availablePages.includes(item)">{{ t(item) }}</option></select></label>
    <label>{{ t('theme') }}<select v-model="theme" @change="send()"><option v-for="value in ['light','dark','system']" :key="value" :value="value">{{ t('theme_'+value) }}</option></select></label>
    <label>Language<select v-model="language" @change="send()"><option value="ko">한국어</option><option value="en">English</option></select></label>
    <label>{{ language==='ko'?'화면 폭':'Viewport' }}<select v-model="width"><option v-for="value in ['360','768','1280']" :key="value" :value="value">{{ value }}px</option><option value="available">{{ language==='ko'?'가용 폭':'Available width' }}</option></select></label>
    <button @click="send('mount')">{{ language==='ko'?'초기화':'Reset' }}</button>
  </div>
  <p class="demo-note">{{ language==='ko'?'권한이 없는 화면은 canvas에서 제공되지 않습니다. 표시 설정과 시나리오는 저장하지 않습니다.':'Unavailable views remain permission-limited in the canvas. Scenarios and display settings are not persisted.' }}</p>
  <p v-if="config && !synthetic" class="demo-note">{{ language==='ko'?'실제 조회 세션으로 로그인합니다. 아래 합성 테스트 key만 사용하세요. 오류 주입은 꺼져 있으며 preset은 실행 명령에서 선택합니다.':'Sign in through a real read session using only this synthetic test key. Fault injection is disabled; choose the preset in the launch command.' }} <code>{{ fixtureReadCredential }}</code></p>
</header>
<div v-if="config" class="demo-stage"><iframe ref="frame" src="/canvas.html" title="Shared dashboard canvas" :style="{width:width==='available'?'100%':width+'px'}"></iframe></div>
</div>
</template>
<style>
*{box-sizing:border-box}body{margin:0;background:var(--canvas);color:var(--text);font:14px var(--font-family)}
.demo-tools{padding:20px 24px;background:var(--surface);border-bottom:1px solid var(--line)}
.demo-tools p{font-size:12px;line-height:1.6;margin:10px 0}.demo-badge{display:inline-block;background:var(--warning-soft);color:var(--warning);padding:4px 8px;border-radius:4px;font-size:12px}
.demo-controls{display:flex;flex-wrap:wrap;align-items:end;gap:12px}.demo-controls label{display:grid;gap:5px;font-size:12px}
.demo-controls select,.demo-controls button{font:inherit;padding:8px;border:1px solid var(--line);border-radius:5px;background:var(--surface);color:var(--text)}.demo-controls button{cursor:pointer}
.demo-controls :focus-visible{outline:3px solid var(--focus);outline-offset:2px}.demo-note{color:var(--muted)}.demo-note code{display:block;overflow-wrap:anywhere;margin-top:8px}
.demo-stage{padding:20px;overflow:auto}.demo-stage iframe{display:block;border:1px solid var(--line);box-sizing:content-box;height:900px;background:var(--surface);margin:0 auto}
@media(max-width:700px){.demo-tools{padding:16px}.demo-stage{padding:12px}.demo-controls select{max-width:280px}}
</style>
