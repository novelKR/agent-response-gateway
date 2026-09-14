export const demoSchema = 'gateway-devdemo/v1';
export const baseTime = Date.UTC(2026, 0, 2, 12);
export const scenarios = [
  ['standalone','Standalone ready','Standalone 정상','overview'],
  ['stopped','Stopped runtime','실행 중지','overview'],
  ['pending','Pending configuration','구성 적용 대기','configuration'],
  ['external','External change','외부 변경 감지','configuration'],
  ['versions','Different package versions','확장 버전 차이','extensions'],
  ['unobserved','Unobserved modules','모듈 미관측','overview'],
  ['unsupported','Unsupported modules','모듈 미지원','extensions'],
  ['team','Own Team usage','팀 본인 사용량','usage'],
  ['embedded','Restricted Embedded host','제한된 Embedded 호스트','overview'],
  ['empty','Empty lists','빈 목록','usage'],
  ['long','Long paginated content','긴 내용과 페이지 나눔','activity'],
  ['loading','Loading reads','조회 중','overview'],
  ['forbidden','Read access refused','조회 권한 거절','overview'],
  ['expired','Expired session','세션 만료','overview'],
  ['offline','Disconnected API','API 연결 끊김','overview'],
  ['usage','Unknown, zero and large counters','미관측·0·큰 카운터','usage'],
  ['audit','Succeeded, failed and uncertain','성공·실패·미확정','activity'],
].map(([id,en,ko,page])=>Object.freeze({id,en,ko,page}));
export const pages=['overview','runtime','configuration','extensions','usage','activity'];
export function scenarioOf(id) {
  const scenario=scenarios.find(value=>value.id===id);
  if(!scenario)throw new TypeError('Unknown development scenario');
  return scenario;
}
const digest = c => c.repeat(64);
function operation(index) {
  const state=['succeeded','failed','uncertain'][index%3], subject='synthetic-operator';
  const events=['accepted','started','finished'].map((phase,i)=>({sequence:i+1,phase,at_ms:baseTime-3000+i*1000,actor:{subject},state:i===0?'queued':i===1?'running':state}));
  return {cursor:index+1,observed_state:state,uncertainty:state==='uncertain'?'result_record_missing':null,operation:{id:'synthetic-operation-'+String(index+1).padStart(3,'0'),actor:{subject,credential:'synthetic-credential-id'},request:{action:'configuration_select',target:'gateway'},state,events}};
}
export function scenarioData(id) {
  scenarioOf(id);
  const module=(name,contract,data)=>({id:name,contract,observation:{state:'observed',observed_at_ms:baseTime-60000,data:{schema:contract,...data}}});
  const current={configuration_sha256:digest('a'),execution_sha256:digest('b')};
  const runtime={target:'gateway',revision:3,ownership:id==='embedded'?'unowned':id==='stopped'?'stopped':'owned',selected:'candidate-current',candidates:['candidate-current','candidate-next'],external_change:id==='external',running:id==='stopped'?null:{instance_id:'synthetic-instance',gateway:current},running_manifest:{models:['synthetic-model'],readiness_version:7},desired:{models:['synthetic-model']},desired_valid:true,restart_required:['pending','external','versions'].includes(id)};
  if(runtime.restart_required)runtime.selected='candidate-next';
  const packages=['1.0.0','2.0.0'].map((version,i)=>({id:'synthetic-codec',version,package_sha256:digest(i?'d':'c'),verified:true,package:{permissions:['network']}}));
  const extensions={target:'gateway',store:{inventory:{installed:packages,activation:{extensions:[{...packages[id==='versions'?1:0],grants:['network']}]}}},effective:{packages:[packages[0]]},removal_supported:false};
  const modules=[module('runtime','gateway-runtime-status/v1',runtime),module('native','gateway-extension-status/v1',extensions),{id:'profiles',contract:'gateway-extension-status/v1',observation:{state:'unsupported',reason:'host_observation_unavailable'}}];
  if(['unobserved','unsupported'].includes(id))for(const item of modules)item.observation={state:id,reason:'host_observation_unavailable'};
  if(id==='empty'){extensions.store.inventory.installed=[];extensions.store.inventory.activation.extensions=[];extensions.effective.packages=[];}
  if(id==='long') {
    runtime.running.instance_id='synthetic-instance-'+('long-identifier-'.repeat(20));
    extensions.store.inventory.installed=Array.from({length:30},(_,i)=>({...packages[0],id:'synthetic-package-'+i+'-'+('long-'.repeat(10))}));
  }
  const grants=id==='team'?['read_usage']:id==='embedded'?['read_state']:['read_state','read_usage','read_operations'];
  const groups=id==='empty'?[]:[
    {date:'2026-01-02',model_alias:'synthetic-model',provider:'synthetic',interpretation:{kind:'builtin_parser',profile:'responses/v1'},calls:4,final:2,partial:1,unobserved:1,unfinished:1,token_sums:{input_tokens:1250,output_tokens:null}},
    {date:'2026-01-01',model_alias:'synthetic-large',provider:'synthetic',interpretation:{kind:'trusted_provider_plugin',protocol:'gateway-provider/v1',provider_protocol:'synthetic-provider/v1',package_id:'synthetic-provider',package_version:'1.0.0',package_sha256:'a'.repeat(64),executable_sha256:'b'.repeat(64)},calls:1,final:1,partial:0,unobserved:0,unfinished:0,token_sums:{input_tokens:'__devdemo_large_integer__',output_tokens:0}},
  ];
  const own={schema:'gateway-team-http/v1',scope:'own_subject',from_ms:baseTime-86400000,to_ms:baseTime,next_after:null,requests:[{record:{admission:{id:'synthetic-team-request',subject:'synthetic-alice',route:'synthetic-model',at_ms:baseTime-3000},headers:{gateway_request:'synthetic-request-id',status:200},finished:{transport:'eof',at_ms:baseTime-2000}},usage:{state:'unobserved',attempts:[]},transport_observation:'recorded'}]};
  return {capabilities:{target:'gateway',features:[{id:'synthetic-view-host',version:'fixture/v1',installed:true,enabled:true,operations:grants}],supported_operations:grants,allowed_operations:grants,read_sessions:true,unsupported_operations:['package_remove']},state:{schema:'gateway-management-state/v1',modules},usage:id==='team'?own:{schema:'gateway-management-usage/v2',groups},operations:id==='empty'?[]:Array.from({length:id==='long'?55:3},(_,i)=>operation(i))};
}

// An in-memory HTTP-shaped transport: the shared production parser still consumes it.
export function syntheticTransport(id) {
  const data=scenarioData(id);let authenticated=true,capabilities=0;
  const reply=(value,status=200)=>new Response(JSON.stringify({schema:'gateway-management-http/v1',observed_at_ms:baseTime,...value}).replaceAll('"__devdemo_large_integer__"','9007199254740993123'),{status,headers:{'Content-Type':'application/json'}});
  const failure=(code,status)=>reply({error:{code}},status);
  return async (path,options) => {
    if(options.signal?.aborted)throw new DOMException('Aborted','AbortError');
    const url=new URL(path,'http://synthetic.invalid');
    if(!url.pathname.startsWith('/management/v1/'))return failure('not_found',404);
    const route=url.pathname.slice('/management/v1/'.length);
    if(route==='session'&&options.method==='DELETE'){authenticated=false;return reply({data:{}});}
    if(options.method!=='GET')return failure('unauthorized',401);
    if(!authenticated)return failure('unauthorized',401);
    if(route==='capabilities') {
      capabilities++;
      if(id==='offline'&&capabilities>1)throw new TypeError('Synthetic connection failure');
      if(id==='expired'&&capabilities>1)return failure('unauthorized',401);
      return reply({data:data.capabilities});
    }
    if(id==='loading')return new Promise((_resolve,reject)=>options.signal.addEventListener('abort',()=>reject(new DOMException('Aborted','AbortError')),{once:true}));
    if(id==='forbidden'&&route==='state')return failure('forbidden',403);
    const permission=route==='state'?'read_state':route==='usage'?'read_usage':route.startsWith('operations')?'read_operations':null;
    if(!data.capabilities.allowed_operations.includes(permission))return failure('forbidden',403);
    if(route==='state'||route==='usage')return reply({data:data[route]});
    if(route==='operations') {
      const after=Number(url.searchParams.get('after')||0),limit=Number(url.searchParams.get('limit')||20);
      return reply({data:{items:data.operations.filter(row=>row.cursor>after).slice(0,limit)}});
    }
    if(route.startsWith('operations/')) {
      const row=data.operations.find(row=>row.operation.id===route.slice(11));
      return row?reply({data:row}):failure('not_found',404);
    }
    return failure('not_found',404);
  };
}
