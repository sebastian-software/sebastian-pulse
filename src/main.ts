import { createServer } from 'node:http';
import { getRequestListener } from '@hono/node-server';
import { createApp } from './app.ts';
import { runtimeConfig, ConfigurationError } from './config.ts';
import { createPool } from './database.ts';
import { loadMigrations } from './migrations.ts';
import { errorCode, log } from './log.ts';

try {
  const config = runtimeConfig(process.env);
  const pool = createPool(config.database);
  const service = createApp(pool, await loadMigrations(), config.release);
  const server = createServer({ requestTimeout: 10_000, headersTimeout: 10_000, keepAliveTimeout: 5_000 },
    getRequestListener(service.app.fetch));
  server.listen(config.port, '0.0.0.0', () => {
    log('service_started', { release: config.release, port: config.port });
  });
  server.on('error', (error) => { log('server_failed', { code: errorCode(error) }); void shutdown(1); });
  let stopping = false;
  async function shutdown(exitCode = 0) {
    if (stopping) return;
    stopping = true;
    service.drain();
    log('shutdown_started');
    // Finish within Bunny's default 30-second grace period, including database cleanup.
    const deadline = setTimeout(() => { log('shutdown_deadline'); process.exit(1); }, 20_000);
    try {
      await new Promise<void>((resolve, reject) => server.close((error) => error ? reject(error) : resolve()));
      await pool.end();
      process.exitCode = exitCode;
      log('shutdown_complete');
    } catch (error) {
      log('shutdown_failed', { code: errorCode(error) });
      process.exitCode = 1;
    } finally { clearTimeout(deadline); }
  }
  process.once('SIGTERM', () => { void shutdown(); });
  process.once('SIGINT', () => { void shutdown(); });
} catch (error) {
  log('startup_failed', error instanceof ConfigurationError ? { reason: error.message } : { code: errorCode(error) });
  process.exitCode = 1;
}
