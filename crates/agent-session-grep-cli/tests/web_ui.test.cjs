// Execute the shipped inline script with a small DOM boundary and controllable
// fetch promises. Deliberately ignore abort in fetch so late completions also
// exercise the request-identity guard. No packages, browser, or network needed.
const assert = require('node:assert/strict');
const { readFileSync } = require('node:fs');
const { join } = require('node:path');
const { test } = require('node:test');
const vm = require('node:vm');

const html = readFileSync(join(__dirname, '../src/web/index.html'), 'utf8');
const script = html.match(/<script>([\s\S]*?)<\/script>/)[1];
const flush = () => new Promise(resolve => setImmediate(resolve));

class Element {
  constructor(tag = 'div') {
    this.tagName = tag.toUpperCase();
    this.children = [];
    this.attributes = new Map();
    this.listeners = new Map();
    this.className = '';
    this.value = '';
    this.checked = false;
    this.hidden = false;
    this.disabled = false;
    this.dataset = {};
    this._text = '';
    this.classList = {
      add: value => this.setClass(value, true),
      remove: value => this.setClass(value, false),
      toggle: (value, force) => this.setClass(value, force ?? !this.className.split(' ').includes(value)),
    };
  }
  setClass(value, on) {
    const values = new Set(this.className.split(' ').filter(Boolean));
    if (on) values.add(value); else values.delete(value);
    this.className = [...values].join(' ');
    return on;
  }
  set textContent(value) { this._text = String(value); this.children = []; }
  get textContent() { return this._text + this.children.map(child => child.textContent).join(''); }
  append(...children) { this.children.push(...children); }
  appendChild(child) { this.append(child); return child; }
  replaceChildren(...children) { this._text = ''; this.children = children; }
  setAttribute(name, value) { this.attributes.set(name, String(value)); }
  getAttribute(name) { return this.attributes.get(name) ?? null; }
  addEventListener(type, handler) {
    if (!this.listeners.has(type)) this.listeners.set(type, []);
    this.listeners.get(type).push(handler);
  }
  dispatch(type, extra = {}) {
    return (this.listeners.get(type) || []).map(handler => handler({ target: this, ...extra }));
  }
  matches(selector) { return selector.split(',').some(tag => tag.trim().toUpperCase() === this.tagName); }
  checkValidity() {
    if (this.attributes.get('type') !== 'number') return true;
    if (!this.value) return !this.attributes.has('required');
    const number = Number(this.value);
    return Number.isInteger(number) && number >= Number(this.attributes.get('min') || '-Infinity')
      && number <= Number(this.attributes.get('max') || 'Infinity');
  }
  reportValidity() { return this.checkValidity(); }
}

async function page() {
  const nodes = new Map();
  for (const match of html.matchAll(/<([a-z][a-z0-9-]*)\b([^>]*\bid="([^"]+)"[^>]*)>/g)) {
    const node = new Element(match[1]);
    for (const attribute of match[2].matchAll(/([\w-]+)(?:="([^"]*)")?/g)) {
      node.setAttribute(attribute[1], attribute[2] || '');
    }
    node.value = node.getAttribute('value') || '';
    node.hidden = node.attributes.has('hidden');
    node.disabled = node.attributes.has('disabled');
    nodes.set(match[3], node);
  }
  for (const match of html.matchAll(/<select\b[^>]*id="([^"]+)"[^>]*>\s*<option value="([^"]*)"/g)) {
    nodes.get(match[1]).value = match[2];
  }
  const document = new Element('document');
  document.documentElement = new Element('html');
  document.activeElement = new Element('body');
  document.getElementById = id => {
    assert.ok(nodes.has(id), `missing markup id ${id}`);
    return nodes.get(id);
  };
  document.createElement = tag => new Element(tag);
  const translated = [];
  for (const match of html.matchAll(/<([a-z][a-z0-9-]*)\b([^>]*\bdata-i18n="([^"]+)"[^>]*)>/g)) {
    const id = match[2].match(/\bid="([^"]+)"/);
    const node = id ? nodes.get(id[1]) : new Element(match[1]);
    node.dataset.i18n = match[3];
    translated.push(node);
  }
  document.querySelectorAll = selector => selector === '[data-i18n]' ? translated : [];
  for (const node of nodes.values()) node.focus = () => { document.activeElement = node; };
  const storage = new Map([['asg-web-token', 'a'.repeat(32)], ['asg-web-lang', 'en']]);
  const localStorage = {
    getItem: key => storage.get(key) ?? null,
    setItem: (key, value) => storage.set(key, value),
    removeItem: key => storage.delete(key),
  };
  const window = new Element('window');
  window.location = { href: 'http://127.0.0.1:8080/' };
  window.history = { replaceState() {} };
  window.matchMedia = () => ({ matches: false });
  const pending = [];
  const response = (body, status = 200) => ({ ok: status >= 200 && status < 300, status, json: async () => body });
  const fetch = (url, options) => {
    if (url === '/api/status') return Promise.resolve(response({ data: {
      generation: 1, catalog_count: 4, web_capabilities: { provider_values: ['claude-code', 'codex', 'opencode'] },
    } }));
    if (url === '/api/providers') return Promise.resolve(response({ data: {
      providers: ['claude-code', 'codex', 'opencode', 'deferred-provider'].map(provider_id => ({ provider_id })),
    } }));
    return new Promise((resolve, reject) => pending.push({
      url, options, done: false,
      resolve(body, status) { this.done = true; resolve(response(body, status)); },
      reject(error) { this.done = true; reject(error); },
    }));
  };
  const ui = vm.createContext({ document, window, localStorage, navigator: { language: 'en' },
    URL, URLSearchParams, AbortController, DOMException, fetch, console });
  vm.runInContext(script, ui, { filename: 'embedded-web-ui.js' });
  await flush();
  return { ui, nodes, document, storage, pending, translated,
    request(prefix) {
      const request = pending.find(item => !item.done && item.url.startsWith(prefix));
      assert.ok(request, `no pending request for ${prefix}`);
      return request;
    },
  };
}

function searchBody(id, cursor = null) {
  return { data: { hits: [{ id, session_id: `ses_${id}`, text: id, score: 1 }], retrieval_mode: 'lexical' },
    page: { has_more: Boolean(cursor), next_cursor: cursor } };
}
function contextMessage(id, role, text) {
  return { id: `msg_${id}`, message_id: `msg_${id}`, placement_id: `occ_${id}`, payload: { role, text } };
}
// Match render(AppResponse::Context), ContextTalk and ContextSessionSummary.
// These are response fixtures, not a second implementation of the UI renderer.
function contextEnvelope(data = {}, outcome = 'success', evidenceCount) {
  const messages = data.messages || [];
  const evidence = messages.slice(0, evidenceCount ?? messages.length).map((message, index) => ({
    occurrence_id: message.placement_id, message_id: message.message_id,
    source_document_id: 'doc_fixture', generation: 1, source_fingerprint: 'fixture-fingerprint',
    byte_start: null, byte_end: null, line_start: null, line_end: null,
    record_ordinal: index, snippet_char_start: null, snippet_char_end: null, precision: 'unknown',
  }));
  return { command: 'context', outcome, data: {
    session_id: 'ses_fixture', session: { document: 'doc_fixture', messages: messages.map(message => message.id) },
    branch_leaf: messages.at(-1)?.message_id || null,
    branch_leaf_placement_id: messages.at(-1)?.placement_id || null,
    messages, evidence, tool_activities: [], requested_level: 'raw', effective_level: 'raw',
    talks: [], summary: null, hint: null, truncation: { truncated: false, reason: null }, generation: 1,
    ...data,
  }, page: { has_more: false, next_cursor: null }, warnings: evidence.length ? [
    `${evidence.length} of ${evidence.length} evidence spans have unknown precision (legacy rows; re-ingest to restore byte spans)`,
  ] : [] };
}
function contextBody(text) {
  return contextEnvelope({ messages: [contextMessage(text, 'user', text)] });
}
function levelBody(level, { empty = false, partial = false, reason = 'max_messages' } = {}) {
  const messages = empty ? [] : [
    contextMessage('system', 'system', '系统提示 🧭 must survive structural grouping'),
    contextMessage('user', 'user', '问题 👩🏽‍💻 e\u0301 <img src=x onerror=alert(1)>'),
    contextMessage('assistant', 'assistant', '回答 😀 <script>not executable</script>'),
    contextMessage('tool', 'tool', '历史工具失败不是当前读取失败 🔧'),
  ];
  // With no user anchor, Application actually falls back to raw, even when
  // talks/sessions was requested. Do not invent empty summary/talk DTOs.
  const effective = empty ? 'raw' : level;
  return contextEnvelope({
    messages, requested_level: level, effective_level: effective,
    talks: effective === 'talks' ? [{ user_message: messages[1], following_messages: messages.slice(2) }] : [],
    summary: effective === 'sessions' ? {
      first_user_message: messages[1], message_count: 4, turn_count: 1, file_references: ['doc_fixture'],
    } : null,
    hint: effective === 'raw' ? null : {
      command: 'get_session_context', session_id: 'ses_fixture', level: effective === 'sessions' ? 'talks' : 'raw',
    },
    truncation: { truncated: partial, reason: partial ? reason : null },
  }, partial ? 'partial' : 'success', partial && reason.includes('max_evidence_spans') ? 2 : undefined);
}
function descendants(node) {
  return node.children.flatMap(child => [child, ...descendants(child)]);
}
function paramsOf(request) {
  return new URL(request.url, 'http://localhost').searchParams;
}
async function search(p, id = 'first', cursor = null) {
  p.nodes.get('query').value = 'needle';
  const task = p.ui.doSearch();
  p.request('/api/search?').resolve(searchBody(id, cursor));
  await task;
}

const SEARCH_EDITS = {
  query: 'different needle', provider: 'opencode', mode: 'hybrid', limit: '3', maxBytes: '8192',
  repo: 'owner/changed', since: '2026-01-01T00:00:00Z', until: '2027-01-01T00:00:00Z',
  sidechain: 'main_only', toolKind: 'command', toolName: 'shell', includeSystem: true, groupBySession: true,
};
function editSearchControl(p, id) {
  const control = p.nodes.get(id);
  const property = typeof SEARCH_EDITS[id] === 'boolean' ? 'checked' : 'value';
  assert.notEqual(control[property], SEARCH_EDITS[id], `${id} must actually change`);
  control[property] = SEARCH_EDITS[id];
}
function enterQuery(p) {
  const query = p.nodes.get('query');
  query.focus();
  const event = { key: 'Enter', target: query, preventDefault() {} };
  query.dispatch('keydown', event);
  // The harness does not bubble automatically. Exercise the real document
  // listener too, so a second global Enter handler would create a second fetch.
  p.document.dispatch('keydown', event);
}

test('all visible filters reach the API and providers use the capability registry', async () => {
  const p = await page();
  const fields = { query: 'needle', mode: 'hybrid', limit: '3', maxBytes: '4096', provider: 'opencode',
    repo: 'owner/repo', since: '2026-01-01T00:00:00Z', until: '2027-01-01T00:00:00Z',
    sidechain: 'subagent_only', toolKind: 'command', toolName: 'shell' };
  for (const [id, value] of Object.entries(fields)) p.nodes.get(id).value = value;
  p.nodes.get('includeSystem').checked = true;
  p.nodes.get('groupBySession').checked = true;
  const task = p.ui.doSearch();
  const request = p.request('/api/search?');
  assert.deepEqual(Object.fromEntries(new URL(request.url, 'http://localhost').searchParams), {
    q: 'needle', mode: 'hybrid', limit: '3', max_bytes: '4096', provider: 'opencode', repo: 'owner/repo',
    since: fields.since, until: fields.until, sidechain: 'subagent_only', tool_kind: 'command',
    tool_name: 'shell', include_system: 'true', group_by_session: 'true',
  });
  assert.ok(!p.nodes.get('provider').children.some(option => option.value === 'deferred-provider'));
  assert.equal(p.nodes.get('results').getAttribute('aria-busy'), 'true');
  request.resolve(searchBody('filtered'));
  await task;
  assert.equal(p.nodes.get('results').getAttribute('aria-busy'), 'false');
});

for (const failure of [false, true]) test(`late search ${failure ? 'failure' : 'success'} cannot replace a newer result`, async () => {
  const p = await page();
  p.nodes.get('query').value = 'old';
  const oldTask = p.ui.doSearch();
  const old = p.request('/api/search?');
  p.nodes.get('query').value = 'new';
  const newTask = p.ui.doSearch();
  const newer = p.pending.at(-1);
  assert.equal(old.options.signal.aborted, true);
  newer.resolve(searchBody('new-result', 'new-page'));
  await newTask;
  if (failure) old.reject(new Error('old failure')); else old.resolve(searchBody('old-result', 'old-page'));
  await oldTask;
  assert.match(p.nodes.get('results').textContent, /new-result/);
  assert.doesNotMatch(p.nodes.get('results').textContent, /old-result|old failure/);
  assert.equal(p.nodes.get('loadMore').disabled, false);
});

test('each search input invalidates its cursor and both change events retain feedback', async () => {
  const ids = ['query', 'provider', 'mode', 'limit', 'maxBytes', 'repo', 'since', 'until',
    'sidechain', 'toolKind', 'toolName', 'includeSystem', 'groupBySession'];
  for (const id of ids) {
    const p = await page();
    await search(p, 'first', 'page-two');
    editSearchControl(p, id);
    p.nodes.get(id).dispatch('input');
    p.nodes.get(id).dispatch('change');
    assert.equal(p.nodes.get('loadMore').disabled, true, id);
    assert.equal(p.nodes.get('handoff').disabled, true, id);
    assert.equal(p.nodes.get('results').children.length, 0, id);
    assert.match(p.nodes.get('searchFeedback').textContent, /criteria changed/, id);
    const count = p.pending.length;
    p.nodes.get('loadMore').dispatch('click');
    assert.equal(p.pending.length, count, id);
  }
});

test('cursor guard also rejects programmatic input changes and duplicate page loads', async () => {
  const p = await page();
  await search(p, 'first', 'page-two');
  const firstPage = p.ui.doSearch('page-two');
  await p.ui.doSearch('page-two');
  assert.equal(p.pending.filter(item => !item.done).length, 1);
  p.request('/api/search?').resolve(searchBody('second', 'page-three'));
  await firstPage;
  assert.match(p.nodes.get('resultMeta').textContent, /2$/);
  p.nodes.get('repo').value = 'different/repo';
  const count = p.pending.length;
  await p.ui.doSearch('page-three');
  assert.equal(p.pending.length, count);
  assert.equal(p.nodes.get('loadMore').disabled, true);
});

test('clearing a query or failing a request resets paging, busy state and visible results', async () => {
  const p = await page();
  await search(p, 'first', 'page-two');
  const task = p.ui.doSearch('page-two');
  const old = p.request('/api/search?');
  p.nodes.get('query').value = '';
  await p.ui.doSearch();
  old.resolve(searchBody('obsolete', 'obsolete-page'));
  await task;
  assert.equal(p.nodes.get('results').textContent, '');
  assert.match(p.nodes.get('searchFeedback').textContent, /Enter a search query/);
  assert.equal(p.nodes.get('loadMore').disabled, true);
  p.nodes.get('query').value = 'failed';
  const failing = p.ui.doSearch();
  p.request('/api/search?').resolve({ error: { message: 'synthetic error' } }, 400);
  await failing;
  assert.match(p.nodes.get('searchFeedback').textContent, /synthetic error/);
  assert.equal(p.nodes.get('results').getAttribute('aria-busy'), 'false');
  assert.equal(p.nodes.get('loadMore').disabled, true);
});

for (const failure of [false, true]) test(`late context ${failure ? 'failure' : 'success'} cannot overwrite a new session`, async () => {
  const p = await page();
  const first = p.ui.loadContext('ses_a');
  const old = p.request('/api/context?');
  const second = p.ui.loadContext('ses_b');
  assert.equal(p.nodes.get('contextMessages').textContent, '');
  p.pending.at(-1).resolve(contextBody('new-session'));
  await second;
  if (failure) old.reject(new Error('old error')); else old.resolve(contextBody('old-session'));
  await first;
  assert.match(p.nodes.get('contextMessages').textContent, /new-session/);
  assert.doesNotMatch(p.nodes.get('contextMessages').textContent, /old-session|old error/);
  assert.equal(p.nodes.get('contextMessages').getAttribute('aria-busy'), 'false');
});

test('context policy changes reload and closing prevents context or preview resurrection', async () => {
  const p = await page();
  await search(p);
  p.nodes.get('results').children[0].dispatch('click');
  const old = p.request('/api/context?');
  p.nodes.get('contextPolicy').value = 'full';
  p.nodes.get('contextPolicy').dispatch('change');
  const current = p.pending.at(-1);
  assert.match(current.url, /policy=full/);
  current.resolve(contextBody('full-context'));
  await flush();
  const previewTask = p.ui.previewResume();
  const preview = p.request('/api/resume?');
  p.nodes.get('closeContext').dispatch('click');
  old.resolve(contextBody('late-context'));
  preview.resolve({ data: { command: 'synthetic resume' } });
  await previewTask;
  await flush();
  assert.equal(p.nodes.get('contextMessages').textContent, '');
  assert.equal(p.nodes.get('previewOutput').hidden, true);
  assert.equal(p.nodes.get('contextPanel').hidden, true);
  assert.ok(!p.nodes.get('results').children[0].className.includes('active'));
});

test('an old unauthorized response cannot discard a newly accepted token', async () => {
  const p = await page();
  p.nodes.get('query').value = 'old';
  const task = p.ui.doSearch();
  const old = p.request('/api/search?');
  p.ui.acceptToken('b'.repeat(32));
  await flush();
  old.resolve({}, 401);
  await task;
  assert.equal(p.storage.get('asg-web-token'), 'b'.repeat(32));
  assert.equal(p.nodes.get('gate').hidden, true);
});

test('slash stays editable in filter inputs and a language switch preserves all loaded hits', async () => {
  const p = await page();
  for (const id of ['repo', 'since', 'toolName', 'mode']) {
    p.nodes.get(id).focus();
    let prevented = false;
    p.document.dispatch('keydown', { key: '/', preventDefault() { prevented = true; } });
    assert.equal(prevented, false, id);
    assert.equal(p.document.activeElement, p.nodes.get(id));
  }
  await search(p, 'first', 'page-two');
  const task = p.ui.doSearch('page-two');
  p.request('/api/search?').resolve(searchBody('second'));
  await task;
  p.nodes.get('langToggle').dispatch('click');
  await flush();
  assert.equal(p.nodes.get('results').children.length, 2);
  assert.match(p.nodes.get('resultMeta').textContent, /2$/);
});

const contextBudgets = [['contextMaxMessages', 'max_messages'], ['contextMaxBytes', 'max_bytes']];
const handoffBudgets = [['handoffMaxEvidence', 'max_evidence'], ['handoffMaxTokens', 'max_tokens'], ['handoffMaxBytes', 'max_bytes']];

test('read budgets have optional labelled controls and bilingual default/scope explanations', async () => {
  const p = await page();
  for (const [id] of [...contextBudgets, ...handoffBudgets]) {
    const control = p.nodes.get(id);
    assert.ok(control, `missing optional control ${id}`);
    assert.equal(control.getAttribute('type'), 'number');
    assert.equal(control.value, '');
    assert.equal(control.attributes.has('required'), false);
    assert.match(html, new RegExp(`<label[^>]*><span data-i18n="${id}"></span><input id="${id}"`));
    const label = p.translated.find(node => node.dataset.i18n === id);
    assert.match(label.textContent, /Context|Handoff/);
  }
  assert.match(p.nodes.get('readBudgetHint').textContent, /server defaults.*0/);
  assert.match(p.nodes.get('handoffScopeHint').textContent, /q\/provider\/since\/until/);
  assert.match(p.nodes.get('handoffScopeHint').textContent, /does not inherit repo, sidechain, tool.*mode/);
  p.nodes.get('langToggle').dispatch('click');
  await flush();
  assert.match(p.nodes.get('readBudgetHint').textContent, /服务端默认.*0/);
  assert.match(p.nodes.get('handoffScopeHint').textContent, /不继承.*repo.*sidechain.*tool.*mode/);
  for (const [id] of [...contextBudgets, ...handoffBudgets]) {
    assert.match(p.translated.find(node => node.dataset.i18n === id).textContent, /上下文|交接包/);
  }
});

test('handoff cannot use a pending, failed or invalidated search as an accepted snapshot', async () => {
  const p = await page();
  p.nodes.get('query').value = 'not accepted yet';
  const task = p.ui.doSearch();
  const request = p.request('/api/search?');
  const count = p.pending.length;
  const earlyPreview = p.ui.previewHandoff();
  assert.equal(p.pending.length, count, 'pending search must not enable a Handoff request');
  await earlyPreview;
  request.resolve({ error: { message: 'synthetic failure' } }, 400);
  await task;
  await p.ui.previewHandoff();
  assert.equal(p.pending.length, count, 'failed search must not supply a Handoff query');
  await search(p);
  editSearchControl(p, 'since');
  p.nodes.get('since').dispatch('input');
  const invalidatedCount = p.pending.length;
  await p.ui.previewHandoff();
  assert.equal(p.pending.length, invalidatedCount);
});

test('handoff inherits only q/provider/since/until from the accepted request, not live controls', async () => {
  const p = await page();
  const fields = { query: '中文 👩🏽‍💻 & + "quoted"', provider: 'opencode',
    since: '2026-01-01T08:00:00+08:00', until: '2026-02-01T08:00:00+08:00',
    mode: 'hybrid', repo: 'owner/repo', sidechain: 'subagent_only', toolKind: 'command', toolName: 'shell' };
  for (const [id, value] of Object.entries(fields)) p.nodes.get(id).value = value;
  p.nodes.get('includeSystem').checked = true;
  p.nodes.get('groupBySession').checked = true;
  const task = p.ui.doSearch();
  // Mutating the form before acceptance must not mutate the request snapshot.
  for (const id of ['query', 'provider', 'since', 'until']) p.nodes.get(id).value = 'unsubmitted';
  p.request('/api/search?').resolve(searchBody('accepted'));
  await task;
  p.nodes.get('handoffMaxEvidence').value = '0';
  p.nodes.get('handoffMaxTokens').value = '64';
  p.nodes.get('handoffMaxBytes').value = '4096';
  const preview = p.ui.previewHandoff();
  const request = p.request('/api/handoff?');
  assert.deepEqual(Object.fromEntries(paramsOf(request)), {
    q: fields.query, provider: fields.provider, since: fields.since, until: fields.until,
    max_evidence: '0', max_tokens: '64', max_bytes: '4096',
  });
  request.resolve({ data: { text: 'accepted preview 😀' } });
  await preview;
  assert.match(p.nodes.get('previewOutput').textContent, /accepted preview 😀/);
});

for (const kind of ['context', 'handoff']) test(`${kind} budgets omit blanks and forward zero/large values without clamping`, async () => {
  const p = await page();
  const fields = kind === 'context' ? contextBudgets : handoffBudgets;
  if (kind === 'handoff') await search(p);
  for (const value of ['', '0', '4096', '4294967296']) {
    for (const [id] of fields) {
      assert.ok(p.nodes.has(id), `missing budget field ${id}`);
      p.nodes.get(id).value = value;
    }
    const task = kind === 'context' ? p.ui.loadContext('ses_fixture') : p.ui.previewHandoff();
    const request = p.request(`/api/${kind}?`);
    for (const [, name] of fields) assert.equal(paramsOf(request).get(name), value === '' ? null : value);
    assert.equal(request.options.headers.Authorization, 'Bearer ' + 'a'.repeat(32));
    // Budget rejection belongs to the server, not a replacement/default in JS.
    request.resolve(value === '0' ? { error: { message: 'budget below floor' } }
      : kind === 'context' ? contextBody('bounded') : { data: { text: 'bounded' } }, value === '0' ? 400 : 200);
    await task;
    if (value === '0') assert.match(p.nodes.get(kind === 'context' ? 'contextFeedback' : 'previewOutput').textContent, /budget below floor/);
  }
});

test('a superseded search response cannot replace the accepted Handoff filters', async () => {
  const p = await page();
  p.nodes.get('query').value = 'old';
  p.nodes.get('provider').value = 'codex';
  const oldTask = p.ui.doSearch();
  const old = p.request('/api/search?');
  p.nodes.get('query').value = 'new';
  p.nodes.get('provider').value = 'opencode';
  const newTask = p.ui.doSearch();
  p.pending.at(-1).resolve(searchBody('new'));
  await newTask;
  old.resolve(searchBody('old'));
  await oldTask;
  const preview = p.ui.previewHandoff();
  const request = p.request('/api/handoff?');
  assert.deepEqual(Object.fromEntries(paramsOf(request)), { q: 'new', provider: 'opencode' });
  request.resolve({ data: {} });
  await preview;
});

test('every search edit cancels and clears an old Handoff preview', async () => {
  for (const id of ['query', 'provider', 'mode', 'limit', 'maxBytes', 'repo', 'since', 'until',
    'sidechain', 'toolKind', 'toolName', 'includeSystem', 'groupBySession']) {
    const p = await page();
    await search(p);
    const visible = p.ui.previewHandoff();
    p.request('/api/handoff?').resolve({ data: { text: 'visible old preview' } });
    await visible;
    const task = p.ui.previewHandoff();
    const old = p.request('/api/handoff?');
    editSearchControl(p, id);
    p.nodes.get(id).dispatch('input');
    assert.equal(old.options.signal.aborted, true, id);
    assert.equal(p.nodes.get('previewOutput').hidden, true, id);
    old.resolve({ data: { text: 'obsolete preview' } });
    await task;
    assert.equal(p.nodes.get('previewOutput').textContent, '', id);
  }
});

for (const failure of [false, true]) test(`Handoff budget edits reject late preview ${failure ? 'errors' : 'successes'}`, async () => {
  for (const [id, name] of handoffBudgets) {
    const p = await page();
    await search(p);
    const visible = p.ui.previewHandoff();
    p.request('/api/handoff?').resolve({ data: { text: 'old visible preview' } });
    await visible;
    const oldTask = p.ui.previewHandoff();
    const old = p.request('/api/handoff?');
    assert.ok(p.nodes.has(id), `missing budget field ${id}`);
    p.nodes.get(id).value = '0';
    p.nodes.get(id).dispatch('input');
    p.nodes.get(id).dispatch('change');
    assert.equal(old.options.signal.aborted, true, id);
    assert.equal(p.nodes.get('previewOutput').hidden, true, id);
    const currentTask = p.ui.previewHandoff();
    const current = p.pending.at(-1);
    assert.equal(paramsOf(current).get(name), '0');
    current.resolve({ data: { text: '当前交接包 😀' } });
    await currentTask;
    if (failure) old.reject(new Error('obsolete error')); else old.resolve({ data: { text: 'obsolete success' } });
    await oldTask;
    p.nodes.get('langToggle').dispatch('click');
    await flush();
    assert.match(p.nodes.get('previewOutput').textContent, /当前交接包 😀/);
    assert.doesNotMatch(p.nodes.get('previewOutput').textContent, /obsolete|old visible/);
  }
});

test('context budget edits cancel context and preview work before reloading the new budget', async () => {
  for (const [id, name] of contextBudgets) {
    const p = await page();
    await search(p);
    const oldTask = p.ui.loadContext('ses_fixture');
    const old = p.request('/api/context?');
    const previewTask = p.ui.previewHandoff();
    const preview = p.request('/api/handoff?');
    assert.ok(p.nodes.has(id), `missing budget field ${id}`);
    p.nodes.get(id).value = '4096';
    p.nodes.get(id).dispatch('input');
    assert.equal(old.options.signal.aborted, true);
    assert.equal(preview.options.signal.aborted, true);
    assert.equal(p.nodes.get('contextMessages').getAttribute('aria-busy'), 'false');
    p.nodes.get(id).dispatch('change');
    const current = p.pending.at(-1);
    assert.equal(paramsOf(current).get(name), '4096');
    current.resolve(contextBody('new budget response'));
    await flush();
    old.resolve(contextBody('obsolete context'));
    preview.resolve({ data: { text: 'obsolete preview' } });
    await Promise.all([oldTask, previewTask]);
    assert.match(p.nodes.get('contextMessages').textContent, /new budget response/);
    assert.doesNotMatch(p.nodes.get('contextMessages').textContent, /obsolete/);
    assert.equal(p.nodes.get('previewOutput').hidden, true);
  }
});

for (const level of ['raw', 'talks', 'sessions']) {
  for (const state of ['nonempty', 'empty', 'partial', 'empty partial']) {
    test(`${level} context renders actual DTOs for ${state} and survives a language switch`, async () => {
      const p = await page();
      const empty = state.includes('empty') && state !== 'nonempty';
      const partial = state.includes('partial');
      const reason = empty ? 'max_response_bytes' : level === 'raw' ? 'max_messages'
        : level === 'talks' ? 'max_response_bytes' : 'max_evidence_spans';
      const body = levelBody(level, { empty, partial, reason });
      p.nodes.get('contextLevel').value = level;
      const task = p.ui.loadContext('ses_fixture');
      p.request('/api/context?').resolve(body);
      await task;
      const container = p.nodes.get('contextMessages');
      const feedback = p.nodes.get('contextFeedback');
      assert.match(container.textContent, new RegExp(`Requested level: ${level}`));
      assert.match(container.textContent, new RegExp(`Effective level: ${body.data.effective_level}`));
      assert.match(container.textContent, new RegExp(`Returned: ${empty ? 0 : 4} messages`));
      assert.match(container.textContent, new RegExp(`${body.data.evidence.length} evidence spans`));
      assert.equal(container.getAttribute('aria-busy'), 'false');
      assert.equal(feedback.className.includes('error'), false);
      if (partial) {
        assert.match(feedback.textContent, /Partial context/);
        assert.match(container.textContent, new RegExp(`Truncation reason: ${reason}`));
        assert.doesNotMatch(container.textContent, /total messages|omitted messages/i);
      } else {
        assert.doesNotMatch(feedback.textContent, /Partial context|Could not load/);
        assert.doesNotMatch(container.textContent, /Truncation reason/);
      }
      if (empty) {
        assert.match(container.textContent, partial ? /No messages returned within this response budget/ : /No messages were returned/);
        if (level !== 'raw') assert.match(container.textContent, /Fallback/);
      } else {
        for (const message of body.data.messages) assert.ok(container.textContent.includes(message.payload.text));
        for (const warning of body.warnings) assert.ok(feedback.textContent.includes(warning));
        assert.doesNotMatch(container.textContent, /No messages/);
        assert.equal(descendants(container).some(node => ['SCRIPT', 'IMG'].includes(node.tagName)), false);
        const sections = container.children.filter(node => node.tagName === 'SECTION');
        if (level !== 'raw') {
          const returned = sections.at(-1);
          assert.equal(returned.children[0].textContent, 'Returned messages');
          assert.deepEqual(returned.children.filter(node => node.tagName === 'ARTICLE').map(node => node.children[1].textContent),
            body.data.messages.map(message => message.payload.text));
        }
        if (level === 'talks') {
          assert.equal(sections[0].children[0].textContent, 'Talk 1');
          assert.deepEqual(sections[0].children.filter(node => node.tagName === 'ARTICLE').map(node => node.children[1].textContent),
            [body.data.talks[0].user_message, ...body.data.talks[0].following_messages].map(message => message.payload.text));
        }
        if (level === 'sessions') {
          assert.deepEqual(sections[0].children.filter(node => node.tagName === 'ARTICLE').map(node => node.children[1].textContent),
            [body.data.summary.first_user_message.payload.text]);
          assert.match(container.textContent, /Session summary/);
          assert.match(container.textContent, /Summary messages: 4/);
          assert.match(container.textContent, /Summary turns: 1/);
          assert.match(container.textContent, /doc_fixture/);
        }
        if (body.data.hint) {
          assert.match(container.textContent, /get_session_context/);
          assert.match(container.textContent, new RegExp(`level=${body.data.hint.level}`));
        }
      }
      const count = p.pending.length;
      p.nodes.get('langToggle').dispatch('click');
      await flush();
      assert.equal(p.pending.length, count, 'language changes must not refetch or clear context');
      assert.match(container.textContent, new RegExp(`请求层级：${level}`));
      assert.match(container.textContent, new RegExp(`实际层级：${body.data.effective_level}`));
      if (partial) assert.match(feedback.textContent, /上下文仅部分返回/);
      for (const message of body.data.messages) assert.ok(container.textContent.includes(message.payload.text));
      for (const warning of body.warnings) assert.ok(feedback.textContent.includes(warning));
      if (empty) assert.match(container.textContent, /未返回消息/);
    });
  }
}

test('effective level and supplied hint win over live selection; entity actions keep exact IDs', async () => {
  const p = await page();
  p.nodes.get('contextLevel').value = 'talks';
  const task = p.ui.loadContext('ses_fixture');
  p.nodes.get('contextLevel').value = 'sessions';
  p.request('/api/context?').resolve(levelBody('talks'));
  await task;
  const container = p.nodes.get('contextMessages');
  assert.match(container.textContent, /Effective level: talks/);
  assert.match(container.textContent, /Talk 1/);
  assert.doesNotMatch(container.textContent, /Summary messages/);
  assert.match(container.textContent, /get_session_context.*ses_fixture.*level=raw/);
  const show = descendants(container).find(node => node.tagName === 'BUTTON' && node.title === 'msg_user');
  assert.ok(show, 'structured messages keep their show action');
  show.dispatch('click');
  const request = p.request('/api/show?');
  assert.equal(paramsOf(request).get('id'), 'msg_user');
  request.resolve({ data: { text: '<script>实体 😀</script>' } });
  await flush();
  assert.match(p.nodes.get('previewOutput').textContent, /<script>实体 😀<\/script>/);
});

test('nonempty structural fallback renders raw messages without inventing talks or summaries', async () => {
  for (const level of ['talks', 'sessions']) {
    const p = await page();
    p.nodes.get('contextLevel').value = level;
    const task = p.ui.loadContext('ses_fixture');
    p.request('/api/context?').resolve(contextEnvelope({ requested_level: level, effective_level: 'raw',
      messages: [contextMessage('assistant', 'assistant', 'No user anchor 😀')] }));
    await task;
    assert.match(p.nodes.get('contextMessages').textContent, /Fallback.*Effective level: raw/s);
    assert.match(p.nodes.get('contextMessages').textContent, /No user anchor 😀/);
    assert.doesNotMatch(p.nodes.get('contextMessages').textContent, /Talk 1|Summary messages/);
  }
});

for (const failure of [false, true]) test(`context option/language changes reject stale ${failure ? 'errors' : 'successes'} without extra requests`, async () => {
  for (const [id, value] of [['contextLevel', 'talks'], ['contextPolicy', 'full']]) {
    const p = await page();
    const oldTask = p.ui.loadContext('ses_fixture');
    const old = p.request('/api/context?');
    const previewTask = p.ui.previewResume();
    const preview = p.request('/api/resume?');
    p.nodes.get(id).value = value;
    p.nodes.get(id).dispatch('change');
    const current = p.pending.at(-1);
    assert.equal(old.options.signal.aborted, true);
    assert.equal(preview.options.signal.aborted, true);
    const count = p.pending.length;
    p.nodes.get('langToggle').dispatch('click');
    await flush();
    assert.equal(p.pending.length, count);
    assert.equal(p.nodes.get('contextMessages').getAttribute('aria-busy'), 'true');
    assert.match(p.nodes.get('contextFeedback').textContent, /正在加载会话/);
    current.resolve(levelBody(id === 'contextLevel' ? 'talks' : 'raw'));
    await flush();
    if (failure) old.reject(new Error('obsolete error')); else old.resolve(contextBody('obsolete context'));
    preview.resolve({ data: { text: 'obsolete preview' } });
    await Promise.all([oldTask, previewTask]);
    assert.match(p.nodes.get('contextMessages').textContent, /问题 👩🏽‍💻/);
    assert.doesNotMatch(p.nodes.get('contextMessages').textContent, /obsolete/);
    assert.equal(p.nodes.get('previewOutput').hidden, true);
  }
});

test('context failure stays distinct from empty/partial and is preserved through language changes', async () => {
  const p = await page();
  const first = p.ui.loadContext('ses_fixture');
  p.request('/api/context?').resolve(levelBody('sessions'));
  await first;
  const failing = p.ui.refreshContext();
  p.request('/api/context?').resolve({ error: { message: 'synthetic error 失败 😀' } }, 400);
  await failing;
  assert.equal(p.nodes.get('contextMessages').textContent, '');
  assert.match(p.nodes.get('contextFeedback').textContent, /Could not load session: synthetic error 失败 😀/);
  assert.equal(p.nodes.get('contextFeedback').className.includes('error'), true);
  const count = p.pending.length;
  p.nodes.get('langToggle').dispatch('click');
  await flush();
  assert.equal(p.pending.length, count);
  assert.equal(p.nodes.get('contextMessages').textContent, '');
  assert.match(p.nodes.get('contextFeedback').textContent, /会话加载失败：synthetic error 失败 😀/);
  assert.doesNotMatch(p.nodes.get('contextFeedback').textContent, /未返回|部分返回/);
});

for (const kind of ['context', 'handoff']) test(`${kind} rejects native invalid numeric input instead of treating it as an omitted budget`, async () => {
  const p = await page();
  if (kind === 'handoff') await search(p);
  const control = p.nodes.get(kind === 'context' ? 'contextMaxBytes' : 'handoffMaxBytes');
  // A number input can have an empty value while the browser reports badInput
  // (e.g. an unfinished exponent). This is not the optional blank/default case.
  control.value = '';
  control.checkValidity = () => false;
  let reported = false;
  control.reportValidity = () => { reported = true; return false; };
  const count = p.pending.length;
  const task = kind === 'context' ? p.ui.loadContext('ses_fixture') : p.ui.previewHandoff();
  assert.equal(p.pending.length, count, 'invalid input must not silently request defaults');
  await task;
  assert.equal(reported, true);
  assert.match(p.nodes.get(kind === 'context' ? 'contextFeedback' : 'previewOutput').textContent, /Check the numeric filters/);
});

test('sessions-to-talks fallback preserves its supplied hint and compound truncation facts', async () => {
  const p = await page();
  const body = levelBody('talks', { partial: true, reason: 'max_messages,max_response_bytes,max_evidence_spans' });
  body.data.requested_level = 'sessions';
  p.nodes.get('contextLevel').value = 'sessions';
  const task = p.ui.loadContext('ses_fixture');
  p.request('/api/context?').resolve(body);
  await task;
  assert.match(p.nodes.get('contextMessages').textContent, /Fallback.*Requested level: sessions.*Effective level: talks/);
  assert.match(p.nodes.get('contextMessages').textContent, /max_messages,max_response_bytes,max_evidence_spans/);
  assert.match(p.nodes.get('contextMessages').textContent, /Returned: 4 messages · 2 evidence spans/);
  assert.match(p.nodes.get('contextMessages').textContent, /get_session_context.*level=raw/);
  assert.doesNotMatch(p.nodes.get('contextMessages').textContent, /Summary messages/);
  assert.match(p.nodes.get('contextFeedback').textContent, /Partial context.*2 of 2 evidence spans/);
});

// Native text input commits can follow Enter's keydown even when the submitted
// value has not changed. They must not invalidate that very request.
test('fresh query Enter bubbles once and its unchanged native change keeps the search alive', async () => {
  const p = await page();
  const query = p.nodes.get('query');
  query.value = 'gateprobe';
  query.dispatch('input');
  enterQuery(p);
  assert.equal(p.pending.length, 1, 'query/document keydown must create exactly one search');
  const request = p.request('/api/search?');
  assert.equal(paramsOf(request).get('q'), 'gateprobe');
  assert.equal(request.options.signal.aborted, false);
  query.dispatch('change');
  assert.equal(request.options.signal.aborted, false, 'unchanged Enter commit must not cancel its search');
  assert.equal(p.nodes.get('results').getAttribute('aria-busy'), 'true');
  assert.match(p.nodes.get('searchFeedback').textContent, /Searching/);
  assert.equal(p.nodes.get('handoff').disabled, true, 'Handoff still waits for successful acceptance');
  const body = searchBody('gateprobe-0', 'page-two');
  body.data.hits = Array.from({ length: 4 }, (_, index) => searchBody(`gateprobe-${index}`).data.hits[0]);
  request.resolve(body);
  await flush();
  assert.equal(p.pending.length, 1);
  assert.equal(p.nodes.get('results').children.length, 4);
  assert.match(p.nodes.get('resultMeta').textContent, /4$/);
  assert.equal(p.nodes.get('results').getAttribute('aria-busy'), 'false');
  assert.equal(p.nodes.get('loadMore').disabled, false);
  assert.equal(p.nodes.get('handoff').disabled, false);
  assert.doesNotMatch(p.nodes.get('searchFeedback').textContent, /criteria changed/);
});

test('unchanged search input/change events preserve accepted results, context, previews, budgets and paging', async () => {
  const p = await page();
  await search(p, 'first', 'page-two');
  const budgets = { contextMaxMessages: '12', contextMaxBytes: '4096',
    handoffMaxEvidence: '0', handoffMaxTokens: '64', handoffMaxBytes: '8192' };
  for (const [id, value] of Object.entries(budgets)) p.nodes.get(id).value = value;
  p.nodes.get('results').children[0].dispatch('click');
  p.request('/api/context?').resolve(contextBody('selected context 😀'));
  await flush();
  const preview = p.ui.previewHandoff();
  p.request('/api/handoff?').resolve({ data: { text: 'accepted preview 😀' } });
  await preview;
  const requestCount = p.pending.length;
  for (const id of Object.keys(SEARCH_EDITS)) {
    p.nodes.get(id).dispatch('input');
    p.nodes.get(id).dispatch('change');
    assert.equal(p.nodes.get('loadMore').disabled, false, id);
    assert.equal(p.nodes.get('handoff').disabled, false, id);
    assert.match(p.nodes.get('results').textContent, /first/, id);
    assert.ok(p.nodes.get('results').children[0].className.includes('active'), id);
    assert.equal(p.nodes.get('contextPanel').hidden, false, id);
    assert.match(p.nodes.get('contextMessages').textContent, /selected context 😀/, id);
    assert.equal(p.nodes.get('previewOutput').hidden, false, id);
    assert.match(p.nodes.get('previewOutput').textContent, /accepted preview 😀/, id);
  }
  assert.equal(p.pending.length, requestCount);
  for (const [id, value] of Object.entries(budgets)) assert.equal(p.nodes.get(id).value, value, id);
  const nextPage = p.ui.doSearch('page-two');
  const request = p.request('/api/search?');
  assert.equal(paramsOf(request).get('cursor'), 'page-two');
  p.nodes.get('query').dispatch('change');
  assert.equal(request.options.signal.aborted, false, 'matching commit must also preserve an active page request');
  request.resolve(searchBody('second'));
  await nextPage;
  assert.equal(p.nodes.get('results').children.length, 2);
  assert.match(p.nodes.get('contextMessages').textContent, /selected context 😀/);
  assert.match(p.nodes.get('previewOutput').textContent, /accepted preview 😀/);
});

for (const event of ['input', 'change']) test(`genuine ${event} edits after Enter still invalidate and reject stale results`, async () => {
  for (const failure of [false, true]) {
    const p = await page();
    const query = p.nodes.get('query');
    query.value = 'old gateprobe';
    query.dispatch('input');
    enterQuery(p);
    const old = p.request('/api/search?');
    query.value = 'new gateprobe';
    query.dispatch(event);
    assert.equal(old.options.signal.aborted, true, event);
    assert.equal(p.nodes.get('loadMore').disabled, true);
    assert.equal(p.nodes.get('handoff').disabled, true);
    assert.equal(p.nodes.get('results').getAttribute('aria-busy'), 'false');
    assert.match(p.nodes.get('searchFeedback').textContent, /criteria changed/);
    enterQuery(p);
    const current = p.pending.at(-1);
    assert.equal(p.pending.length, 2);
    assert.equal(paramsOf(current).get('q'), 'new gateprobe');
    query.dispatch('change');
    assert.equal(current.options.signal.aborted, false, 'the next unchanged commit is not another edit');
    current.resolve(searchBody('current-hit', 'current-page'));
    await flush();
    if (failure) old.reject(new Error('obsolete failure')); else old.resolve(searchBody('obsolete-hit', 'obsolete-page'));
    await flush();
    assert.match(p.nodes.get('results').textContent, /current-hit/);
    assert.doesNotMatch(p.nodes.get('results').textContent, /obsolete/);
    assert.doesNotMatch(p.nodes.get('searchFeedback').textContent, /criteria changed|obsolete/);
    assert.equal(p.nodes.get('loadMore').disabled, false);
    assert.equal(p.nodes.get('handoff').disabled, false);
  }
});

test('native invalid numeric input still invalidates when its empty value serializes like an omitted field', async () => {
  const p = await page();
  await search(p, 'first', 'page-two');
  const nextPage = p.ui.doSearch('page-two');
  const request = p.request('/api/search?');
  const control = p.nodes.get('maxBytes');
  assert.equal(control.value, '');
  control.checkValidity = () => false;
  control.dispatch('input');
  control.dispatch('change');
  assert.equal(request.options.signal.aborted, true);
  request.resolve(searchBody('obsolete'));
  await nextPage;
  assert.equal(p.nodes.get('results').textContent, '');
  assert.equal(p.nodes.get('results').getAttribute('aria-busy'), 'false');
  assert.equal(p.nodes.get('loadMore').disabled, true);
  assert.equal(p.nodes.get('handoff').disabled, true);
  assert.match(p.nodes.get('searchFeedback').textContent, /criteria changed/);
});

test('search edit comparison uses the serialized request signature rather than untrimmed text', async () => {
  const p = await page();
  await search(p, 'first', 'page-two');
  p.nodes.get('query').value = '  needle  ';
  p.nodes.get('query').dispatch('input');
  p.nodes.get('query').dispatch('change');
  assert.match(p.nodes.get('results').textContent, /first/);
  assert.equal(p.nodes.get('loadMore').disabled, false);
  const nextPage = p.ui.doSearch('page-two');
  const request = p.request('/api/search?');
  assert.equal(paramsOf(request).get('q'), 'needle');
  request.resolve(searchBody('second'));
  await nextPage;
  assert.equal(p.nodes.get('results').children.length, 2);
});
