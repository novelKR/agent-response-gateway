<script setup>
import {ref,computed,onMounted,onUnmounted} from 'vue';
import {scenarios,scenarioOf,pages,demoSchema} from './scenarios.mjs';
import {messages} from '../src/i18n.mjs';
const scenario=ref('standalone'),page=ref('overview'),language=ref('ko'),theme=ref('light'),width=ref('1280'),frame=ref(null),ready=ref(false);
const availablePages=computed(()=>scenario.value==='team'?['usage']:scenario.value==='embedded'?pages.filter(p=>!['usage','activity'].includes(p)):pages);
const t=key=>messages[language.value][key]||key;
function send(kind='present'){if(ready.value)frame.value.contentWindow.postMessage({schema:demoSchema,kind,scenario:scenario.value,page:page.value,language:language.value,theme:theme.value},location.origin);}
function selectScenario(){page.value=scenarioOf(scenario.value).page;send('mount');}
function receive(event){
  if(event.origin!==location.origin||event.source!==frame.value?.contentWindow||event.data?.schema!==demoSchema)return;
  if(event.data.kind==='ready'){ready.value=true;send('mount');}
  else if(event.data.kind==='home'){page.value=availablePages.value[0];send();}
}
onMounted(()=>window.addEventListener('message',receive));
onUnmounted(()=>window.removeEventListener('message',receive));
</script>
<template>
<div class="demo-shell">
<header class="demo-tools">
  <div><strong>Gateway DevDemo</strong> <span class="demo-badge">{{ language==='ko'?'개발 전용 · 합성 데이터':'Development only · Synthetic data' }}</span></div>
  <p>{{ language==='ko'?'동일한 운영 UI를 재현하는 개발 화면입니다. 실제 Gateway·provider를 실행하지 않습니다.':'Shared production views with synthetic responses. No Gateway or provider is running.' }}</p>
  <div class="demo-controls">
    <label>{{ language==='ko'?'시나리오':'Scenario' }}<select v-model="scenario" @change="selectScenario"><option v-for="item in scenarios" :key="item.id" :value="item.id">{{ item[language] }}</option></select></label>
    <label>{{ language==='ko'?'화면':'Page' }}<select v-model="page" @change="send()"><option v-for="item in pages" :key="item" :value="item" :disabled="!availablePages.includes(item)">{{ t(item) }}</option></select></label>
    <label>{{ t('theme') }}<select v-model="theme" @change="send()"><option v-for="value in ['light','dark','system']" :key="value" :value="value">{{ t('theme_'+value) }}</option></select></label>
    <label>Language<select v-model="language" @change="send()"><option value="ko">한국어</option><option value="en">English</option></select></label>
    <label>{{ language==='ko'?'화면 폭':'Viewport' }}<select v-model="width"><option v-for="value in ['360','768','1280']" :key="value" :value="value">{{ value }}px</option><option value="available">{{ language==='ko'?'가용 폭':'Available width' }}</option></select></label>
    <button @click="send('mount')">{{ language==='ko'?'초기화':'Reset' }}</button>
  </div>
  <p class="demo-note">{{ language==='ko'?'권한이 없는 화면은 canvas에서 제공되지 않습니다. 표시 설정과 시나리오는 저장하지 않습니다.':'Unavailable views remain permission-limited in the canvas. Scenarios and display settings are not persisted.' }}</p>
</header>
<div class="demo-stage"><iframe ref="frame" src="/canvas.html" title="Shared dashboard canvas" :style="{width:width==='available'?'100%':width+'px'}"></iframe></div>
</div>
</template>
<style>
*{box-sizing:border-box}body{margin:0;background:#e7ecf1;color:#243345;font:14px system-ui,sans-serif}
.demo-tools{padding:20px 24px;background:#fff;border-bottom:1px solid #b8c5d1}
.demo-tools p{font-size:12px;line-height:1.6;margin:10px 0}.demo-badge{display:inline-block;background:#fff0c6;color:#674608;padding:4px 8px;border-radius:4px;font-size:12px}
.demo-controls{display:flex;flex-wrap:wrap;align-items:end;gap:12px}.demo-controls label{display:grid;gap:5px;font-size:12px}
.demo-controls select,.demo-controls button{font:inherit;padding:8px;border:1px solid #8293a6;border-radius:5px;background:#fff;color:#243345}.demo-controls button{cursor:pointer}
.demo-controls :focus-visible{outline:3px solid #28726c;outline-offset:2px}.demo-note{color:#536477}
.demo-stage{padding:20px;overflow:auto}.demo-stage iframe{display:block;border:1px solid #8a9aaa;box-sizing:content-box;height:900px;background:#fff;margin:0 auto}
@media(max-width:700px){.demo-tools{padding:16px}.demo-stage{padding:12px}.demo-controls select{max-width:280px}}
</style>
