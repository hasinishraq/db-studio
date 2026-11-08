// AI, table editing and clean-up, in the real app, with a local stand-in model.
import WebSocket from 'ws';
import http from 'node:http';
import fs from 'node:fs';

const OUT = process.argv[2];
const SECRET = 'Ann-Private-Value';
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const results = [];
const check = (name, ok, extra = '') => {
  results.push({ name, ok });
  console.log(`${ok ? 'PASS' : 'FAIL'}  ${name}${extra ? `  [${extra}]` : ''}`);
};

// The stand-in model: decides from the last message, and keeps everything it was sent.
const received = [];
const chunk = (delta, finish = null) =>
  `data: ${JSON.stringify({ id: '1', object: 'chat.completion.chunk', created: 1, model: 'm', choices: [{ index: 0, delta, finish_reason: finish }] })}\n\n`;
const tool = (sql) =>
  chunk({ tool_calls: [{ index: 0, id: `c${received.length}`, type: 'function', function: { name: 'run_sql', arguments: JSON.stringify({ sql }) } }] }) +
  chunk({}, 'tool_calls');
const say = (text) => chunk({ content: text }) + chunk({}, 'stop');
const model = http.createServer((req, res) => {
  let body = '';
  req.on('data', (d) => (body += d));
  req.on('end', () => {
    received.push(body);
    const { messages } = JSON.parse(body);
    const last = messages[messages.length - 1];
    const text = typeof last.content === 'string' ? last.content : JSON.stringify(last.content);
    let reply;
    if (last.role === 'tool') reply = say(`Finished. ${text.slice(0, 40)}`);
    else if (text.includes('list everyone')) reply = tool('SELECT * FROM e2e_ai');
    else if (text.includes('remove Ben')) reply = tool("DELETE FROM e2e_ai WHERE name = 'Ben'");
    else if (text.includes('remove Cy')) reply = tool("DELETE FROM e2e_ai WHERE name = 'Cy'");
    else reply = say('Hello.');
    res.writeHead(200, { 'content-type': 'text/event-stream' });
    res.end(`${reply}data: [DONE]\n\n`);
  });
});
await new Promise((r) => model.listen(0, '127.0.0.1', r));
const endpoint = `http://127.0.0.1:${model.address().port}/v1`;

const list = await (await fetch('http://127.0.0.1:9222/json')).json();
const ws = new WebSocket(list.find((t) => t.type === 'page').webSocketDebuggerUrl, { maxPayload: 2 ** 28 });
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
    problems.push(m.params.exceptionDetails.exception?.description ?? m.params.exceptionDetails.text);
  } else if (m.method === 'Log.entryAdded' && m.params.entry.level === 'error') {
    problems.push(m.params.entry.text);
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
  if (!r || r.exceptionDetails) throw new Error(r?.exceptionDetails?.exception?.description ?? `failed: ${expression}`);
  return r.result.value;
};
await js(`
  window.t = {
    byText: (text) => [...document.querySelectorAll('button')].find(e => e.textContent.trim() === text && e.offsetParent !== null),
    click: (text) => { const e = t.byText(text); if (!e) throw new Error('no button ' + text); e.click(); return true; },
    setValue: (i, value) => {
      const proto = i.tagName === 'SELECT' ? HTMLSelectElement.prototype : i.tagName === 'TEXTAREA' ? HTMLTextAreaElement.prototype : HTMLInputElement.prototype;
      Object.getOwnPropertyDescriptor(proto, 'value').set.call(i, value);
      i.dispatchEvent(new Event(i.tagName === 'SELECT' ? 'change' : 'input', { bubbles: true }));
      return true;
    },
    fill: (name, value) => {
      const l = [...document.querySelectorAll('label')].find(l => l.textContent.trim().startsWith(name) && l.offsetParent !== null);
      const i = l?.querySelector('input,select'); if (!i) throw new Error('no field ' + name);
      return t.setValue(i, value);
    },
    text: () => document.body.innerText,
  };
`);
const shot = async (name) => {
  const { data } = await send('Page.captureScreenshot', { format: 'png' });
  fs.writeFileSync(`${OUT}/${name}.png`, Buffer.from(data, 'base64'));
};
const has = (text) => js(`t.text().includes(${JSON.stringify(text)})`);
const until = async (fn, ms = 10000) => {
  const end = Date.now() + ms;
  while (Date.now() < end) {
    if (await fn()) return true;
    await sleep(100);
  }
  return false;
};
const typeSql = async (sql) => {
  const box = await js(`(() => { const e = [...document.querySelectorAll('.monaco-editor .view-lines')].find(n => n.offsetParent); const r = e.getBoundingClientRect(); return { x: r.x + 60, y: r.y + 10 }; })()`);
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
const approveSql = async (sql) => {
  await typeSql(sql);
  await js(`t.click('Run')`);
  await until(() => has('Run on E2E Postgres'));
  await js(`[...document.querySelectorAll('label')].find(l => l.textContent.includes('I understand'))?.querySelector('input').click()`);
  await sleep(100);
  await js(`t.click('Run on E2E Postgres')`);
  await until(async () => !(await has('Run on E2E Postgres')));
  await sleep(400);
};
const ask = async (text) => {
  await js(`t.setValue(document.querySelector('[aria-label="Message"]'), ${JSON.stringify(text)})`);
  await sleep(100);
  await js(`t.click('Send')`);
};

await until(() => js(`!!document.querySelector('[aria-label="Search tables"]')`));
await sleep(800);
check('the profile survived a restart', await has('E2E Postgres'));
await js(`document.querySelector('[aria-label="Connect E2E Postgres"]').click()`);
check('the saved password came back from the OS keychain', await until(() => js(`!!document.querySelector('[aria-label="Disconnect E2E Postgres"]')`)));
await js(`t.click('New query')`);
await until(() => js(`!!document.querySelector('.monaco-editor .view-lines')`));
await sleep(800);
await approveSql(`CREATE TABLE e2e_ai (id INTEGER PRIMARY KEY, name TEXT)`);
await approveSql(`INSERT INTO e2e_ai VALUES (1, '${SECRET}'), (2, 'Ben'), (3, 'Cy')`);

// Point the assistant at the stand-in.
await js(`document.querySelector('[aria-label="Settings"]').click()`);
await until(() => has('Endpoint'));
await js(`t.fill('Provider', 'OpenAi')`);
await sleep(100);
await js(`t.fill('Endpoint', ${JSON.stringify(endpoint)}); t.fill('Model', 'stand-in');`);
await sleep(100);
// Settings save as they change.
await sleep(1200);
await until(() => has('Saved'));

await js(`document.querySelector('[aria-label="Assistant"]').click()`);
await until(() => js(`!!document.querySelector('[aria-label="Message"]')`));
await sleep(400);

await ask('list everyone');
check('a safe read runs with no review', await until(() => has(SECRET)));
check('the UI shows the rows', await has('3 rows'));
await until(() => has('Finished.'));
check('private row values never reached the model', !received.join('').includes(SECRET));
check('the model did get the shape', received.join('').includes('Rows: 3'));
check('the model was given the schema', received[0].includes('e2e_ai'));
await shot('10-ai-read');

await ask('remove Ben');
check('a write stops at a review card', await until(() => has('The assistant wants to run this')));
await shot('11-ai-review');
await js(`t.click('Decline')`);
check('declining is shown', await until(() => has('Declined. It did not run.')));
await until(() => has('Finished.'));
check('the model is told it was declined', received.join('').includes('declined'));

await ask('remove Cy');
await until(() => has('The assistant wants to run this'));
await js(`t.click('Approve')`);
check('an approved write runs', await until(() => has('1 rows affected')));
await sleep(500);

// The table tab shows the outcome of both decisions.
await js(`document.querySelector('[aria-label="Connections"]').click()`);
await sleep(200);
if (!(await js(`!!t.byText('public')`))) await js(`document.querySelector('[aria-label="Connections"]').click()`);
await sleep(200);
await js(`[...document.querySelectorAll('button')].find(b => b.textContent.trim() === 'public').click()`);
await until(() => js(`!!t.byText('e2e_ai')`));
await js(`t.click('e2e_ai')`);
check('declined row kept, approved row gone', await until(() => has('Ben')) && !(await has('Cy')));

// Edit a cell through the grid.
await until(() => js(`[...document.querySelectorAll('td')].some(c => c.textContent === 'Ben' && c.offsetParent)`));
await js(`[...document.querySelectorAll('td')].find(c => c.textContent === 'Ben' && c.offsetParent).dispatchEvent(new MouseEvent('dblclick', { bubbles: true }))`);
await until(() => has('Edit row'));
await js(`t.setValue(document.querySelector('input[aria-label="name"]'), "O'Brien")`);
await sleep(100);
await js(`t.click('Review change')`);
check('a grid edit opens the review', await until(() => has('Review this change')));
const planned = await js(`document.querySelector('[role=alertdialog] pre').textContent`);
check('Rust planned safe SQL', planned.includes("'O''Brien'") && planned.includes(`"id" = '2'`), planned);
await shot('12-grid-review');
await js(`t.click('Run on E2E Postgres')`);
check('the grid edit shows in the log under the data', await until(() => js(`[...document.querySelectorAll('[aria-label="Run log"]')].some((log) => log.offsetParent && log.innerText.includes('1 row affected'))`)));
check('the edit reaches the grid', await until(() => has("O'Brien")));
await shot('13-table');

await js(`t.click('Structure')`);
check('structure shows the key', await until(() => has('id (key)')));
await shot('14-structure');

// Audit shows who did what.
await js(`document.querySelector('[aria-label="Audit log"]').click()`);
await until(() => has('Verify integrity'));
await sleep(300);
const audit = await js(`t.text()`);
check('the AI write is audited as the assistant', /Ai\s+ok\s+1\s+DELETE FROM e2e_ai WHERE name = 'Cy'/.test(audit));
check('the grid edit is audited as a grid edit', /Grid\s+ok\s+1\s+UPDATE/.test(audit));
check('the declined write is not audited', !audit.includes("name = 'Ben'"));

const failed = results.filter((r) => !r.ok);
console.log(`\n${results.length - failed.length}/${results.length} passed`);
console.log('page problems:', problems.length ? [...new Set(problems)].slice(0, 8) : 'none');
ws.close();
model.close();
process.exit(failed.length ? 1 : 0);
