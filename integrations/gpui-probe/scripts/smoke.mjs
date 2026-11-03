// Window smoke test: start the demo app, drive the probe over HTTP, and check
// what a real window answers. Build the demo first:
//   cargo build --example demo --no-default-features --features snapshot
// GPUI_PROBE_CAPTURE=1|0 turns the capture check on or off (unset: Windows only).
// IMPECCABLE_BIN (optional) also runs the engine's `gpui snapshot` and `sweep`.
import { spawn, execFileSync } from 'node:child_process';
import { existsSync, readFileSync, rmSync } from 'node:fs';
import { join, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';
import assert from 'node:assert/strict';

const root = join(dirname(fileURLToPath(import.meta.url)), '..');
const exe = join(root, 'target', 'debug', 'examples', process.platform === 'win32' ? 'demo.exe' : 'demo');
const discovery = join(root, '.impeccable', 'gpui-probe.json');
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

assert.ok(existsSync(exe), `build the demo first: ${exe}`);
rmSync(discovery, { force: true });
const app = spawn(exe, [], { cwd: root, stdio: 'ignore' });
let port;
try {
  for (let i = 0; i < 120 && !port; i++) {
    await sleep(250);
    if (existsSync(discovery)) port = JSON.parse(readFileSync(discovery, 'utf8')).port;
  }
  assert.ok(port, 'the probe never wrote .impeccable/gpui-probe.json');
  const url = (p) => `http://127.0.0.1:${port}${p}`;
  const get = async (p) => (await fetch(url(p))).json();
  const post = async (p, body) => (await fetch(url(p), { method: 'POST', body: JSON.stringify(body) })).json();
  const step = (name) => console.log(`ok ${name}`);

  const status = await get('/status');
  assert.equal(status.windows.length, 1);
  assert.ok(status.appearanceHook);
  step('status');

  const snap = await get('/snapshot');
  const texts = snap.nodes.filter((n) => n.kind === 'text').map((n) => n.text);
  assert.ok(texts.includes('Builds') && texts.includes('Retry nightly'), JSON.stringify(texts));
  assert.ok(snap.nodes.some((n) => n.kind === 'element' && n.input.clickable && n.states.hover));
  step(`snapshot (${snap.nodes.length} nodes)`);

  const slots = await get('/slots');
  assert.deepEqual(slots.slots.map((s) => s.id), ['build-item']);
  await post('/slot', { id: 'build-item', index: 1 });
  const row = await get('/snapshot');
  assert.ok(row.nodes.length !== snap.nodes.length || JSON.stringify(row) !== JSON.stringify(snap), 'arm 1 changed nothing');
  await post('/slot', { id: 'build-item', index: 0 });
  step('slots');

  const sweep = await post('/sweep', { hover: true, focus: true });
  assert.ok(sweep.hover.length >= 2, `hovered ${sweep.hover.length}`);
  assert.ok(sweep.focus.length >= 1, 'focus_next reached nothing');
  step(`sweep (${sweep.hover.length} hovered, ${sweep.focus.length} focus stops)`);

  const pick = await post('/pick', { x: 40, y: 40 });
  assert.match(pick.source ?? '', /demo\.rs:\d+:\d+/);
  await post('/inspector', { open: false });
  step('pick');

  const capture = process.env.GPUI_PROBE_CAPTURE ?? (process.platform === 'win32' ? '1' : '0');
  assert.ok(capture === '1' || capture === '0', `GPUI_PROBE_CAPTURE must be 1 or 0, got ${capture}`);
  if (capture === '1') {
    const png = Buffer.from(await (await fetch(url('/capture'))).arrayBuffer());
    assert.equal(png.subarray(1, 4).toString(), 'PNG');
    step(`capture (${png.length} bytes)`);
  }

  const refused = await fetch(url('/status'), { headers: { Origin: 'https://example.com' } });
  assert.equal(refused.status, 403);
  step('browser origin refused');

  if (process.env.IMPECCABLE_BIN) {
    const run = (...args) => JSON.parse(execFileSync(process.env.IMPECCABLE_BIN, ['gpui', ...args, '--port', String(port)], { encoding: 'utf8' }));
    const ids = (r) => r.findings.map((f) => `${f.antipattern}@${f.line}`);
    const found = ids(run('snapshot'));
    for (const id of ['side-tab', 'tiny-text', 'low-contrast', 'text-overflow', 'gpui/unreachable-control', 'gpui/unnamed-icon-button']) {
      assert.ok(found.some((f) => f.startsWith(`${id}@`)), `snapshot missed ${id}: ${found}`);
    }
    step('engine snapshot rules');
    const swept = run('sweep');
    assert.ok(swept.findings.some((f) => f.snippet.includes('(while hovering')), 'sweep found no hover-only issue');
    assert.ok(swept.findings.some((f) => f.snippet.startsWith('taking focus changes nothing')), 'sweep did not check focus');
    assert.equal(swept.focusOrder.length, 1);
    step('engine sweep');
  }
} finally {
  app.kill();
}
console.log('smoke passed');
