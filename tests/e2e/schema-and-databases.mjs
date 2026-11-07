// Databases, row export, the log, and schema files, in the real app with real file dialogs.
// Needs E2E_APP_PID, the process id of the app under test.
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
const fileAppears = (file) => until(() => fs.existsSync(file) && fs.statSync(file).size > 0, 10000);
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
const bold = () => js(`[...document.querySelectorAll('[aria-current="true"]')].map((e) => e.textContent.trim()).join(',')`);
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

// PostgreSQL with no database named.
const PG = 'Blank Postgres';
check('connects with only server details', await addConnection(PG, 'Postgres', '54329'));
check('lists the databases on the server', await until(() => has('studio')));
check('the database in use is bold', (await bold()) === 'postgres', await bold());
await shot('20-databases');

await js(`[...document.querySelectorAll('button')].find((b) => b.textContent.trim() === 'studio' && b.offsetParent).click()`);
check('choosing one moves the bold mark', await until(async () => (await bold()) === 'studio'), await bold());
check('its schemas appear under it', await until(() => has('public')));

await js(`t.click('New query')`);
await until(() => js(`!!document.querySelector('.monaco-editor textarea')`));
await sleep(800);
check('the title bar names it', await has(`${PG}, studio`));
await run(`SELECT current_database()`);
check('queries run in the chosen database', await until(() => js(`document.querySelector('[aria-label="Query results"]')?.innerText.includes('studio') ?? false`)));

await run(`DROP TABLE IF EXISTS d3_child; DROP TABLE IF EXISTS d3_parent; DROP TABLE IF EXISTS d3_imported_b; DROP TABLE IF EXISTS d3_imported_a`, PG);
await run(`CREATE TABLE d3_parent (id INTEGER PRIMARY KEY, name TEXT); CREATE TABLE d3_child (id SERIAL PRIMARY KEY, parent_id INTEGER REFERENCES d3_parent (id) ON DELETE CASCADE)`, PG);
await run(`INSERT INTO d3_parent VALUES (1, 'one'), (2, 'two'), (3, 'three'), (4, 'four')`, PG);

// Picking rows in query results.
await run(`SELECT id, name FROM d3_parent ORDER BY id`);
await until(() => js(`document.querySelector('[aria-label="Query results"]').innerText.includes('four')`));
const clickRow = async (index, modifiers = 0) => {
  const { x, y } = await js(`(() => {
    const r = [...document.querySelectorAll('[aria-label="Query results"] [role=row]')][${index}].getBoundingClientRect();
    return { x: r.x + 30, y: r.y + r.height / 2 };
  })()`);
  for (const type of ['mousePressed', 'mouseReleased']) {
    await send('Input.dispatchMouseEvent', { type, x, y, button: 'left', clickCount: 1, modifiers });
  }
  await sleep(100);
};
await clickRow(1);
await clickRow(3, 2);
check('rows can be selected', await until(() => has('2 selected')));
await shot('21-selected-rows');

for (const [format, name, count] of [
  ['Csv', 'picked.csv', (t) => t.trim().split('\n').length === 3 && t.includes('two') && t.includes('four') && !t.includes('one')],
  ['JsonLines', 'picked.jsonl', (t) => t.trim().split('\n').length === 2 && !t.includes('three')],
  ['SqlInserts', 'picked.sql', (t) => (t.match(/INSERT INTO/g) ?? []).length === 2 && !t.includes('one')],
]) {
  const file = path.join(DIR, name);
  const answered = answer(file);
  await pick('Export results', format);
  check(`${format} save dialog answered`, await answered);
  await fileAppears(file);
  check(`${format} holds only the selected rows`, fs.existsSync(file) && count(fs.readFileSync(file, 'utf8')));
}
{
  const file = path.join(DIR, 'picked.xlsx');
  const answered = answer(file);
  await pick('Export results', 'Xlsx');
  await answered;
  check('Excel export writes a file', await fileAppears(file));
}
check('the log says what was exported', (await logText()).includes('Exported 2 rows (selected)'));

// Nothing selected means the whole result.
await js(`t.click('Clear selection')`);
{
  const file = path.join(DIR, 'all.csv');
  const answered = answer(file);
  await pick('Export results', 'Csv');
  await answered;
  await fileAppears(file);
  check('with nothing selected, every row exports', fs.readFileSync(file, 'utf8').trim().split('\n').length === 5);
}

// Dragging the log taller and shorter.
const logHeight = () => js(`document.querySelector('[aria-label="Run log"]').getBoundingClientRect().height`);
const handle = () => js(`(() => { const r = document.querySelector('[aria-label="Resize log"]').getBoundingClientRect(); return { x: r.x + r.width / 2, y: r.y + r.height / 2 }; })()`);
const dragBy = async (dy) => {
  const { x, y } = await handle();
  await mouse([['mouseMoved', x, y], ['mousePressed', x, y], ['mouseMoved', x, y + dy / 2], ['mouseMoved', x, y + dy], ['mouseReleased', x, y + dy]]);
  await sleep(200);
};
const startHeight = await logHeight();
await dragBy(-90);
const taller = await logHeight();
check('the log drags taller', taller > startHeight + 40, `${startHeight} to ${taller}`);
await dragBy(140);
const shorter = await logHeight();
check('the log drags shorter', shorter < taller - 40, `${taller} to ${shorter}`);
await shot('22-log-resized');

// A table tab: selected rows only.
await js(`[...document.querySelectorAll('button')].find((b) => b.textContent.trim() === 'public' && b.offsetParent).click()`);
await until(() => js(`[...document.querySelectorAll('button')].some((b) => b.textContent.trim() === 'd3_parent' && b.offsetParent)`));
await js(`[...document.querySelectorAll('button')].find((b) => b.textContent.trim() === 'd3_parent' && b.offsetParent).click()`);
await until(() => js(`!!document.querySelector('[aria-label="Select row 2"]')`));
await js(`document.querySelector('[aria-label="Select row 2"]').click()`);
await sleep(200);
{
  const file = path.join(DIR, 'table.jsonl');
  const answered = answer(file);
  await pick('Export table', 'JsonLines');
  await answered;
  await fileAppears(file);
  const text = fs.readFileSync(file, 'utf8').trim();
  check('a table tab exports only the ticked row', text.split('\n').length === 1 && text.includes('two'), text);
}

// The diagram.
await js(`[...document.querySelectorAll('button')].find((b) => b.textContent.trim() === 'Diagram' && b.offsetParent).click()`);
const boxes = () => js(`[...document.querySelectorAll('[data-table]')].filter((e) => e.getBoundingClientRect().width > 0).map((e) => e.dataset.table)`);
check('the diagram draws the tables', await until(async () => (await boxes()).includes('d3_child')));
check('it draws the relationship', await js(`[...document.querySelectorAll('svg path[stroke="var(--muted)"]')].some((e) => e.getBoundingClientRect().width > 0)`));
const zoomText = () => js(`[...document.querySelectorAll('output')].find((e) => e.offsetParent).textContent`);
const zoomBefore = await zoomText();
await js(`document.querySelector('[aria-label="Zoom in"]').click()`);
await sleep(200);
check('zoom buttons work', (await zoomText()) !== zoomBefore, `${zoomBefore} to ${await zoomText()}`);
await js(`t.click('Fit')`);
await sleep(200);
const boxAt = () => js(`(() => { const r = document.querySelector('[data-table="d3_parent"]').getBoundingClientRect(); return { x: r.x + 40, y: r.y + 12 }; })()`);
const before = await boxAt();
await mouse([['mouseMoved', before.x, before.y], ['mousePressed', before.x, before.y], ['mouseMoved', before.x + 30, before.y + 30], ['mouseMoved', before.x + 60, before.y + 50], ['mouseReleased', before.x + 60, before.y + 50]]);
await sleep(200);
const moved = await boxAt();
check('a table can be dragged', Math.abs(moved.x - before.x) > 30, `${before.x} to ${moved.x}`);
await js(`t.click('Fit')`);
await sleep(200);
await shot('23-diagram');

// Export the schema.
const exported = path.join(DIR, 'public.sql');
{
  const answered = answer(exported);
  await js(`[...document.querySelectorAll('button')].find((b) => b.textContent.includes('Export schema') && b.offsetParent).click()`);
  await answered;
  await fileAppears(exported);
  const text = fs.readFileSync(exported, 'utf8');
  check('schema export writes the tables', text.includes('d3_child') && text.includes('d3_parent'));
  check('schema export keeps foreign keys and their rules', /FOREIGN KEY[^;]*ON DELETE CASCADE/.test(text));
  check('schema export holds no rows', !text.includes('three'));
}

// Import: preview first, then apply.
const schemaFile = path.join(DIR, 'import.sql');
fs.writeFileSync(
  schemaFile,
  `CREATE TABLE d3_imported_a (id INTEGER PRIMARY KEY, label TEXT);
   CREATE TABLE d3_imported_b (id INTEGER PRIMARY KEY, a_id INTEGER REFERENCES d3_imported_a (id));`,
);
{
  const answered = answer(schemaFile);
  await js(`[...document.querySelectorAll('button')].find((b) => b.textContent.includes('Import schema') && b.offsetParent).click()`);
  await answered;
  check('the preview says nothing was applied', await until(() => has('Nothing has been applied')));
  check('the file is drawn before anything runs', await until(async () => (await boxes()).includes('d3_imported_b')));
  await shot('24-import-preview');
}
await js(`t.click('Close preview')`);
check('closing the preview shows the live schema again', await until(async () => !(await boxes()).includes('d3_imported_b')));
check('so the preview changed nothing on the server', (await boxes()).includes('d3_parent'));

{
  const answered = answer(schemaFile);
  await js(`[...document.querySelectorAll('button')].find((b) => b.textContent.includes('Import schema') && b.offsetParent).click()`);
  await answered;
  await until(() => has('Nothing has been applied'));
}
await js(`t.click('Apply to studio')`);
check('applying goes through the review', await until(() => has(`Run on ${PG}`)));
check('the review warns about partial failure', await has('statements before it stay applied'));
await js(`t.click("Don't run")`);
check('declining applies nothing', await until(async () => (await logText()).includes('not applied')));
await js(`t.click('Apply to studio')`);
await review(PG);
check('the log counts what ran', await until(async () => (await logText()).includes('2 of 2 statements ran')));
check('the diagram now shows the live tables', await until(async () => (await boxes()).includes('d3_imported_b') && !(await has('Nothing has been applied'))));

// Change database with a table tab open: those tabs belong to the old database.
await js(`[...document.querySelectorAll('button')].find((b) => b.textContent.trim() === 'postgres' && b.offsetParent).click()`);
check('switching back works', await until(async () => (await bold()) === 'postgres'));
const openTabs = () => js(`[...document.querySelectorAll('[role=tab]')].map((e) => e.textContent.trim())`);
check('tabs from the old database close', await until(async () => {
  const names = await openTabs();
  return !names.includes('d3_parent') && !names.includes('public diagram') && names.includes('New query');
}), String(await openTabs()));

// Clean up through the same app.
await js(`[...document.querySelectorAll('button')].find((b) => b.textContent.trim() === 'studio' && b.offsetParent).click()`);
await until(async () => (await bold()) === 'studio');
await js(`[...document.querySelectorAll('[role=tab]')].find((e) => e.textContent === 'New query').click()`);
await sleep(300);
await run(`DROP TABLE d3_imported_b; DROP TABLE d3_imported_a; DROP TABLE d3_child; DROP TABLE d3_parent`, PG);
await js(`document.querySelector('[aria-label="Disconnect ${PG}"]').click()`);
await until(() => js(`!!document.querySelector('[aria-label="Connect ${PG}"]')`));

// MySQL with no database named.
const MY = 'Blank MySQL';
check('MySQL connects with only server details', await addConnection(MY, 'MySql', '33069'));
check('MySQL lists its databases', await until(() => has('studio')));
check('nothing is bold until one is chosen', (await bold()) === '', await bold());
await js(`[...document.querySelectorAll('button')].find((b) => b.textContent.trim() === 'studio' && b.offsetParent).click()`);
check('choosing one makes it bold', await until(async () => (await bold()) === 'studio'));
await shot('25-mysql-chosen');
await js(`[...document.querySelectorAll('button')].filter((b) => b.textContent.trim() === 'New query' && b.offsetParent).pop().click()`);
await until(() => js(`[...document.querySelectorAll('select[aria-label="Connection"]')].some((e) => e.offsetParent)`));
await sleep(900);
await pick('Connection', await js(`[...[...document.querySelectorAll('select[aria-label="Connection"]')].find((e) => e.offsetParent).options].find((o) => o.textContent === ${JSON.stringify(MY)}).value`));
await sleep(200);
await run(`SELECT DATABASE() AS chosen`);
check('MySQL queries run in the chosen database', await until(() => js(`[...document.querySelectorAll('[aria-label="Query results"]')].some((e) => e.offsetParent && e.innerText.includes('studio'))`)));

await js(`document.querySelector('[aria-label="Disconnect ${MY}"]').click()`);
await until(() => js(`!!document.querySelector('[aria-label="Connect ${MY}"]')`));
await js(`document.querySelector('[aria-label="Delete ${MY}"]').click()`);
await js(`document.querySelector('[aria-label="Delete ${PG}"]').click()`);
await sleep(500);
fs.rmSync(DIR, { recursive: true, force: true });

const failed = results.filter((r) => !r.ok);
console.log(`\n${results.length - failed.length}/${results.length} passed`);
console.log('page problems:', problems.length ? [...new Set(problems)].slice(0, 12) : 'none');
ws.close();
process.exit(failed.length ? 1 : 0);
