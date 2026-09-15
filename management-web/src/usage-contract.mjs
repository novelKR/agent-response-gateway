// Read-side shape/number checks. Host normalization and provenance remain host duties.
const schemas = ['gateway-usage-event/v1','gateway-usage-event/v2'];
const fields = ['input_tokens','output_tokens','total_tokens','input_regular_tokens','cache_read_input_tokens','cache_write_input_tokens','reasoning_output_tokens'];
const common = ['schema','producer_id','request_id','attempt_id','event_id','revision','kind','started_at_ms','observed_at_ms','provider','model_alias','upstream_model','reported_model','provider_request_id','provider_response_id','configuration_sha256','upstream','gateway','finality','observation_incomplete','usage'];
const outcomes = ['unknown','in_progress','completed','incomplete','failed','cancelled','transport_lost','conversion_failed','observation_incomplete'];
const object = v => v !== null && typeof v === 'object' && !Array.isArray(v);
const exact = (v, keys) => object(v) && Object.keys(v).length === keys.length && keys.every(k => Object.hasOwn(v,k));
const label = v => typeof v === 'string' && /^[A-Za-z0-9_/.:\-]{1,200}$/.test(v) && !v.endsWith('\n');
const hex = v => typeof v === 'string' && /^[a-f0-9]{64}$/.test(v) && v.length===64;
const require = value => { if(!value) { const error=new TypeError('unsupported_usage_contract');error.code='unsupported_contract';throw error; } };
const preservedIntegers = new WeakMap();
export function rememberNumber(holder, key, source) {
  if(!preservedIntegers.has(holder))preservedIntegers.set(holder,new Map());
  preservedIntegers.get(holder).set(key,source);
}
function integer(value, holder, key) {
  // The API parser preserves unsafe JSON integers as exact decimal display strings.
  if(typeof value==='number' && Number.isSafeInteger(value) && value>=0) {
    const source=preservedIntegers.get(holder)?.get(key);
    return source===undefined || /^(0|[1-9][0-9]*)$/.test(source) ? BigInt(value) : null;
  }
  if(typeof value==='string' && preservedIntegers.get(holder)?.get(key)===value && /^(0|[1-9][0-9]{0,19})$/.test(value) && !value.endsWith('\n')) {
    const number=BigInt(value);
    if(number>BigInt(Number.MAX_SAFE_INTEGER) && number<=18446744073709551615n)return number;
  }
  return null;
}
function counter(value) {
  require(exact(value,['source','value']));
  if(['reported','derived'].includes(value.source))require(integer(value.value,value,'value')!==null);
  else require(['not_reported','not_applicable','invalid'].includes(value.source) && value.value===null);
}
function interpretation(value) {
  require(exact(value,['kind','protocol','provider_protocol','package_id','package_version','package_sha256','executable_sha256']));
  require(value.kind==='trusted_provider_plugin' && value.protocol==='gateway-provider/v1');
  require(typeof value.provider_protocol==='string' && /^[a-z][a-z0-9._-]{0,63}\/v[1-9][0-9]{0,5}$/.test(value.provider_protocol) && !value.provider_protocol.endsWith('\n'));
  require(typeof value.package_id==='string' && /^[a-z][a-z0-9-]{0,63}$/.test(value.package_id) && !value.package_id.endsWith('\n'));
  require(typeof value.package_version==='string' && /^(0|[1-9][0-9]{0,5})\.(0|[1-9][0-9]{0,5})\.(0|[1-9][0-9]{0,5})$/.test(value.package_version) && !value.package_version.endsWith('\n'));
  require(hex(value.package_sha256) && hex(value.executable_sha256));
}
function event(value) {
  require(object(value) && schemas.includes(value.schema));
  const v2=value.schema===schemas[1];
  require(exact(value,[...common,v2?'interpretation':'profile']));
  for(const key of ['producer_id','request_id','attempt_id','event_id','provider','model_alias','upstream_model'])require(label(value[key]));
  for(const key of ['reported_model','provider_request_id','provider_response_id'])require(value[key]===null || label(value[key]));
  for(const key of ['revision','started_at_ms','observed_at_ms'])require(integer(value[key],value,key)!==null);
  require(integer(value.observed_at_ms,value,'observed_at_ms')>=integer(value.started_at_ms,value,'started_at_ms'));
  require(['attempt_started','usage_updated','attempt_finished'].includes(value.kind) && (value.kind==='attempt_started')===(integer(value.revision,value,'revision')===0n));
  require(outcomes.includes(value.upstream) && outcomes.includes(value.gateway) && ['unobserved','partial','final'].includes(value.finality));
  require(typeof value.observation_incomplete==='boolean' && hex(value.configuration_sha256));
  if(v2)interpretation(value.interpretation);
  else require(['responses_v1','chat_v1','deep_seek_v1','messages_v1','gemini_interactions_v1'].includes(value.profile));
  const usage=value.usage;
  require(exact(usage,['counters','reported','cache_write_details','violations']) && exact(usage.counters,fields));
  fields.forEach(key=>counter(usage.counters[key]));
  require(object(usage.reported) && Object.keys(usage.reported).length<=16);
  Object.values(usage.reported).forEach(counter);
  require(Array.isArray(usage.cache_write_details) && usage.cache_write_details.length<=16);
  usage.cache_write_details.forEach(detail=>{require(exact(detail,['ttl_seconds','input_tokens']) && integer(detail.ttl_seconds,detail,'ttl_seconds')!==null);counter(detail.input_tokens);});
  require(Array.isArray(usage.violations) && usage.violations.length<=32 && usage.violations.every(v=>['invalid_counter','input_partition','total_mismatch','subset_exceeds_total','ttl_mismatch','counter_decreased'].includes(v)));
  if(v2) {
    require(Object.keys(usage.reported).length===0 && usage.cache_write_details.length===0);
    const number = key => integer(usage.counters[key].value,usage.counters[key],'value');
    const input=number('input_tokens'),output=number('output_tokens'),total=number('total_tokens');
    if(input!==null && output!==null && total!==null)require(input+output===total);
    const parts=['input_regular_tokens','cache_read_input_tokens','cache_write_input_tokens'].map(number);
    if(input!==null) {
      const sum=parts.filter(v=>v!==null).reduce((sum,v)=>sum+v,0n);
      require(sum<=input && (!parts.every(v=>v!==null) || sum===input));
    }
    const reasoning=number('reasoning_output_tokens');
    if(output!==null && reasoning!==null)require(reasoning<=output);
  }
  return v2;
}
export function validateTeamUsage(value) {
  require(object(value) && value.schema==='gateway-team-http/v1' && Array.isArray(value.requests));
  let v2=false;
  for(const row of value.requests) {
    require(object(row) && object(row.usage));
    if(row.usage.state==='observed') {
      require(Array.isArray(row.usage.attempts) && row.usage.attempts.length>0 && row.usage.attempts.length<=16);
      for(const attempt of row.usage.attempts)v2=event(attempt)||v2;
    } else require(['unobserved','unattributed'].includes(row.usage.state) && (!Object.hasOwn(row.usage,'attempts') || (Array.isArray(row.usage.attempts) && row.usage.attempts.length===0)));
  }
  if(v2)require(Array.isArray(value.usage_event_schemas) && value.usage_event_schemas.length===2 && schemas.every((v,i)=>value.usage_event_schemas[i]===v));
  else require(!Object.hasOwn(value,'usage_event_schemas'));
  return value;
}
export function mergeTeamUsage(previous, next) {
  validateTeamUsage(previous);validateTeamUsage(next);
  for(const key of ['schema','scope','from_ms','to_ms','window'])require(previous[key]===next[key]);
  const merged={...next,requests:[...previous.requests,...next.requests]};
  if(previous.usage_event_schemas || next.usage_event_schemas)merged.usage_event_schemas=[...schemas];
  return validateTeamUsage(merged);
}
