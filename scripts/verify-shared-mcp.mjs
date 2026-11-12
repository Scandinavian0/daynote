// Real HTTP clients against the installed desktop app. No model API calls,
// subprocesses, note contents or credentials in the output. Read-only by default.
import assert from 'node:assert/strict';
import http from 'node:http';
import { readFileSync } from 'node:fs';

const [endpoint, tokenFile, expectedVersion, mode] = process.argv.slice(2);
if (!endpoint || !tokenFile || !expectedVersion || (mode && mode !== '--write-fixture')) {
  throw new Error('Usage: node scripts/verify-shared-mcp.mjs <loopback /mcp URL> <token-file> <version> [--write-fixture]');
}
const url = new URL(endpoint);
assert.equal(url.protocol, 'http:');
assert.equal(url.hostname, '127.0.0.1');
assert.equal(url.pathname, '/mcp');
assert.ok(!url.search && !url.hash && !url.username && !url.password);
const token = readFileSync(tokenFile, 'utf8').trim();
assert.match(token, /^[0-9a-f]{64}$/i, 'Invalid token file');
const writeFixture = mode === '--write-fixture';
if (writeFixture && (process.env.GITHUB_ACTIONS !== 'true' || process.env.RUNNER_ENVIRONMENT !== 'github-hosted')) {
  throw new Error('Writes are allowed only in the disposable GitHub-hosted installer test.');
}

function send(message, source = 'mcp-verification', overrides = {}, method = 'POST') {
  const body = message === undefined ? undefined : JSON.stringify(message);
  return new Promise((resolve, reject) => {
    const req = http.request(url, {
      method,
      agent: false,
      headers: {
        Authorization: `Bearer ${token}`,
        Accept: 'application/json, text/event-stream',
        'Content-Type': 'application/json',
        'MCP-Protocol-Version': '2025-11-25',
        'X-DayNote-Source': source,
        ...(body ? { 'Content-Length': Buffer.byteLength(body) } : {}),
        ...overrides,
      },
    }, res => {
      const chunks = [];
      let size = 0;
      res.on('data', chunk => {
        size += chunk.length;
        if (size > 4 * 1024 * 1024) res.destroy(new Error('Unexpectedly large response'));
        else chunks.push(chunk);
      });
      res.on('error', reject);
      res.on('end', () => resolve({ status: res.statusCode, headers: res.headers, body: Buffer.concat(chunks).toString('utf8') }));
    });
    req.setTimeout(10_000, () => req.destroy(new Error('MCP request timed out')));
    req.on('error', reject);
    req.end(body);
  });
}

let nextId = 0;
async function rpc(method, params, source) {
  const id = ++nextId;
  const reply = await send({ jsonrpc: '2.0', id, method, params }, source);
  assert.equal(reply.status, 200, `Unexpected HTTP status for ${method}`);
  assert.match(reply.headers['content-type'], /^application\/json/);
  const message = JSON.parse(reply.body);
  assert.equal(message.jsonrpc, '2.0');
  assert.equal(message.id, id);
  assert.equal(message.error, undefined, `JSON-RPC error for ${method}`);
  return message.result;
}
async function tool(name, args, source) {
  const result = await rpc('tools/call', { name, arguments: args }, source);
  assert.notEqual(result.isError, true, `Tool failed: ${name}`);
  const value = JSON.parse(result.content.find(item => item.type === 'text').text);
  assert.match(value.today, /^\d{4}-\d{2}-\d{2}$/);
  return value;
}

const clientCount = 20;
const clients = Array.from({ length: clientCount }, (_, i) => `shared-client-${i}`);
const results = await Promise.all(clients.map(async source => {
  const initialized = await rpc('initialize', {
    protocolVersion: '2025-11-25', capabilities: {}, clientInfo: { name: source, version: '1' },
  }, source);
  assert.equal(initialized.protocolVersion, '2025-11-25');
  assert.equal(initialized.serverInfo.version, expectedVersion);
  const notification = await send({ jsonrpc: '2.0', method: 'notifications/initialized' }, source);
  assert.equal(notification.status, 202);
  assert.equal(notification.body, '');
  const catalog = await rpc('tools/list', {}, source);
  assert.ok(catalog.tools.some(item => item.name === 'list_tasks'));
  assert.ok(catalog.tools.some(item => item.name === 'add_tasks'));
  assert.ok(!catalog.tools.some(item => item.name === 'set_setting'));
  return tool('list_tasks', {}, source);
}));
assert.equal(new Set(results.map(result => result.today)).size, 1);

const ping = { jsonrpc: '2.0', id: ++nextId, method: 'ping' };
assert.equal((await send(ping, clients[0], { Authorization: 'Bearer invalid' })).status, 401);
assert.equal((await send(ping, clients[0], { Origin: 'https://example.invalid' })).status, 403);
assert.equal((await send(ping, clients[0], { Host: 'example.invalid' })).status, 403);
assert.equal((await send(undefined, clients[0], {}, 'GET')).status, 405);

if (writeFixture) {
  const date = results[0].today;
  const created = await tool('add_tasks', { date, tasks: [{ title: 'Shared MCP installer fixture' }] }, clients[0]);
  const id = created.tasks[0].id;
  try {
    const observed = await tool('list_tasks', { date }, clients[1]);
    assert.equal(observed.tasks.find(task => task.id === id)?.source, clients[0]);
    await tool('update_task', { id, title: 'Shared MCP fixture updated' }, clients[1]);
    const updated = await tool('list_tasks', { date }, clients[2]);
    assert.equal(updated.tasks.find(task => task.id === id)?.title, 'Shared MCP fixture updated');
  } finally {
    await tool('delete_task', { id }, clients[1]);
  }
  const after = await tool('list_tasks', { date }, clients[2]);
  assert.ok(!after.tasks.some(task => task.id === id));
}
// A new client can reconnect without an old session ID or a new server process.
const reconnected = await rpc('initialize', {
  protocolVersion: '2025-11-25', capabilities: {}, clientInfo: { name: 'reconnected', version: '1' },
}, 'reconnected');
assert.equal(reconnected.serverInfo.version, expectedVersion);
await tool('get_summary', {}, 'reconnected');
console.log(JSON.stringify({ passed: true, clientCount, version: expectedVersion, transport: 'streamable-http', sharedReadWrite: writeFixture, authAndOrigin: true, reconnect: true }));
