import assert from 'node:assert/strict';
import { createHash, randomUUID } from 'node:crypto';
import { once } from 'node:events';
import { test } from 'node:test';
import { serve } from '@hono/node-server';
import pg from 'pg';
import { createApp } from '../../src/app.ts';
import { createPool } from '../../src/database.ts';
import { loadMigrations, migrate, type Migration } from '../../src/migrations.ts';

const source = process.env.TEST_DATABASE_ADMIN_URL;
if (!source) throw new Error('TEST_DATABASE_ADMIN_URL is required; use a disposable local PostgreSQL server');
const adminUrl = new URL(source);
if (!['localhost', '127.0.0.1', '[::1]'].includes(adminUrl.hostname) || adminUrl.pathname !== '/postgres') {
  throw new Error('Integration tests require a loopback PostgreSQL administrator connection to /postgres');
}

async function fixture() {
  const admin = new pg.Client({ connectionString: adminUrl.toString() });
  await admin.connect();
  const name = `analytics_test_${randomUUID().replaceAll('-', '')}`;
  const connection = (user: string, password: string) => {
    const url = new URL(adminUrl); url.username = user; url.password = password; url.pathname = `/${name}`;
    return { connectionString: url.toString(), ssl: false as const };
  };
  await admin.query(`CREATE DATABASE ${name}`);
  const owner = new pg.Client(connection(adminUrl.username, adminUrl.password));
  await owner.connect();
  await owner.query(`REVOKE ALL ON DATABASE ${name} FROM PUBLIC;
    GRANT CONNECT ON DATABASE ${name} TO analytics_migrator, analytics_app;
    REVOKE ALL ON SCHEMA public FROM PUBLIC;
    CREATE SCHEMA analytics AUTHORIZATION analytics_migrator`);
  return {
    admin, name, owner,
    migration: connection('analytics_migrator', 'local-migration-only'),
    runtime: connection('analytics_app', 'local-runtime-only'),
    async close() {
      await owner.end();
      await admin.query(`DROP DATABASE ${name} WITH (FORCE)`);
      await admin.end();
    },
  };
}

const migrations = await loadMigrations();
function extra(sql: string): Migration {
  return { version: '000002_test.sql', sql, checksum: createHash('sha256').update(sql).digest('hex') };
}

test('migration locking, idempotency, and immutable history work against PostgreSQL', async () => {
  const db = await fixture();
  try {
    const counts = await Promise.all([migrate(db.migration, migrations), migrate(db.migration, migrations)]);
    assert.deepEqual(counts.sort(), [0, 1]);
    assert.equal(await migrate(db.migration, migrations), 0);
    const changed = migrations.map((migration) => ({ ...migration, checksum: 'changed' }));
    await assert.rejects(migrate(db.migration, changed), /history differs/);
    await assert.rejects(migrate(db.migration, []), /history differs/);
    assert.equal((await db.owner.query('SELECT count(*)::int AS n FROM analytics.schema_migrations')).rows[0].n, 1);
  } finally { await db.close(); }
});

test('a failed migration rolls back SQL and history together', async () => {
  const db = await fixture();
  try {
    await migrate(db.migration, migrations);
    await assert.rejects(migrate(db.migration, [...migrations, extra(`
      CREATE TABLE analytics.must_rollback (id integer);
      SELECT * FROM analytics.this_table_does_not_exist;
    `)]));
    assert.equal((await db.owner.query("SELECT to_regclass('analytics.must_rollback') AS relation")).rows[0].relation, null);
    assert.equal((await db.owner.query('SELECT count(*)::int AS n FROM analytics.schema_migrations')).rows[0].n, 1);
  } finally { await db.close(); }
});

test('runtime can use application data but cannot migrate or elevate itself', async () => {
  const db = await fixture();
  const pool = createPool(db.runtime);
  try {
    await migrate(db.migration, [...migrations, extra('CREATE TABLE analytics.synthetic_probe (id integer PRIMARY KEY, value text NOT NULL)')]);
    await pool.query("INSERT INTO analytics.synthetic_probe VALUES (1, 'before')");
    await pool.query("UPDATE analytics.synthetic_probe SET value = 'after' WHERE id = 1");
    assert.equal((await pool.query('SELECT value FROM analytics.synthetic_probe')).rows[0].value, 'after');
    await pool.query('DELETE FROM analytics.synthetic_probe WHERE id = 1');
    for (const sql of [
      'CREATE TABLE analytics.forbidden (id int)',
      'ALTER TABLE analytics.synthetic_probe ADD COLUMN forbidden int',
      'DELETE FROM analytics.schema_migrations',
      'TRUNCATE analytics.schema_migrations',
      'SET ROLE analytics_migrator',
      'CREATE ROLE forbidden_superuser SUPERUSER',
    ]) await assert.rejects(pool.query(sql), (error: unknown) =>
      typeof error === 'object' && error !== null && 'code' in error && error.code === '42501');
    assert.equal(pool.options.max, 5);
  } finally { await pool.end(); await db.close(); }
});

test('HTTP readiness rejects unmigrated schema, database outages, and drift, then recovers', async () => {
  const db = await fixture();
  const pool = createPool(db.runtime);
  const service = createApp(pool, migrations, 'integration');
  const server = serve({ fetch: service.app.fetch, port: 0, hostname: '127.0.0.1' });
  if (!server.listening) await once(server, 'listening');
  const address = server.address();
  assert(address && typeof address !== 'string');
  const request = (path: string) => fetch(`http://127.0.0.1:${address.port}${path}`, { signal: AbortSignal.timeout(6_000) });
  try {
    assert.equal((await request('/health/live')).status, 200);
    assert.equal((await request('/health/ready')).status, 503);
    await migrate(db.migration, migrations);
    const ready = await request('/health/ready');
    assert.equal(ready.status, 200);
    assert.equal(ready.headers.get('cache-control'), 'no-store');
    assert.deepEqual(await ready.json(), { status: 'ready', release: 'integration' });
    await db.admin.query(`ALTER DATABASE ${db.name} ALLOW_CONNECTIONS false`);
    await db.admin.query('SELECT pg_terminate_backend(pid) FROM pg_stat_activity WHERE datname = $1 AND usename = $2', [db.name, 'analytics_app']);
    assert.equal((await request('/health/live')).status, 200);
    assert.equal((await request('/health/ready')).status, 503);
    await db.admin.query(`ALTER DATABASE ${db.name} ALLOW_CONNECTIONS true`);
    assert.equal((await request('/health/ready')).status, 200);
    await db.owner.query("UPDATE analytics.schema_migrations SET checksum = 'drift'");
    assert.equal((await request('/health/ready')).status, 503);
    service.drain();
    assert.equal((await request('/health/live')).status, 503);
    assert.equal((await request('/health/ready')).status, 503);
  } finally {
    await new Promise<void>((resolve, reject) => server.close((error) => error ? reject(error) : resolve()));
    await pool.end();
    await db.close();
  }
});

test('readiness rejects schema-owner credentials even with a current schema', async () => {
  const db = await fixture();
  const pool = createPool(db.migration);
  try {
    await migrate(db.migration, migrations);
    const { app } = createApp(pool, migrations, 'integration');
    assert.equal((await app.request('/health/ready')).status, 503);
  } finally { await pool.end(); await db.close(); }
});
