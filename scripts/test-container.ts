import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { setTimeout } from 'node:timers/promises';

// Only this generated project is stopped or removed; never use the developer's Compose stack.
const project = `analytics-foundation-test-${process.pid}`;
const env = { ...process.env, APP_PORT: '0', POSTGRES_PORT: '0' };
function docker(args: string[]) {
  const result = spawnSync('docker', args, { env, encoding: 'utf8', timeout: 180_000, maxBuffer: 4 * 1024 * 1024 });
  if (result.status !== 0) throw new Error(`Docker failed: ${args.join(' ')}\n${result.stderr}\n${result.stdout}`);
  return result.stdout.trim();
}
const compose = (...args: string[]) => docker(['compose', '-p', project, ...args]);
let base: string;
async function status(path: string) {
  return (await fetch(`${base}${path}`, { signal: AbortSignal.timeout(5_000) })).status;
}
async function waitForReadiness(expected: number) {
  const deadline = Date.now() + 30_000;
  while (Date.now() < deadline) {
    if (await status('/health/ready') === expected) return;
    await setTimeout(200);
  }
  throw new Error(`Readiness did not become ${expected}`);
}
try {
  compose('up', '-d', '--no-build', '--wait', '--wait-timeout', '90');
  base = `http://${compose('port', 'app', '3000')}`;
  assert.equal(await status('/health/live'), 200);
  assert.equal(await status('/health/ready'), 200);
  assert.equal(compose('exec', '-T', 'app', 'id', '-u'), '1000');
  assert.equal(compose('exec', '-T', 'app', 'node', '-p', 'Boolean(process.env.DATABASE_MIGRATION_URL)'), 'false');
  compose('stop', 'postgres');
  assert.equal(await status('/health/live'), 200);
  await waitForReadiness(503);
  compose('start', 'postgres');
  await waitForReadiness(200);
  compose('stop', '-t', '25', 'app');
  const id = compose('ps', '-a', '-q', 'app');
  assert.equal(docker(['inspect', '--format', '{{.State.ExitCode}}', id]), '0');
  const logs = compose('logs', '--no-color', 'app');
  assert.match(logs, /"event":"shutdown_complete"/);
  assert.doesNotMatch(logs, /local-runtime-only|local-migration-only|postgresql:\/\//);
  console.log('Container smoke passed: fresh migrations, non-root runtime, outage/reconnect, SIGTERM, secret-free logs');
} catch (error) {
  console.error(compose('logs', '--no-color'));
  throw error;
} finally {
  compose('down', '--volumes', '--remove-orphans');
}
