import pg from 'pg';
import type { DatabaseConfig } from './config.ts';
import { errorCode, log } from './log.ts';

export function createPool(config: DatabaseConfig) {
  const pool = new pg.Pool({
    ...config,
    application_name: 'analytics-service',
    max: 5,
    connectionTimeoutMillis: 2_000,
    idleTimeoutMillis: 30_000,
    maxLifetimeSeconds: 300,
    statement_timeout: 2_000,
    query_timeout: 3_000,
    idle_in_transaction_session_timeout: 5_000,
  });
  pool.on('error', (error) => log('database_idle_connection_error', { code: errorCode(error) }));
  return pool;
}

export const POSTGRES_MAJOR = 18;
