<script setup>
import { ref, computed, onMounted, onUnmounted, nextTick } from 'vue';
import { createClient, modulesOf, observed, inventoryRows } from './api.mjs';
import { messages } from './i18n.mjs';
const savedLanguage = (() => { try { return localStorage.getItem('gateway-view-language'); } catch { return null; } })();
const language = ref(['en','ko'].includes(savedLanguage) ? savedLanguage : navigator.language.startsWith('ko') ? 'ko' : 'en');
const t = key => messages[language.value][key] || key;
const pages = ['overview','runtime','configuration','extensions','usage','activity'];
const page = ref('overview'), target = ref(new URLSearchParams(location.search).get('target') || 'gateway');
const credential = ref(''), authenticated = ref(false), busy = ref(false), loginBusy = ref(false), error = ref('');
const capabilities = ref(null), state = ref(null), usage = ref(null), operations = ref([]), viewErrors = ref({});
const selectedOperation = ref(null), days = ref(7), cursor = ref(0), hasMore = ref(false);
let client, generation = 0, timer, detailTrigger;
const viewTimes = ref({});
const allowed = action => capabilities.value?.allowed_operations?.includes(action) === true;
const visiblePages = computed(() => pages.filter(p => p === 'usage' ? allowed('read_usage') : p === 'activity' ? allowed('read_operations') : allowed('read_state')));
const observationTime = computed(() => viewTimes.value[page.value === 'usage' ? 'usage' : page.value === 'activity' ? 'operations' : 'state'] ?? null);
const timezone = Intl.DateTimeFormat().resolvedOptions().timeZone || 'UTC';
const modules = computed(() => modulesOf(state.value));
const runtimeModule = computed(() => modules.value.find(m => m.contract === 'gateway-runtime-status/v1'));
const runtime = computed(() => observed(runtimeModule.value));
const extensionModules = computed(() => modules.value.filter(m => m.contract === 'gateway-extension-status/v1'));
const packageCount = computed(() => extensionModules.value.length && extensionModules.value.every(m => Array.isArray(observed(m)?.store?.inventory?.installed)) ? extensionModules.value.reduce((n,m) => n + inventoryRows(m).length, 0) : null);
const ownership = computed(() => runtime.value?.ownership === 'owned' ? 'running' : runtime.value?.ownership || 'unknown');
const usageGroups = computed(() => Array.isArray(usage.value?.groups) ? usage.value.groups : null);
const format = value => value === null || value === undefined ? '—' : typeof value === 'number' && !Number.isSafeInteger(value) ? t('unknown') : String(value);
const short = value => value ? String(value).slice(0,12) : '—';
const time = value => Number.isSafeInteger(value) && value > 0 ? new Intl.DateTimeFormat(language.value, { dateStyle:'medium', timeStyle:'medium' }).format(value) : t('noObservation');
const pretty = value => value ? JSON.stringify(value,null,2) : t('noObservation');
const statusClass = value => ['succeeded','running','verified'].includes(value) ? 'good' : ['uncertain','pending','unowned'].includes(value) ? 'warn' : ['failed','invalid'].includes(value) ? 'bad' : 'neutral';
function changeLanguage() { document.documentElement.lang=language.value; try { localStorage.setItem('gateway-view-language',language.value); } catch {} }
function clearViews() { state.value=null; capabilities.value=null; usage.value=null; operations.value=[]; selectedOperation.value=null; cursor.value=0; hasMore.value=false; viewErrors.value={}; viewTimes.value={}; }
async function load() {
  if (!authenticated.value || busy.value) return;
  busy.value=true; const current=generation;
  const until=Date.now(), from=until-Number(days.value)*86400000;
  try { const result=await client.capabilities(); if(current!==generation)return;capabilities.value=result.data; }
  catch(failure){if(current===generation){clearViews();busy.value=false;readFailure(failure,'state');}return;}
  if(!visiblePages.value.includes(page.value))page.value=visiblePages.value[0]||'overview';
  if(!allowed('read_usage'))usage.value=null;
  if(!allowed('read_operations')){operations.value=[];selectedOperation.value=null;}
  const calls=[['state','read_state',()=>client.state()],['operations','read_operations',()=>client.operations(0)],['usage','read_usage',()=>client.usage(from,until,timezone)]].filter(([,action])=>allowed(action)).map(([name,,call])=>[name,call]);
  const results=await Promise.allSettled(calls.map(([,call])=>call()));
  if (current!==generation) return;
  const errors={};
  results.forEach((result,index)=>{
    const name=calls[index][0];
    if (result.status==='rejected') {errors[name]=result.reason?.code || 'error'; if(result.reason?.status===401){authenticated.value=false;error.value='unauthorized';} return;}
    viewTimes.value[name]=result.value.observed_at_ms;
    if(name==='capabilities') capabilities.value=result.value.data;
    if(name==='state') state.value=result.value.data;
    if(name==='usage') usage.value=result.value.data;
    if(name==='operations') {operations.value=result.value.data?.items || [];cursor.value=operations.value.at(-1)?.cursor || 0;hasMore.value=operations.value.length===20;}
  });
  viewErrors.value=errors;busy.value=false;
  if(!authenticated.value) clearViews();
}
function readFailure(failure, view){if(failure.status===401){generation++;authenticated.value=false;busy.value=false;clearViews();error.value='unauthorized';}else{viewErrors.value={...viewErrors.value,[view]:failure.code||'error'};}}
async function signIn() {
  error.value='';loginBusy.value=true;generation++;busy.value=false;
  try {client=createClient(target.value);const pending=client.login(credential.value);credential.value='';await pending;generation++;authenticated.value=true;clearViews();await load();}
  catch(failure){error.value=failure.code || 'error';}
  finally {credential.value='';loginBusy.value=false;}
}
async function signOut() {
  generation++;authenticated.value=false;busy.value=false;clearViews();
  try {await client.logout();} catch {} finally {credential.value='';}
}
async function more() {
  if(!hasMore.value||busy.value)return;busy.value=true;const current=generation;
  try {const result=await client.operations(cursor.value);if(current!==generation)return;const rows=result.data?.items||[];operations.value.push(...rows);cursor.value=rows.at(-1)?.cursor||cursor.value;hasMore.value=rows.length===20;}
  catch(failure){if(current===generation)readFailure(failure,'operations');}
  finally{if(current===generation)busy.value=false;}
}
async function inspectOperation(id) {
  const current=generation;detailTrigger=document.activeElement;
  try {const result=await client.operation(id);if(current===generation){selectedOperation.value=result.data;await nextTick();document.querySelector('.drawer button')?.focus();}}
  catch(failure){if(current===generation)readFailure(failure,'operations');}
}
function closeDetails(){selectedOperation.value=null;nextTick(()=>{if(detailTrigger?.isConnected)detailTrigger.focus();});}
function keyboard(event){if(!selectedOperation.value)return;if(event.key==='Escape'){event.preventDefault();closeDetails();}if(event.key==='Tab'){const items=[...document.querySelectorAll('.drawer button,.drawer summary,.drawer a,.drawer input,.drawer select')];const first=items[0],last=items.at(-1);if(event.shiftKey&&document.activeElement===first){event.preventDefault();last?.focus();}else if(!event.shiftKey&&document.activeElement===last){event.preventDefault();first?.focus();}}}
onMounted(async()=>{changeLanguage();window.addEventListener('keydown',keyboard);const current=generation;try{client=createClient(target.value);await client.capabilities();if(current===generation){authenticated.value=true;await load();}}catch(failure){if(current===generation&&failure.status!==401)error.value=failure.code||'error';}timer=setInterval(()=>{if(!document.hidden&&authenticated.value&&operations.value.length<=20)load();},15000);});
onUnmounted(()=>{clearInterval(timer);window.removeEventListener('keydown',keyboard);generation++;});
</script>

<template>
  <div class="app-shell" :class="{ 'session-closed': !authenticated }">
    <aside class="sidebar">
      <a class="brand" href="./" aria-label="Gateway dashboard">
<span class="brand-icon" aria-hidden="true">G<span>↗</span>
</span>
<span>Response Gateway<small>{{ t('subtitle') }}</small>
</span>
</a>
      <div class="instance-label">
<span class="connection-dot" :class="{ online: authenticated }">
</span>
<span>{{ target || 'gateway' }}</span>
<span class="local-label">LOCAL</span>
</div>
      <nav v-if="authenticated" :aria-label="t('title')">
<button v-for="(item,index) in visiblePages" :key="item" :class="{ active: page===item }" @click="page=item">
<span class="nav-symbol" aria-hidden="true">{{ ['◫','◉','≡','◇','▥','◷'][index] }}</span>{{ t(item) }}<span v-if="page===item" class="nav-arrow" aria-hidden="true">›</span>
</button>
</nav>
      <div class="sidebar-bottom">
<span class="read-badge">◉ {{ t('readOnly') }}</span>
<p>{{ authenticated ? t('capabilityNote') : t('loginBoundary') }}</p>
<label class="language">
<span>Language</span>
<select v-model="language" @change="changeLanguage">
<option value="en">English</option>
<option value="ko">한국어</option>
</select>
</label>
</div>
    </aside>
    <main>
      <header class="topbar">
<span>{{ t('title') }} <span class="slash">/</span> <strong>{{ authenticated ? t(page) : t('waiting') }}</strong>
</span>
<div class="top-actions">
<span class="connection" :class="{ online: authenticated }">● {{ authenticated ? t('connected') : t('waiting') }}</span>
<button v-if="authenticated" class="text-button" @click="signOut">{{ t('signOut') }}</button>
</div>
</header>
      <section v-if="!authenticated" class="login-region">
        <div class="login-card">
<div class="eyebrow">LOCAL MANAGEMENT</div>
<h1>{{ t('title') }}</h1>
<p>{{ t('loginNote') }}</p>
<form @submit.prevent="signIn">
<label>{{ t('target') }}<input v-model.trim="target" required maxlength="96" spellcheck="false" autocomplete="off" />
</label>
<label>{{ t('readToken') }}<input v-model="credential" type="password" required autocomplete="off" spellcheck="false" maxlength="4096" />
</label>
<div v-if="error" class="notice bad" role="alert">{{ t(error) }}</div>
<button class="primary" :disabled="loginBusy">{{ loginBusy ? t('refreshing') : t('connect') }} <span aria-hidden="true">→</span>
</button>
</form>
<div class="login-footer">
<span class="read-badge">{{ t('readOnly') }}</span>
<span>{{ t('loginBoundary') }}</span>
</div>
</div>
      </section>
      <div v-else class="content">
<p v-if="!visiblePages.length" class="notice warn">{{ t('forbidden') }}</p>
        <div class="page-heading">
<div>
<div class="eyebrow">{{ target }} <span>·</span> {{ t('readOnly') }}</div>
<h1>{{ t(page) }}</h1>
<p>{{ page==='extensions' ? t('packageNote') : page==='usage' ? t('usageNote') : page==='activity' ? t('operationNote') : t('subtitle') }}</p>
</div>
<button class="secondary" :disabled="busy" @click="load">
<span aria-hidden="true">↻</span> {{ busy ? t('refreshing') : t('refresh') }}</button>
</div>
        <div class="observed-line">{{ t('apiChecked') }} <span>{{ time(observationTime) }}</span>
</div>
        <div v-if="viewErrors.state && ['overview','runtime','configuration','extensions'].includes(page)" class="notice warn" role="status">{{ t(viewErrors.state) }} {{ t('staleView') }}</div>
        <template v-if="page==='overview'">
          <div class="metric-grid">
<article class="metric">
<span>{{ t('runtime') }}</span>
<strong>
<i class="status-dot" :class="statusClass(ownership)">
</i>{{ t(ownership) }}</strong>
<small>{{ runtime?.running?.instance_id ? short(runtime.running.instance_id) : t('noObservation') }}</small>
</article>
<article class="metric">
<span>{{ t('configuration') }}</span>
<strong>{{ runtime ? t(runtime.restart_required ? 'pending' : runtime.running ? 'current' : runtime.selected ? 'selected' : 'noObservation') : '—' }}</strong>
<small>{{ short(runtime?.running?.gateway?.configuration_sha256) }}</small>
</article>
<article class="metric">
<span>{{ t('extensions') }}</span>
<strong>{{ format(packageCount) }}</strong>
<small>{{ t('installed') }}</small>
</article>
</div>
          <div class="overview-grid">
<article class="panel">
<div class="panel-title">
<h2>{{ t('runtime') }}</h2>
<span class="tag" :class="statusClass(ownership)">{{ t(ownership) }}</span>
</div>
<dl>
<div>
<dt>{{ t('instance') }}</dt>
<dd>{{ runtime?.running?.instance_id || '—' }}</dd>
</div>
<div>
<dt>{{ t('configurationDigest') }}</dt>
<dd class="mono">{{ runtime?.running?.gateway?.configuration_sha256 || '—' }}</dd>
</div>
<div>
<dt>{{ t('executionDigest') }}</dt>
<dd class="mono">{{ runtime?.running?.gateway?.execution_sha256 || '—' }}</dd>
</div>
</dl>
<p v-if="!runtime" class="empty">{{ t('noRuntime') }}</p>
<p v-if="runtime?.restart_required" class="notice warn">{{ t('needsRestart') }}</p>
</article>
<article class="panel">
<div class="panel-title">
<h2>{{ t('metadata') }}</h2>
<span class="small-muted">{{ format(modules.length) }}</span>
</div>
<div v-for="module in modules" :key="module.id" class="module-row">
<span class="module-icon">◇</span>
<div>
<strong>{{ module.id }}</strong>
<small>{{ module.contract }}</small>
</div>
<small>{{ time(module.observation.observed_at_ms) }}</small>
<span class="tag neutral">{{ module.observation.state==='observed' ? t('observed') : t(module.observation.state) }}</span>
</div>
<p v-if="!modules.length" class="empty">{{ t('noData') }}</p>
</article>
</div>
          <article class="panel">
<h2>{{ t('features') }}</h2>
<div v-for="feature in capabilities?.features || []" :key="feature.id" class="module-row">
<div>
<strong>{{ feature.id }}</strong>
<small>{{ feature.version }}</small>
</div>
<span class="tag neutral">{{ t('installed') }}: {{ t(feature.installed ? 'yes' : 'no') }}</span>
<span class="tag neutral">{{ t('enabled') }}: {{ t(feature.enabled ? 'yes' : 'no') }}</span>
</div>
<p class="small-muted">{{ t('allowedReads') }}: {{ capabilities?.allowed_operations?.join(', ') || '—' }}</p>
</article>
        </template>
        <template v-if="page==='runtime'">
<article class="panel">
<div class="panel-title">
<h2>{{ t('runtime') }}</h2>
<span class="tag" :class="statusClass(ownership)">{{ t(ownership) }}</span>
</div>
<p v-if="runtime?.external_change" class="notice warn">{{ t('externalChange') }}</p>
<p class="small-muted">{{ t('lastObserved') }}: {{ time(runtimeModule?.observation?.observed_at_ms) }}</p>
<pre class="json-view">{{ pretty(runtime) }}</pre>
</article>
</template>
        <template v-if="page==='configuration'">
<p class="small-muted">{{ t('lastObserved') }}: {{ time(runtimeModule?.observation?.observed_at_ms) }}</p>
<p v-if="runtime?.restart_required" class="notice warn">{{ t('needsRestart') }}</p>
<div class="two-columns">
<article class="panel">
<div class="panel-title">
<h2>{{ t('current') }}</h2>
<span class="tag neutral">{{ runtime?.running ? t('effective') : t('noObservation') }}</span>
</div>
<pre class="json-view">{{ pretty(runtime?.running_manifest) }}</pre>
</article>
<article class="panel">
<div class="panel-title">
<h2>{{ t('desired') }}</h2>
<span class="tag" :class="runtime?.restart_required ? 'warn' : 'neutral'">{{ runtime?.selected || '—' }}</span>
</div>
<pre class="json-view">{{ pretty(runtime?.desired) }}</pre>
</article>
</div>
<article class="panel">
<h2>{{ t('candidates') }}</h2>
<div class="chips">
<span v-for="candidate in runtime?.candidates || []" :key="candidate" class="tag neutral">{{ candidate }}</span>
<span v-if="!runtime?.candidates?.length">—</span>
</div>
</article>
</template>
        <template v-if="page==='extensions'">
<p class="notice neutral">{{ t('readOnlyPackages') }}</p>
<article v-for="module in extensionModules" :key="module.id" class="panel">
<div class="panel-title">
<h2>{{ module.id }}</h2>
<span class="small-muted">{{ time(module.observation.observed_at_ms) }}</span>
</div>
<div class="table-wrap">
<table>
<thead>
<tr>
<th>{{ t('package') }}</th>
<th>{{ t('version') }}</th>
<th>{{ t('state') }}</th>
<th>{{ t('selected') }}</th>
<th>{{ t('effective') }}</th>
<th>{{ t('requestedPermissions') }}</th>
<th>{{ t('permissions') }}</th>
</tr>
</thead>
<tbody>
<tr v-for="item in inventoryRows(module)" :key="item.id+item.package_sha256">
<td>
<strong>{{ item.id }}</strong>
<small class="mono">{{ short(item.package_sha256) }}</small>
</td>
<td class="mono">{{ item.version }}</td>
<td>
<span class="tag" :class="item.verified ? 'good' : 'bad'">{{ t(item.verified ? 'verified' : 'invalid') }}</span>
</td>
<td>{{ t(item.selected ? 'yes' : 'no') }}</td>
<td>{{ t(item.effective===null ? 'unknown' : item.effective ? 'yes' : 'no') }}</td>
<td class="permission-list">{{ item.package?.permissions?.join(', ') || '—' }}</td>
<td class="permission-list">{{ item.grants?.join(', ') || '—' }}</td>
</tr>
</tbody>
</table>
</div>
<p v-if="!inventoryRows(module).length" class="empty">{{ module.observation.state==='observed' ? t('noPackages') : t(module.observation.state) }}</p>
</article>
<div v-if="!extensionModules.length" class="empty panel">{{ t('noData') }}</div>
</template>
        <template v-if="page==='usage'">
<div class="filter-row">
<select v-model="days" @change="load">
<option :value="1">1 {{ t('days') }}</option>
<option :value="7">7 {{ t('days') }}</option>
<option :value="30">30 {{ t('days') }}</option>
</select>
<span>{{ timezone }}</span>
</div>
<p v-if="viewErrors.usage" class="notice warn">{{ t(viewErrors.usage) }} {{ t('staleView') }}</p>
<article class="panel">
<div class="table-wrap">
<table>
<thead>
<tr>
<th>{{ t('date') }}</th>
<th>{{ t('model') }}</th>
<th>{{ t('calls') }}</th>
<th>{{ t('input') }}</th>
<th>{{ t('output') }}</th>
<th>{{ t('partial') }}</th>
<th>{{ t('unobserved') }}</th>
<th>{{ t('unfinished') }}</th>
</tr>
</thead>
<tbody>
<tr v-for="(group,index) in usageGroups || []" :key="index">
<td>{{ group.date }}</td>
<td>
<strong>{{ group.model_alias }}</strong>
<small>{{ group.provider }}</small>
</td>
<td>{{ format(group.calls) }}</td>
<td>{{ format(group.token_sums?.input_tokens) }}</td>
<td>{{ format(group.token_sums?.output_tokens) }}</td>
<td>{{ format(group.partial) }}</td>
<td>{{ format(group.unobserved) }}</td>
<td>{{ format(group.unfinished) }}</td>
</tr>
</tbody>
</table>
</div>
<p v-if="!usageGroups?.length" class="empty">{{ usageGroups ? t('noUsage') : t('noData') }}</p>
</article>
</template>
        <template v-if="page==='activity'">
<p v-if="viewErrors.operations" class="notice warn">{{ t(viewErrors.operations) }} {{ t('staleView') }}</p>
<article class="panel">
<div class="table-wrap">
<table>
<thead>
<tr>
<th>{{ t('operation') }}</th>
<th>{{ t('action') }}</th>
<th>{{ t('subject') }}</th>
<th>{{ t('observedState') }}</th>
<th>{{ t('lastObserved') }}</th>
<th>
</th>
</tr>
</thead>
<tbody>
<tr v-for="row in operations" :key="row.operation.id">
<td class="mono">{{ short(row.operation.id) }}</td>
<td>{{ row.operation.request.action }}</td>
<td>{{ row.operation.actor.subject }}</td>
<td>
<span class="tag" :class="statusClass(row.observed_state)">{{ t(row.observed_state) }}</span>
</td>
<td>{{ time(row.operation.events.at(-1)?.at_ms) }}</td>
<td>
<button class="text-button" @click="inspectOperation(row.operation.id)">{{ t('details') }} ↗</button>
</td>
</tr>
</tbody>
</table>
</div>
<p v-if="!operations.length" class="empty">{{ t('noOperations') }}</p>
<button v-if="hasMore" class="secondary load-more" :disabled="busy" @click="more">{{ t('more') }}</button>
</article>
</template>
      </div>
    </main>
    <div v-if="selectedOperation" class="drawer-backdrop" @click.self="closeDetails">
<section class="drawer" role="dialog" aria-modal="true" :aria-label="t('operation')">
<div class="panel-title">
<h2>{{ t('operation') }}</h2>
<button class="secondary" @click="closeDetails">{{ t('close') }} ×</button>
</div>
<p class="mono wrap">{{ selectedOperation.operation.id }}</p>
<div class="chips">
<span class="tag" :class="statusClass(selectedOperation.observed_state)">{{ t(selectedOperation.observed_state) }}</span>
<span>{{ t('recordedState') }}: {{ t(selectedOperation.operation.state) }}</span>
</div>
<p v-if="selectedOperation.uncertainty" class="notice warn">{{ t(selectedOperation.uncertainty) }}</p>
<ol class="timeline">
<li v-for="event in selectedOperation.operation.events" :key="event.sequence">
<span class="timeline-dot">
</span>
<div>
<strong>{{ t(event.phase) }}</strong>
<small>{{ time(event.at_ms) }} · {{ event.actor?.subject || t('system') }}</small>
<span class="tag" :class="statusClass(event.state)">{{ t(event.state) }}</span>
</div>
</li>
</ol>
<details>
<summary>{{ t('metadata') }}</summary>
<pre class="json-view">{{ pretty(selectedOperation.operation) }}</pre>
</details>
</section>
</div>
  </div>
</template>
