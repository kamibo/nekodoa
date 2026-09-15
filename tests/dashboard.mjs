// Run with: node tests/dashboard.mjs
// Exercise the embedded page's actual script with a minimal DOM and fetch stub.
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import vm from 'node:vm';

function element() {
  const children = new Map();
  const classes = new Set();
  return {
    textContent: '', className: '', innerHTML: '', appended: [],
    disabled: false,
    addEventListener() {},
    classList: { add: x => classes.add(x), remove: x => classes.delete(x), contains: x => classes.has(x) },
    querySelector(selector) {
      if (!children.has(selector)) children.set(selector, element());
      return children.get(selector);
    },
    appendChild(child) { this.appended.push(child); },
  };
}

const nodes = new Map(['pins', 'status', 'counts', 'uptime', 'updated', 'relay-toggle', 'relay-state', 'relay-message'].map(id => [id, element()]));
const data = {
  uptime: 65,
  pins: Array.from({ length: 40 }, (_, pin) => ({
    pin, level: pin === 0 ? true : pin === 1 ? false : null,
    latch: pin === 2 || pin === 26 ? true : null, mode: pin === 2 || pin === 26 ? 'Output' : 'Input', note: '',
  })),
};
let offline = false;
let failCommand = false;
const commands = [];
const timers = new Map();
let timerId = 0;
const context = vm.createContext({
  document: { getElementById: id => nodes.get(id), createElement: element },
  AbortController, Date,
  setTimeout(callback, delay) { timers.set(++timerId, { callback, delay }); return timerId; },
  clearTimeout(id) { timers.delete(id); },
  async fetch(path, options) {
    assert.equal(options.cache, 'no-store');
    if (options.method === 'POST') {
      assert.equal(options.headers['X-Relay-Control'], '1');
      assert.ok(['/api/relay/high', '/api/relay/low'].includes(path));
      commands.push(path);
      if (failCommand) throw new Error('response lost');
      data.pins[26].latch = path.endsWith('/high');
    } else {
      assert.equal(path, '/api/gpio');
    }
    if (offline) throw new Error('network unavailable');
    return { ok: true, json: async () => data };
  },
});
const page = readFileSync(new URL('../src/index.html', import.meta.url), 'utf8');
vm.runInContext(page.match(/<script>([\s\S]*?)<\/script>/)[1], context);
await new Promise(resolve => setImmediate(resolve));
const cards = nodes.get('pins').appended;
assert.equal(cards.length, 40);
assert.equal(cards[0].querySelector('.level').textContent, 'HIGH');
assert.equal(cards[1].querySelector('.level').textContent, 'LOW');
assert.equal(cards[2].querySelector('.mode').textContent, 'Output · latch');
assert.equal(cards[3].querySelector('.level').textContent, 'Unavailable');
assert.equal(nodes.get('counts').textContent, '1 high · 1 low · 38 not measured');
assert.equal(nodes.get('uptime').textContent, 'Uptime 1m 5s');
assert.equal(nodes.get('status').textContent, 'Live');
assert.equal(nodes.get('relay-toggle').disabled, false);
assert.equal(nodes.get('relay-toggle').textContent, 'Set LOW · activate relay');
assert.ok([...timers.values()].some(timer => timer.delay === 1000));

offline = true;
await context.refresh();
assert.equal(nodes.get('status').textContent, 'Offline · retrying');
assert.ok(nodes.get('pins').classList.contains('stale'));
assert.equal(nodes.get('relay-toggle').disabled, true);
await context.changeRelay();
assert.equal(commands.length, 0, 'offline state must not permit commands');

offline = false;
data.pins[0].level = false;
await context.refresh();
assert.equal(cards.length, 40, 'refresh must update cards without duplicating them');
assert.equal(cards[0].querySelector('.level').textContent, 'LOW');
assert.equal(nodes.get('status').textContent, 'Live');
assert.ok(!nodes.get('pins').classList.contains('stale'));

const firstClick = context.changeRelay();
assert.equal(nodes.get('relay-toggle').disabled, true);
await context.changeRelay();
await firstClick;
assert.deepEqual(commands, ['/api/relay/low'], 'double click sends only one explicit command');
assert.equal(nodes.get('relay-state').textContent, 'GPIO26 output: LOW');
assert.equal(nodes.get('relay-toggle').textContent, 'Set HIGH · release relay');
await context.changeRelay();
assert.deepEqual(commands, ['/api/relay/low', '/api/relay/high']);
assert.equal(nodes.get('relay-state').textContent, 'GPIO26 output: HIGH');

failCommand = true;
await context.changeRelay();
assert.equal(nodes.get('relay-toggle').disabled, true);
assert.equal(nodes.get('relay-state').textContent, 'GPIO26 state unknown');
assert.match(nodes.get('relay-message').textContent, /will not be retried/);
const commandCount = commands.length;
await context.refresh();
assert.equal(commands.length, commandCount, 'polling must not resend a failed command');
assert.equal(nodes.get('relay-state').textContent, 'GPIO26 output: HIGH');
assert.equal(nodes.get('relay-toggle').disabled, false);
console.log('Dashboard checks passed: GPIO rendering, relay HIGH/LOW, repeated clicks, offline and command failure recovery.');
