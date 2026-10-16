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
    classList: { add: x => classes.add(x), remove: x => classes.delete(x), toggle: (x, on) => on ? classes.add(x) : classes.delete(x), contains: x => classes.has(x) },
    setAttribute() {},
    querySelector(selector) {
      if (!children.has(selector)) children.set(selector, element());
      return children.get(selector);
    },
    appendChild(child) { this.appended.push(child); },
    replaceChildren(...children) { this.appended = children; },
  };
}

const nodes = new Map(['pins', 'status', 'counts', 'uptime', 'updated', 'relay-toggle', 'relay-state', 'relay-message', 'relay-duration-form', 'relay-duration', 'relay-duration-message', 'temperature', 'humidity', 'sensor-status', 'sensor-title', 'rfid-code', 'rfid-auth-dot', 'rfid-code-status', 'rfid-status', 'rfid-hex', 'rfid-ascii', 'rfid-auth-form', 'rfid-auth-id', 'rfid-auth-message', 'rfid-auth-status', 'rfid-auth-list', 'logs', 'ota-form', 'ota-file', 'ota-submit', 'ota-progress', 'ota-message'].map(id => [id, element()]));
const data = {
  uptime: 65,
  rfid: { status: 'listening', uart_errors: 0, glitches: 0, captures: 0, hex: null, ascii: null, code: null },
  logs: ['INFO - booted', 'WARN - test'],
  sensor: { model: 'DHT11', status: 'ok', temperature_c: 23.4, humidity_percent: 56.7, age_seconds: 1 },
  pins: Array.from({ length: 40 }, (_, pin) => ({
    pin, level: pin === 0 ? true : pin === 1 ? false : null,
    latch: pin === 2 || pin === 26 ? true : null, mode: pin === 2 || pin === 26 ? 'Output' : 'Input', note: '',
  })),
};
const uploads = [];
class FakeXHR {
  constructor() { this.upload = {}; uploads.push(this); }
  open(method, path) { assert.equal(method, 'POST'); assert.equal(path, '/api/ota'); }
  setRequestHeader(name,value) { (this.headers ??= {})[name] = value; }
  send(file) { this.file = file; }
}
let offline = false;
let failCommand = false;
const commands = [];
const timers = new Map();
let timerId = 0;
const context = vm.createContext({
  document: { getElementById: id => nodes.get(id), createElement: element, querySelectorAll: () => [] },
  AbortController, Date, XMLHttpRequest: FakeXHR,
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
assert.equal(nodes.get('temperature').textContent, '23.4 °C');
assert.equal(nodes.get('sensor-title').textContent, 'DHT11 · GPIO27');
assert.equal(nodes.get('humidity').textContent, '56.7 % RH');
assert.equal(nodes.get('rfid-code').textContent, '—');
assert.match(nodes.get('rfid-status').textContent, /listening.*waiting for data/);
assert.equal(nodes.get('relay-toggle').disabled, false);
assert.equal(nodes.get('relay-toggle').textContent, 'Set LOW · activate relay');
assert.ok([...timers.values()].some(timer => timer.delay === 1000));

offline = true;
await context.refresh();
assert.equal(nodes.get('status').textContent, 'Offline · retrying');
assert.ok(nodes.get('pins').classList.contains('stale'));
assert.equal(nodes.get('temperature').textContent, '—');
assert.match(nodes.get('sensor-status').textContent, /offline/);
assert.equal(nodes.get('rfid-hex').textContent, '—');
assert.match(nodes.get('rfid-status').textContent, /offline/);
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
data.sensor = { status: 'checksum error', temperature_c: null, humidity_percent: null, age_seconds: 0 };
await context.refresh();
assert.equal(nodes.get('temperature').textContent, '—');
assert.equal(nodes.get('humidity').textContent, '—');
assert.equal(nodes.get('sensor-status').textContent, 'Sensor: checksum error');
data.sensor = { model: 'DHT22', status: 'ok', temperature_c: -12.3, humidity_percent: 0, age_seconds: 0 };
await context.refresh();
assert.equal(nodes.get('temperature').textContent, '-12.3 °C');
assert.equal(nodes.get('sensor-title').textContent, 'DHT22 · GPIO27');
assert.equal(nodes.get('humidity').textContent, '0.0 % RH');
data.sensor.status = 'stale';
await context.refresh();
assert.equal(nodes.get('temperature').textContent, '—');
assert.equal(nodes.get('sensor-status').textContent, 'Sensor: stale');
const commandsBeforeRfid = commands.length;
data.rfid = { status: 'received', uart_errors: 0, glitches: 3, captures: 2, hex: '02 30 31 03', ascii: '<script>"\\', boundary: 'idle gap', age_seconds: 4, code: '0123456789ABCDEF', code_age_seconds: 4, codes: 2 };
await context.refresh();
assert.equal(nodes.get('rfid-code').textContent, '0123456789ABCDEF');
assert.match(nodes.get('rfid-code-status').textContent, /Unauthorized tag.*4s ago.*not unique tags/);
assert.equal(nodes.get('rfid-hex').textContent, '02 30 31 03');
assert.equal(nodes.get('rfid-ascii').textContent, '<script>"\\');
assert.equal(nodes.get('rfid-ascii').innerHTML, '', 'raw RFID data must never be HTML');
data.rfid.status = 'uart error';
data.rfid.uart_errors = 1;
data.rfid.age_seconds = 3600;
await context.refresh();
assert.match(nodes.get('rfid-status').textContent, /uart error.*UART errors: 1.*3600s ago/);
assert.equal(commands.length, commandsBeforeRfid, 'RFID reports must not send relay commands');
offline = true;
await context.refresh();
assert.equal(nodes.get('rfid-code').textContent, '—');
assert.equal(nodes.get('rfid-ascii').textContent, '—');
offline = false;
await context.refresh();
assert.equal(nodes.get('rfid-code').textContent, '0123456789ABCDEF');
delete data.rfid;
await context.refresh();
assert.match(nodes.get('rfid-status').textContent, /unavailable/);
console.log('Dashboard checks passed: GPIO, relay, DHT, RFID candidates/raw data/errors, offline and recovery.');

const submitOta = () => vm.runInContext('uploadFirmware({preventDefault(){}})', context);
nodes.get('ota-file').files = [{size: 336, name: 'app.bin'}];
await submitOta();
assert.equal(uploads.length, 1);
assert.equal(uploads[0].headers['X-OTA-Control'], '1');
assert.equal(nodes.get('relay-toggle').disabled, true);
assert.equal(nodes.get('ota-submit').disabled, true);
await submitOta(); assert.equal(uploads.length, 1, 'concurrent upload blocked');
uploads[0].upload.onprogress({lengthComputable:true,loaded:168,total:336});
assert.equal(nodes.get('ota-progress').value, 50);
uploads[0].status = 409; uploads[0].responseText = 'relay active'; uploads[0].onload();
assert.match(nodes.get('ota-message').textContent, /rejected.*relay active/);
assert.equal(nodes.get('ota-submit').disabled, false);
await submitOta(); uploads[1].status = 200; uploads[1].onload();
assert.match(nodes.get('ota-message').textContent, /Restarting/);
assert.equal(nodes.get('ota-progress').value, 100);
data.ota = {ready:true,slot:1}; data.uptime = 40; offline = false;
await vm.runInContext('reconnectOta(0, Date.now()+150000)', context);
assert.match(nodes.get('ota-message').textContent,/passed startup verification/);
assert.equal(nodes.get('ota-submit').disabled, false);
await submitOta(); uploads[2].onerror();
assert.match(nodes.get('ota-message').textContent,/connection lost/);
await vm.runInContext('reconnectOta(1, Date.now()+150000)', context);
assert.match(nodes.get('ota-message').textContent,/previous firmware/);
data.ota.ready = false;
await vm.runInContext('reconnectOta(0, Date.now()-1)', context);
assert.match(nodes.get('ota-message').textContent,/Could not confirm/);
nodes.get('ota-file').files = [{size:1966081}]; await submitOta();
assert.equal(uploads.length,3);
console.log('OTA dashboard checks passed: progress, rejection, concurrency, restart, reconnect, rollback and failure.');
