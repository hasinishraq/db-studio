// Inline grid editing and diagram images, in the real app with real file dialogs.
// Needs PostgreSQL on 54329 and E2E_APP_PID, the process id of the app under test.
import WebSocket from 'ws';
import { execFile } from 'node:child_process';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

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
    label: (name) => [...document.querySelectorAll('label')].find(l => l.offsetParent !== null && l.textContent.trim().startsWith(name)),
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
  const box = await js(`(() => { const r = [...document.querySelectorAll('.monaco-editor')].find((e) => e.offsetParent).getBoundingClientRect(); return { x: r.x + 160, y: r.y + 10 }; })()`);
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

// Native file dialogs are answered through the app's own window handles.
const APP = process.env.E2E_APP_PID;
const DIR = fs.mkdtempSync(path.join(os.tmpdir(), 'studio-e2e-'));
const helper = fileURLToPath(new URL('./file-dialog.ps1', import.meta.url));
const answer = (file) =>
  new Promise((resolve) => {
    execFile('powershell.exe', ['-NoProfile', '-ExecutionPolicy', 'Bypass', '-File', helper, APP, file], (e, out) => {
      if (e) console.log(`dialog helper: ${out.slice(0, 600)}`);
      resolve(!e && out.includes('answered'));
    });
  });
// Some writers save through a temporary file, so the file can vanish between two looks.
const fileAppears = (file) =>
  until(() => {
    try {
      return fs.statSync(file).size > 0;
    } catch {
      return false;
    }
  }, 10000);
const pick = (label, value) =>
  js(`(() => {
    const s = document.querySelector('select[aria-label=${JSON.stringify(label)}]');
    const visible = [...document.querySelectorAll('select[aria-label=${JSON.stringify(label)}]')].find((e) => e.offsetParent);
    Object.getOwnPropertyDescriptor(HTMLSelectElement.prototype, 'value').set.call(visible ?? s, ${JSON.stringify(value)});
    (visible ?? s).dispatchEvent(new Event('change', { bubbles: true }));
  })()`);
const mouse = async (points) => {
  for (const [type, x, y] of points) {
    await send('Input.dispatchMouseEvent', { type, x, y, button: 'left', buttons: type === 'mouseReleased' ? 0 : 1, clickCount: 1 });
    await sleep(40);
  }
};
const bold = () => js(`[...document.querySelectorAll('[aria-current="true"]:not([data-level="schema"])')].map((e) => e.textContent.trim()).join(',')`);
const boldSchema = () => js(`[...document.querySelectorAll('[aria-current="true"][data-level="schema"]')].map((e) => e.textContent.trim()).join(',')`);
const logText = () => js(`[...document.querySelectorAll('[aria-label="Run log"]')].filter((e) => e.offsetParent).map((e) => e.innerText).join(' ')`);
const review = async (name) => {
  await until(() => has(`Run on ${name}`));
  // High-risk statements stay locked until this is ticked.
  await js(`document.querySelector('[role=alertdialog] input[type=checkbox]')?.click()`);
  await sleep(100);
  await js(`t.click('Run on ${name}')`);
  await until(async () => !(await has(`Run on ${name}`)));
};
const run = async (sql, name) => {
  await typeSql(sql);
  await js(`t.click('Run')`);
  if (name) await review(name);
  await sleep(300);
  if (!(await until(() => js(`!!t.byText('Run')`), 20000))) {
    throw new Error(`still running: ${sql}
log: ${await logText()}`);
  }
};

const createDatabase = async (connection, name) => {
  await js(`[...document.querySelectorAll('button')].filter((b) => b.textContent.trim() === 'New database' && b.offsetParent).pop().click()`);
  await until(() => js(`[...document.querySelectorAll('h2')].some((h) => h.textContent === 'New database')`));
  await js(`t.fill('Name', ${JSON.stringify(name)})`);
  await js(`t.click('Create')`);
  await review(connection);
};
const addConnection = async (name, engine, port) => {
  await js(`document.querySelector('[aria-label="New connection"]').click()`);
  await until(() => has('New connection'));
  await js(`t.fill('Name', ${JSON.stringify(name)}); t.fill('Engine', ${JSON.stringify(engine)});`);
  await sleep(100);
  await js(`t.fill('Host', '127.0.0.1'); t.fill('Port', ${JSON.stringify(port)});
            t.fill('User', 'studio'); t.fill('Password', 'studio');`);
  await js(`[...document.querySelectorAll('[role=tab]')].find(e => e.textContent === 'TLS').dispatchEvent(new MouseEvent('mousedown', {bubbles:true, button:0}))`);
  await until(() => has('Mode'));
  await js(`t.fill('Mode', 'Disable')`);
  await js(`t.click('Save')`);
  await until(async () => !(await has('Test connection')));
  await js(`document.querySelector(${JSON.stringify(`[aria-label="Connect ${name}"]`)}).click()`);
  return until(() => js(`!!document.querySelector(${JSON.stringify(`[aria-label="Disconnect ${name}"]`)})`));
};


// --- Inline grid editing -------------------------------------------------------------------
const PG = 'Grid Postgres';
check('connects', await addConnection(PG, 'Postgres', '54329'));
await until(() => has('studio'));
await js(`[...document.querySelectorAll('button')].find((b) => b.textContent.trim() === 'studio' && b.offsetParent).click()`);
await until(() => has('public'));
await js(`t.click('New query')`);
await until(() => js(`!!document.querySelector('.monaco-editor textarea')`));
await sleep(800);
await run(`DROP TABLE IF EXISTS g_people; DROP TABLE IF EXISTS g_nokey`, PG);
await run(`CREATE TABLE g_people (id INTEGER PRIMARY KEY, name TEXT, note TEXT, photo BYTEA, password TEXT); CREATE TABLE g_nokey (a TEXT)`, PG);
await run(`INSERT INTO g_people VALUES (1, 'Ann', 'a', NULL, 'p1'), (2, 'Bob', 'b', NULL, 'p2'), (3, 'Cy', 'c', NULL, 'p3'); INSERT INTO g_nokey VALUES ('x')`, PG);

const select = () =>
  js(`[...document.querySelectorAll('[aria-label="Query results"]')].find((e) => e.offsetParent)?.innerText ?? ''`);
const openTable = async (name) => {
  await js(`[...document.querySelectorAll('button')].find((b) => b.textContent.trim() === ${JSON.stringify(name)} && b.offsetParent).click()`);
  await until(() => js(`!!document.querySelector('[aria-label="Table data"]') && [...document.querySelectorAll('[aria-label="Table data"]')].some((e) => e.offsetParent)`));
  await sleep(600);
};
const grid = () => `[...document.querySelectorAll('[aria-label="Table data"]')].find((e) => e.offsetParent)`;
const cell = (text) => `[...${grid()}.querySelectorAll('td')].find((e) => e.textContent.trim() === ${JSON.stringify(text)})`;
const dbl = async (text) => {
  const box = await js(`(() => { const r = ${cell(text)}.getBoundingClientRect(); return { x: r.x + 8, y: r.y + r.height / 2 }; })()`);
  for (const [type, clickCount] of [['mousePressed', 1], ['mouseReleased', 1], ['mousePressed', 2], ['mouseReleased', 2]]) {
    await send('Input.dispatchMouseEvent', { type, x: box.x, y: box.y, button: 'left', clickCount });
  }
  await sleep(150);
};
const key = async (k, code = k, vk = 0) => {
  await send('Input.dispatchKeyEvent', { type: 'rawKeyDown', key: k, code, windowsVirtualKeyCode: vk });
  await send('Input.dispatchKeyEvent', { type: 'keyUp', key: k, code, windowsVirtualKeyCode: vk });
  await sleep(120);
};
const typeIn = async (text) => {
  await send('Input.insertText', { text });
  await sleep(80);
};
const bar = () => js(`document.querySelector('[aria-label="Unsaved changes"]')?.innerText ?? ''`);
const saveAll = async () => {
  await js(`t.click('Review and save')`);
  await review(PG);
};

await openTable('g_people');
check('masked and binary columns show a lock', await js(`${grid()}.querySelectorAll('thead svg[aria-label]').length === 2`));

// Edit a cell, Tab to the next, then save.
await dbl('Ann');
check('double-click opens the cell editor', await js(`!!document.querySelector('[aria-label="Cell value"]')`));
// The editor selects the old text, so typing replaces it.
await typeIn('Anna');
await key('Tab', 'Tab', 9);
check('Tab moves on to the next cell and edits it', await js(`document.querySelector('[aria-label="Cell value"]')?.value === 'a'`));
await key('Escape', 'Escape', 27);
check('the pending bar counts the change', (await bar()).includes('1 changed'), await bar());
check('the changed cell is tinted', await js(`${cell('Anna')}.className.includes('bg-change')`));
await shot('40-pending-edit');

// Mark a delete, add a row, and fill it.
await js(`document.querySelector('[aria-label="Select row 3"]').click()`);
await js(`[...document.querySelectorAll('button')].find((b) => b.textContent.startsWith('Delete 1') && b.offsetParent).click()`);
await js(`t.click('Insert row')`);
await sleep(200);
check('the bar counts all three kinds', (await bar()).includes('1 changed · 1 new · 1 deleted'), await bar());
const firstDefault = `[...${grid()}.querySelectorAll('td')].filter((e) => e.textContent.trim() === 'default')`;
const box = await js(`(() => { const r = ${firstDefault}[0].getBoundingClientRect(); return { x: r.x + 8, y: r.y + r.height / 2 }; })()`);
for (const [type, clickCount] of [['mousePressed', 1], ['mouseReleased', 1], ['mousePressed', 2], ['mouseReleased', 2]]) {
  await send('Input.dispatchMouseEvent', { type, x: box.x, y: box.y, button: 'left', clickCount });
}
await sleep(150);
await typeIn('4');
await key('Tab', 'Tab', 9);
await typeIn('Dee');
await key('Enter', 'Enter', 13);
await shot('41-pending-all');

// Paging away asks first.
await js(`t.click('Filter')`);
check('leaving with unsaved edits asks first', await until(() => has('Discard unsaved changes?')));
await js(`t.click('Keep editing')`);
check('keeping edits leaves them pending', (await bar()).includes('1 new'));

await saveAll();
check('the save clears the bar', await until(async () => (await bar()) === ''));
check('the log reports the save', (await logText()).includes('Saved 3 changes'), await logText());
await js(`[...document.querySelectorAll('[role=tab]')].find((e) => e.textContent.includes('New query'))?.click()`);
await sleep(300);
await run(`SELECT id, name FROM g_people ORDER BY id`);
await until(() => js(`document.querySelector('[aria-label="Query results"]')?.innerText.includes('Dee') ?? false`));
{
  const text = await select();
  check('the database holds the saved rows', text.includes('Anna') && text.includes('Bob') && text.includes('Dee') && !text.includes('Cy'), text);
}

// A row changed elsewhere after loading: nothing is saved.
await openTable('g_people');
await dbl('Bob');
await typeIn('Bobby');
await key('Enter', 'Enter', 13);
await js(`[...document.querySelectorAll('[role=tab]')].find((e) => e.textContent.includes('New query'))?.click()`);
await sleep(300);
await run(`UPDATE g_people SET name = 'Robert' WHERE id = 2`, PG);
await openTable('g_people');
await saveAll();
check('a row changed since loading is refused', await until(() => has('changed or removed since it was loaded')));
check('the edit stays pending after a refusal', (await bar()).includes('1 changed'));
await shot('42-refused');
await js(`t.click('Discard')`);
check('Discard clears the edits', await until(async () => (await bar()) === ''));

// A table without a key keeps the form.
await openTable('g_nokey');
check('a table without a key explains why', await until(() => has('no primary key')));
await dbl('x');
check('it opens the row form instead', await until(() => has('Edit row')));
await js(`t.click('Cancel')`);

// Closing a tab with edits asks first.
await openTable('g_people');
await dbl('Anna');
await typeIn('Z');
await key('Enter', 'Enter', 13);
await js(`[...document.querySelectorAll('[aria-label^="Close"]')].find((b) => b.offsetParent && b.closest('[role=tab]')?.textContent.includes('g_people'))?.click()`);
check('closing a tab with edits asks first', await until(() => has('Discard and close')));
await js(`t.click('Discard and close')`);
await sleep(300);

// --- Diagram images ------------------------------------------------------------------------
await js(`[...document.querySelectorAll('button')].find((b) => b.textContent.trim() === 'Diagram' && b.offsetParent).click()`);
await until(() => js(`!!document.querySelector('[data-table="g_people"]')`));
await sleep(500);
const magic = {
  Png: (b) => b.subarray(0, 8).equals(Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a])),
  Jpeg: (b) => b[0] === 0xff && b[1] === 0xd8 && b[b.length - 2] === 0xff && b[b.length - 1] === 0xd9,
  Pdf: (b) => b.subarray(0, 5).toString() === '%PDF-',
};
for (const [format, name] of [['Png', 'diagram.png'], ['Jpeg', 'diagram.jpg'], ['Pdf', 'diagram.pdf']]) {
  const file = path.join(DIR, name);
  const answered = answer(file);
  await pick('Export image', format);
  check(`${format} save dialog answered`, await answered);
  await fileAppears(file);
  await sleep(300);
  check(`${format} file is a real ${format}`, fs.existsSync(file) && magic[format](fs.readFileSync(file)));
}
check('the log reports the image', (await logText()).includes('Saved the diagram of public as PDF'), await logText());
await shot('43-diagram-export');

// --- Default schema from the right-click menu --------------------------------------------
await js(`[...document.querySelectorAll('[role=tab]')].find((e) => e.textContent.includes('New query'))?.click()`);
await sleep(300);
await run(`CREATE SCHEMA IF NOT EXISTS g_sales`, PG);
const rightClick = async (text) => {
  const box = await js(`(() => { const b = [...document.querySelectorAll('[data-level]')].find((e) => e.textContent.trim() === ${JSON.stringify(text)} && e.offsetParent); const r = b.getBoundingClientRect(); return { x: r.x + 20, y: r.y + r.height / 2 }; })()`);
  for (const type of ['mousePressed', 'mouseReleased']) {
    await send('Input.dispatchMouseEvent', { type, x: box.x, y: box.y, button: 'right', clickCount: 1 });
  }
  await sleep(200);
};
check('the new schema is listed', await until(() => js(`[...document.querySelectorAll('[data-level="schema"]')].some((e) => e.textContent.trim() === 'g_sales')`), 10000));
await rightClick('g_sales');
check('right-click opens the menu', await until(() => js(`!!document.querySelector('[role=menu]')`)));
await shot('44-schema-menu');
await js(`[...document.querySelectorAll('[role=menuitem]')].find((e) => e.textContent === 'Use as default and remember').click()`);
check('the schema is in use', await until(async () => (await boldSchema()) === 'g_sales'), await boldSchema());
await js(`document.querySelector('[aria-label="Disconnect ${PG}"]').click()`);
await until(() => js(`!!document.querySelector('[aria-label="Connect ${PG}"]')`));
await js(`document.querySelector('[aria-label="Connect ${PG}"]').click()`);
await until(() => js(`!!document.querySelector('[aria-label="Disconnect ${PG}"]')`));
check('after reconnecting, the remembered schema is in use', await until(async () => (await boldSchema()) === 'g_sales', 10000), await boldSchema());
await js(`document.querySelector('[aria-label="Assistant"]')?.click()`);
check('the assistant names server, database and schema', await until(() => has(`Working in ${PG} › studio › g_sales`)));
await shot('45-assistant-place');
await rightClick('g_sales');
await js(`[...document.querySelectorAll('[role=menuitem]')].find((e) => e.textContent === 'Forget remembered schema').click()`);
await sleep(300);
await js(`t.click('New query')`);
await until(() => js(`!!document.querySelector('.monaco-editor textarea')`));
await sleep(800);
await run(`DROP SCHEMA g_sales`, PG);

await js(`[...document.querySelectorAll('[role=tab]')].find((e) => e.textContent.includes('New query'))?.click()`);
await sleep(300);
await run(`DROP TABLE g_people; DROP TABLE g_nokey`, PG);
await js(`document.querySelector('[aria-label="Disconnect ${PG}"]').click()`);
await until(() => js(`!!document.querySelector('[aria-label="Connect ${PG}"]')`));
await js(`document.querySelector('[aria-label="Delete ${PG}"]').click()`);
await sleep(500);
fs.rmSync(DIR, { recursive: true, force: true });

const failed = results.filter((r) => !r.ok);
console.log(`\n${results.length - failed.length}/${results.length} passed`);
console.log('page problems:', problems.length ? [...new Set(problems)].slice(0, 12) : 'none');
ws.close();
process.exit(failed.length ? 1 : 0);
