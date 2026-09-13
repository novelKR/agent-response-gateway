const SCHEMA = 'gateway-management-http/v1';
export class ApiError extends Error {
  constructor(code, status = 0) { super(code); this.code = code; this.status = status; }
}
const safeId = value => typeof value === 'string' && /^[A-Za-z0-9_.:-]{1,96}$/.test(value);
export function createClient(target, fetcher = globalThis.fetch.bind(globalThis)) {
  if (!safeId(target)) throw new ApiError('invalid_target');
  async function request(method, path, token) {
    const headers = { Accept: 'application/json' };
    if (token !== undefined) headers.Authorization = `Bearer ${token}`;
    let response, text = '';
    const controller = new AbortController(), timer = setTimeout(() => controller.abort(), 15000);
    try {
      response = await fetcher('/management/v1/' + path, { method, headers, credentials: 'same-origin', cache: 'no-store', redirect: 'error', signal: controller.signal });
      const reader = response.body?.getReader(), decoder = new TextDecoder('utf-8', { fatal: true });
      let size = 0;
      if (reader) for (;;) {
        const { value, done } = await reader.read(); if (done) break;
        size += value.byteLength;
        if (size > 2 * 1024 * 1024) { await reader.cancel(); throw new ApiError('response_too_large', response.status); }
        text += decoder.decode(value, { stream: true });
      }
      text += decoder.decode();
    } catch (error) { if (error instanceof ApiError) throw error; throw new ApiError('connection_unavailable'); }
    finally { clearTimeout(timer); }
    let value;
    try { value = JSON.parse(text, (_key, item, context) => {
      if (typeof item === 'number' && Number.isInteger(item) && !Number.isSafeInteger(item)) {
        if (!context?.source) throw new ApiError('unsafe_numeric_value', response.status);
        return context.source;
      }
      return item;
    }); } catch (error) { if (error instanceof ApiError) throw error; throw new ApiError('invalid_response', response.status); }
    if (!value || value.schema !== SCHEMA) throw new ApiError('unsupported_contract', response.status);
    if (!response.ok) throw new ApiError(typeof value.error?.code === 'string' ? value.error.code : 'request_failed', response.status);
    return value;
  }
  const query = extra => new URLSearchParams({ target, ...extra }).toString();
  return Object.freeze({
    login(token) {
      if (typeof token !== 'string' || !/^[\x21-\x7e]{32,4096}$/.test(token)) throw new ApiError('invalid_read_credential');
      return request('POST', 'session', token);
    },
    logout: () => request('DELETE', 'session'),
    capabilities: () => request('GET', 'capabilities?' + query({})),
    async state() {
      const result = await request('GET', 'state?' + query({}));
      const view = result.data;
      if (view?.schema !== 'gateway-management-state/v1' || !Array.isArray(view.modules) || view.modules.length > 16) throw new ApiError('unsupported_contract');
      const ids = new Set();
      for (const module of view.modules) {
        if (!module || !safeId(module.id) || ids.has(module.id) || typeof module.contract !== 'string' || !['observed', 'unobserved', 'unsupported'].includes(module.observation?.state)) throw new ApiError('invalid_response');
        ids.add(module.id);
        if (module.observation.state === 'observed' && module.observation.data?.schema !== module.contract) throw new ApiError('unsupported_contract');
      }
      return result;
    },
    operations: (after = 0) => request('GET', 'operations?' + query({ after: String(after), limit: '20' })),
    operation(id) { if (!safeId(id) || id === '.' || id === '..') throw new ApiError('invalid_operation'); return request('GET', 'operations/' + encodeURIComponent(id) + '?' + query({})); },
    usage: (from, to, timezone) => request('GET', 'usage?' + query({ from_ms: String(from), to_ms: String(to), timezone })),
  });
}
export function modulesOf(view) { return Array.isArray(view?.modules) ? view.modules : []; }
export function observed(module) { return module?.observation?.state === 'observed' ? module.observation.data : null; }
export function selectedPackages(inventory) { return inventory?.activation?.extensions || inventory?.activation?.packs || []; }
export function inventoryRows(module) {
  const state = observed(module); if (!state) return [];
  const inventory = state.store?.inventory;
  if (!Array.isArray(inventory?.installed)) return [];
  const selected = selectedPackages(inventory), effective = state.effective?.packages;
  return inventory.installed.map(item => ({ ...item,
    selected: selected.some(s => s.id === item.id && s.version === item.version && s.package_sha256 === item.package_sha256),
    effective: Array.isArray(effective) ? effective.some(s => s.id === item.id && s.version === item.version && s.package_sha256 === item.package_sha256) : null,
    grants: selected.find(s => s.id === item.id && s.version === item.version && s.package_sha256 === item.package_sha256)?.grants ?? null,
  }));
}
