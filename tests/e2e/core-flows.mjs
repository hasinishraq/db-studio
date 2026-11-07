// Drives the real app over the WebView2 debug port against the local test PostgreSQL.
import WebSocket from 'ws';
import fs from 'node:fs';

const OUT = process.argv[2];
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const results = [];
const check = (name, ok, extra = '') => {
  results.push({ name, ok });
  console.log(`${ok ? 'PASS' : 'FAIL'}  ${name}${extra ? `  [${extra}]` : ''}`);
};

async function target() {
  for (let i = 0; i < 60; i++) {
    try {
      const list = await (await fetch('http://127.0.0.1:9222/json')).json();
      const page = list.find((t) => t.type === 'page');
      if (page) return page;
    } catch {}
    await sleep(500);
  }
  throw new Error('no debug target');
}

const page = await target();
const ws = new WebSocket(page.webSocketDebuggerUrl, { maxPayload: 256 * 1024 * 1024 });
await new Promise((r) => ws.once('open', r));
let id = 0;
const pending = new Map();
const problems = [];
ws.on('message', (raw) => {
  const m = JSON.parse(raw);
  if (m.id) {
    pending.get(m.id)?.(m);
    pending.delete(m.id);
  } else if (m.method === 'Runtime.exceptionThrown') {
    problems.push(`exception: ${m.params.exceptionDetails.exception?.description ?? m.params.exceptionDetails.text}`);
  } else if (m.method === 'Runtime.consoleAPICalled' && m.params.type === 'error') {
    problems.push(`console: ${m.params.args.map((a) => a.value ?? a.description).join(' ')}`);
  } else if (m.method === 'Log.entryAdded' && ['error', 'warning'].includes(m.params.entry.level)) {
    problems.push(`log: ${m.params.entry.text}`);
  }
});
const send = (method, params = {}) =>
  new Promise((resolve) => {
    const n = ++id;
    pending.set(n, (m) => resolve(m.result));
    ws.send(JSON.stringify({ id: n, method, params }));
  });
await send('Runtime.enable');
await send('Log.enable');
await send('Page.enable');

const js = async (expression) => {
  const r = await send('Runtime.evaluate', { expression, awaitPromise: true, returnByValue: true });
  if (r.exceptionDetails) throw new Error(r.exceptionDetails.exception?.description ?? r.exceptionDetails.text);
  return r.result.value;
};

await js(`
  window.t = {
    byText: (text, tag='button') => [...document.querySelectorAll(tag)].find(e => e.textContent.trim() === text && e.offsetParent !== null),
    click: (text) => { const e = t.byText(text); if (!e) throw new Error('no button ' + text); e.click(); return true; },
    label: (name) => [...document.querySelectorAll('label')].find(l => l.textContent.trim().startsWith(name)),
    fill: (name, value) => {
      const l = t.label(name); const i = l?.querySelector('input,select');
      if (!i) throw new Error('no field ' + name);
      const proto = i.tagName === 'SELECT' ? HTMLSelectElement.prototype : HTMLInputElement.prototype;
      Object.getOwnPropertyDescriptor(proto, 'value').set.call(i, value);
      i.dispatchEvent(new Event(i.tagName === 'SELECT' ? 'change' : 'input', { bubbles: true }));
      return true;
    },
    text: () => document.body.innerText,
  };
`);

const shot = async (name) => {
  const { data } = await send('Page.captureScreenshot', { format: 'png' });
  fs.writeFileSync(`${OUT}/${name}.png`, Buffer.from(data, 'base64'));
};
const has = (text) => js(`t.text().includes(${JSON.stringify(text)})`);
const until = async (fn, ms = 8000) => {
  const end = Date.now() + ms;
  while (Date.now() < end) {
    if (await fn()) return true;
    await sleep(100);
  }
  return false;
};
const typeSql = async (sql) => {
  const box = await js(`(() => { const r = document.querySelector('.monaco-editor .view-lines').getBoundingClientRect(); return { x: r.x + 60, y: r.y + 10 }; })()`);
  for (const type of ['mousePressed', 'mouseReleased']) {
    await send('Input.dispatchMouseEvent', { type, x: box.x, y: box.y, button: 'left', clickCount: 1 });
  }
  await sleep(100);
  await send('Input.dispatchKeyEvent', { type: 'rawKeyDown', key: 'a', code: 'KeyA', modifiers: 2, windowsVirtualKeyCode: 65, commands: ['selectAll'] });
  await send('Input.dispatchKeyEvent', { type: 'keyUp', key: 'a', code: 'KeyA', modifiers: 2, windowsVirtualKeyCode: 65 });
  await sleep(50);
  await send('Input.insertText', { text: sql });
  await sleep(200);
};

await sleep(1500);
if (process.env.E2E_THEME) await js(`document.documentElement.dataset.theme = ${JSON.stringify(process.env.E2E_THEME)}`);
await shot('01-empty');
check('window renders', await until(() => js(`!!document.querySelector('[aria-label="Search tables"]')`)));
check('no problems on load', problems.length === 0, problems.slice(0, 3).join(' | '));

// Connection dialog.
if (await has('E2E Postgres')) {
  await js(`document.querySelector('[aria-label="Delete E2E Postgres"]').click()`);
  await until(() => has('Add a connection'));
}
await js(`t.click('Add a connection')`);
await until(() => has('New connection'));
await js(`t.fill('Name', 'E2E Postgres'); t.fill('Host', '127.0.0.1'); t.fill('Port', '54329');
          t.fill('Database', 'studio'); t.fill('User', 'studio'); t.fill('Password', 'studio');`);
await js(`[...document.querySelectorAll('[role=tab]')].find(e => e.textContent === 'TLS').dispatchEvent(new MouseEvent('mousedown', {bubbles:true, button:0}))`);
await until(() => has('Mode'));
await js(`t.fill('Mode', 'Disable')`);
await shot('02-connection-tls');
await js(`t.click('Test connection')`);
check('test connection reports the server', await until(() => has('PostgreSQL 17')));
await js(`t.click('Save')`);
await until(async () => !(await has('Test connection')));
check('profile saved and listed', await has('E2E Postgres'));

// Connect, then a query through the real editor.
await js(`document.querySelector('[aria-label="Connect E2E Postgres"]').click()`);
check('connects', await until(() => js(`!!document.querySelector('[aria-label="Disconnect E2E Postgres"]')`)));
await js(`t.click('New query')`);
await until(() => js(`!!document.querySelector('.monaco-editor textarea')`));
await sleep(800);
await shot('03-editor');
check('Monaco loaded under the CSP', await js(`!!document.querySelector('.monaco-editor')`));

await typeSql(`SELECT n, 'row ' || n AS label, now()::date AS day FROM generate_series(1, 3) n`);
await js(`t.click('Run')`);
check('read runs with no dialog', await until(() => has('3 rows')));
check('no dialog for a read', !(await has('Review this change')));
await sleep(300);
await shot('04-results');

// Streaming a million rows.
await typeSql(`SELECT n, md5(n::text) FROM generate_series(1, 1000000) n`);
const startedAt = Date.now();
await js(`t.click('Run')`);
const mid = await until(() => has('Fetching'), 15000);
check('streaming shows progress', mid);
await js(`window.__lag = 0; window.__last = performance.now(); window.__tick = setInterval(() => { const n = performance.now(); window.__lag = Math.max(window.__lag, n - window.__last - 50); window.__last = n; }, 50);`);
await shot('05-streaming');
const finished = await until(async () => /1,000,000 rows/.test(await js(`t.text()`)), 120000);
const lag = await js(`(clearInterval(window.__tick), Math.round(window.__lag))`);
check('1,000,000 rows arrive', finished, `${((Date.now() - startedAt) / 1000).toFixed(1)}s, worst UI stall ${lag}ms`);
check('UI stays responsive while streaming', lag < 1000, `${lag}ms`);
const domRows = await js(`document.querySelectorAll('[aria-label="Query results"] .flex.border-b').length`);
check('only visible rows are in the DOM', domRows < 200, `${domRows} rows`);
await shot('06-million');

// Stop a long stream.
await typeSql(`SELECT n FROM generate_series(1, 100000000) n`);
await js(`t.click('Run')`);
await until(() => has('Fetching'), 20000);
await sleep(800);
await js(`t.click('Stop')`);
check('Stop ends the run', await until(async () => !(await has('Fetching')), 10000));
await sleep(500);

// The review gate.
await typeSql(`CREATE TABLE e2e_people (id INTEGER PRIMARY KEY, name TEXT)`);
await js(`t.click('Run')`);
check('DDL opens the review', await until(() => has('Review this change')));
await shot('07-review');
await js(`t.click('Run on E2E Postgres')`);
await until(async () => !(await has('Review this change')));
await sleep(500);

await typeSql(`INSERT INTO e2e_people VALUES (1, 'Ann'), (2, 'Ben')`);
await js(`t.click('Run')`);
check('INSERT opens the review', await until(() => has('Review this change')));
await js(`t.click('Run on E2E Postgres')`);
check('approved INSERT reports rows', await until(() => has('2 rows affected')));

await typeSql(`DROP TABLE e2e_people`);
await js(`t.click('Run')`);
check('DROP opens the danger review', await until(() => has('This cannot be undone')));
const blocked = await js(`t.byText('Run on E2E Postgres').disabled`);
check('danger review needs the checkbox', blocked === true);
await shot('08-danger');
await js(`t.click("Don't run")`);
check('declining logs it', await until(() => has('Statement not run: confirmation declined')));

await typeSql(`SELECT COUNT(*) AS people FROM e2e_people`);
await js(`t.click('Run')`);
await sleep(800);
check('the declined DROP did not run', await until(() => has('1 rows')));
check('table still has its rows', await js(`[...document.querySelectorAll('[aria-label="Query results"] div')].some(e => e.textContent === '2')`));

// The grid stays after a write, with the log under it.
await typeSql(`SELECT id, name FROM e2e_people ORDER BY id`);
await js(`t.click('Run')`);
await until(() => has('Ann'));
await typeSql(`UPDATE e2e_people SET name = 'Anna' WHERE id = 1`);
await js(`t.click('Run')`);
await until(() => has('Review this change'));
await js(`t.click('Run on E2E Postgres')`);
check('an UPDATE is logged', await until(() => has('1 row affected')));
const grid = () => js(`document.querySelector('[aria-label="Query results"]').innerText`);
check('the table data stays on screen', (await grid()).includes('Ben'));
check('the rows on screen are the ones from before', (await grid()).includes('Ann') && !(await grid()).includes('Anna'));
check('it says the rows are from before the change', await has('loaded before your last change'));
const order = await js(`(() => {
  const log = document.querySelector('[aria-label="Run log"]').getBoundingClientRect();
  const grid = document.querySelector('[aria-label="Query results"]').getBoundingClientRect();
  return log.top >= grid.bottom - 1;
})()`);
check('the log sits below the table data', order);
await shot('15-log-under-table');
await js(`t.click('Refresh')`);
check('Refresh shows the new value', await until(async () => (await grid()).includes('Anna')));
check('the stale note goes away', await until(async () => !(await has('loaded before your last change'))));

// Refresh repeats the read only, even when a write ran in the same go.
await typeSql(`UPDATE e2e_people SET name = 'Cara' WHERE id = 2; SELECT id, name FROM e2e_people ORDER BY id`);
await js(`t.click('Run')`);
await until(() => has('Review this change'));
await js(`t.click('Run on E2E Postgres')`);
await until(async () => (await grid()).includes('Cara'));
await typeSql(`UPDATE e2e_people SET name = 'Dana' WHERE id = 2`);
await js(`t.click('Run')`);
await until(() => has('Review this change'));
await js(`t.click('Run on E2E Postgres')`);
await until(() => has('loaded before your last change'));
await js(`t.click('Refresh')`);
check('Refresh after a mixed run shows the latest value', await until(async () => (await grid()).includes('Dana')));
check('and it did not repeat the write', !(await has('Review this change')) && !(await grid()).includes('Cara'));

// Zoom shortcuts, with the cursor in the editor.
const press = async (key, code, vk, shift = false) => {
  const modifiers = 2 | (shift ? 8 : 0);
  await send('Input.dispatchKeyEvent', { type: 'rawKeyDown', key, code, modifiers, windowsVirtualKeyCode: vk });
  await send('Input.dispatchKeyEvent', { type: 'keyUp', key, code, modifiers, windowsVirtualKeyCode: vk });
  await sleep(350);
};
await typeSql(`SELECT 1`);
const width = () => js(`window.innerWidth`);
const base = await width();
await press('=', 'Equal', 187);
const bigger = await width();
check('Ctrl = makes everything bigger', bigger < base, `${base} to ${bigger}`);
await press('+', 'Equal', 187, true);
check('Ctrl + works too', (await width()) < bigger);
await press('-', 'Minus', 189);
await press('_', 'Minus', 189, true);
check('Ctrl - and Ctrl _ make it smaller', Math.abs((await width()) - base) <= 2, String(await width()));
await press('-', 'Minus', 189);
check('it goes below normal', (await width()) > base);
await press('0', 'Digit0', 48);
check('Ctrl 0 resets', Math.abs((await width()) - base) <= 2);

// A failing statement.
await typeSql(`SELECT * FROM no_such_table_here`);
await js(`t.click('Run')`);
check('database errors are shown', await until(() => has('does not exist')));

// Audit log.
await js(`document.querySelector('[aria-label="Audit log"]').click()`);
check('audit log lists the writes', await until(() => has('INSERT INTO e2e_people')));
check('declined DROP is not in the audit log', !(await has('DROP TABLE e2e_people')));
await js(`t.click('Verify integrity')`);
check('audit chain verifies', await until(() => has('The log is intact.')));
await shot('09-audit');

// Clean up the test table through the real flow.
await js(`[...document.querySelectorAll('[role=tab]')].find(e => e.textContent === 'New query').click()`);
await sleep(300);
await typeSql(`DROP TABLE e2e_people`);
await js(`t.click('Run')`);
await until(() => has('This cannot be undone'));
await js(`t.label('I understand').querySelector('input').click()`);
await sleep(100);
await js(`t.click('Run on E2E Postgres')`);
await sleep(800);

const failed = results.filter((r) => !r.ok);
console.log(`\n${results.length - failed.length}/${results.length} passed`);
console.log('page problems:', problems.length ? [...new Set(problems)].slice(0, 12) : 'none');
ws.close();
process.exit(failed.length ? 1 : 0);
