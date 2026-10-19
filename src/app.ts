import { Hono } from 'hono';
import type pg from 'pg';
import { POSTGRES_MAJOR } from './database.ts';
import { matchesHistory, type Migration } from './migrations.ts';
import { errorCode, log } from './log.ts';

export function createApp(pool: pg.Pool, migrations: Migration[], release: string) {
  const app = new Hono();
  let draining = false;
  let previousReady: boolean | undefined;
  // Coalesce concurrent probes, so health traffic cannot fill the pool's waiting queue.
  let pendingProbe: Promise<boolean> | undefined;

  async function probe(): Promise<boolean> {
    try {
      const result = await pool.query<{ version: string; checksum: string; safe_role: boolean; major: number }>(`
        SELECT m.version, m.checksum,
          NOT r.rolsuper AND NOT r.rolbypassrls
            AND NOT has_schema_privilege(current_user, 'analytics', 'CREATE')
            AND NOT has_table_privilege(current_user, 'analytics.schema_migrations', 'INSERT,UPDATE,DELETE,TRUNCATE')
            AS safe_role,
          current_setting('server_version_num')::int / 10000 AS major
        FROM analytics.schema_migrations m CROSS JOIN pg_roles r
        WHERE r.rolname = current_user ORDER BY m.version
      `);
      return result.rows.every((row) => row.safe_role && row.major === POSTGRES_MAJOR) &&
        matchesHistory(result.rows, migrations, true);
    } catch (error) {
      if (previousReady !== false) log('database_probe_failed', { code: errorCode(error) });
      return false;
    }
  }

  app.use('*', async (context, next) => {
    context.header('Cache-Control', 'no-store');
    if (draining) return context.json({ status: 'draining', release }, 503);
    await next();
  });
  app.get('/health/live', (context) => context.json({ status: 'live', release }));
  app.get('/health/ready', async (context) => {
    pendingProbe ??= probe().finally(() => { pendingProbe = undefined; });
    const ready = await pendingProbe;
    if (ready !== previousReady) log('readiness_changed', { ready: ready ? 1 : 0 });
    previousReady = ready;
    return context.json({ status: ready && !draining ? 'ready' : 'unavailable', release }, ready && !draining ? 200 : 503);
  });
  app.notFound((context) => context.json({ error: 'not_found' }, 404));
  app.onError((error, context) => {
    log('request_failed', { code: errorCode(error) });
    return context.json({ error: 'internal_error' }, 500);
  });

  return { app, drain: () => { draining = true; } };
}
