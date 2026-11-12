// Read-only acceptance through the official SDK, including its background GET
// probe. No model calls, CLI subprocesses, retries or user-data writes.
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { Client } from '@modelcontextprotocol/sdk/client/index.js';
import { StreamableHTTPClientTransport } from '@modelcontextprotocol/sdk/client/streamableHttp.js';

const [endpoint, tokenFile, expectedVersion, roundsText = '10'] = process.argv.slice(2);
if (!endpoint || !tokenFile || !expectedVersion) {
  throw new Error('Usage: node scripts/verify-sdk-mcp.mjs <loopback /mcp URL> <token-file> <version> [rounds]');
}
const url = new URL(endpoint);
assert.equal(url.protocol, 'http:');
assert.equal(url.hostname, '127.0.0.1');
assert.equal(url.pathname, '/mcp');
assert.ok(!url.search && !url.hash && !url.username && !url.password);
const rounds = Number(roundsText);
assert.ok(Number.isInteger(rounds) && rounds > 0 && rounds <= 100);
const token = readFileSync(tokenFile, 'utf8').trim();
assert.match(token, /^[0-9a-f]{64}$/i, 'Invalid token file');
const clientCount = 20;

for (let round = 0; round < rounds; round++) {
  const clients = [];
  const pending = new Set();
  const transportErrors = [];
  const fetchErrors = [];
  let getProbes = 0;
  let get405 = 0;
  let closing = false;
  const trackedFetch = (input, init = {}) => {
    const operation = (async () => {
      const isGet = (init.method ?? 'GET') === 'GET';
      if (isGet) getProbes++;
      try {
        // Bound failures in the acceptance runner without changing parallelism
        // or adding retries. The SDK's optional GET also needs a deadline.
        const timeout = AbortSignal.timeout(15_000);
        const signal = init.signal ? AbortSignal.any([init.signal, timeout]) : timeout;
        const response = await fetch(input, { ...init, signal });
        if (isGet && response.status === 405) get405++;
        return response;
      } catch (error) {
        fetchErrors.push(error.cause?.code ?? error.name);
        throw error;
      }
    })();
    pending.add(operation);
    operation.then(() => pending.delete(operation), () => pending.delete(operation));
    return operation;
  };
  try {
    const results = await Promise.allSettled(Array.from({ length: clientCount }, async (_, index) => {
      const name = `sdk-${round}-${index}`;
      const client = new Client({ name, version: '1' });
      clients.push(client);
      client.onerror = error => { if (!closing) transportErrors.push(error.cause?.code ?? error.name); };
      await client.connect(new StreamableHTTPClientTransport(url, {
        fetch: trackedFetch,
        requestInit: { headers: { Authorization: `Bearer ${token}`, 'X-DayNote-Source': name } },
      }), { timeout: 15_000 });
      assert.equal(client.getServerVersion().version, expectedVersion);
      const catalog = await client.listTools();
      assert.ok(catalog.tools.some(tool => tool.name === 'get_summary'));
      const result = await client.callTool({ name: 'get_summary', arguments: {} });
      assert.notEqual(result.isError, true);
      const summary = JSON.parse(result.content.find(item => item.type === 'text').text);
      assert.match(summary.today, /^\d{4}-\d{2}-\d{2}$/);
    }));
    // Do not abort successful peers when one fails, or ignore an independent
    // background GET failure just because all foreground calls succeeded.
    await Promise.allSettled([...pending]);
    const failed = results.filter(result => result.status === 'rejected');
    assert.equal(failed.length, 0, `Round ${round + 1}: ${failed.length} clients failed; fetch errors: ${fetchErrors.join(', ')}`);
    assert.deepEqual(fetchErrors, [], `Round ${round + 1}: HTTP requests failed`);
    assert.deepEqual(transportErrors, [], `Round ${round + 1}: SDK transport errors`);
    assert.equal(getProbes, clientCount, 'Every client must perform its real background GET');
    assert.equal(get405, clientCount, 'Every background GET must receive the normal 405 response');
  } finally {
    closing = true;
    await Promise.allSettled(clients.map(client => client.close()));
  }
}
console.log(JSON.stringify({ passed: true, sdk: '@modelcontextprotocol/sdk', clientCount, rounds,
  connectionsVerified: clientCount * rounds, backgroundGetProbes: clientCount * rounds,
  version: expectedVersion, readOnly: true }));
