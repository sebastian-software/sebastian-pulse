import { createHash } from 'node:crypto';
import { readdir, readFile } from 'node:fs/promises';
import pg from 'pg';
import type { DatabaseConfig } from './config.ts';
import { POSTGRES_MAJOR } from './database.ts';

export type Migration = { version: string; checksum: string; sql: string };

export async function loadMigrations(directory = new URL('../migrations/', import.meta.url)): Promise<Migration[]> {
  const names = (await readdir(directory)).filter((name) => name.endsWith('.sql')).sort();
  if (!names.length) throw new Error('No SQL migrations found');
  return Promise.all(names.map(async (name, index) => {
    if (!/^\d{6}_[a-z0-9_]+\.sql$/.test(name) || Number(name.slice(0, 6)) !== index + 1) {
      throw new Error('Migrations must have consecutive numbered filenames');
    }
    const sql = await readFile(new URL(name, directory), 'utf8');
    return { version: name, sql, checksum: createHash('sha256').update(sql).digest('hex') };
  }));
}

export function matchesHistory(rows: { version: string; checksum: string }[], migrations: Migration[], complete: boolean) {
  return (!complete || rows.length === migrations.length) && rows.length <= migrations.length &&
    rows.every((row, index) => row.version === migrations[index]?.version && row.checksum === migrations[index]?.checksum);
}

export async function migrate(config: DatabaseConfig, migrations: Migration[]) {
  const client = new pg.Client({
    ...config, application_name: 'analytics-migrator', connectionTimeoutMillis: 5_000,
    statement_timeout: 30_000, lock_timeout: 10_000, idle_in_transaction_session_timeout: 30_000,
  });
  await client.connect();
  try {
    const version = await client.query<{ major: number }>("SELECT current_setting('server_version_num')::int / 10000 AS major");
    if (version.rows[0]?.major !== POSTGRES_MAJOR) throw new Error('Unexpected PostgreSQL major version');
    // The transaction serializes migrators and rolls back all pending SQL and history on failure.
    await client.query('BEGIN');
    try {
      await client.query('SELECT pg_advisory_xact_lock(13752787, 1)');
      await client.query(`CREATE TABLE IF NOT EXISTS analytics.schema_migrations (
        version text PRIMARY KEY, checksum text NOT NULL, applied_at timestamptz NOT NULL DEFAULT now()
      )`);
      const history = await client.query<{ version: string; checksum: string }>(
        'SELECT version, checksum FROM analytics.schema_migrations ORDER BY version');
      if (!matchesHistory(history.rows, migrations, false)) throw new Error('Migration history differs from this release');
      const pending = migrations.slice(history.rowCount ?? 0);
      for (const migration of pending) {
        await client.query(migration.sql);
        await client.query('INSERT INTO analytics.schema_migrations (version, checksum) VALUES ($1, $2)',
          [migration.version, migration.checksum]);
      }
      await client.query('COMMIT');
      return pending.length;
    } catch (error) {
      await client.query('ROLLBACK');
      throw error;
    }
  } finally { await client.end(); }
}
