import { databaseConfig } from './config.ts';
import { loadMigrations, migrate } from './migrations.ts';
import { errorCode, log } from './log.ts';

try {
  const applied = await migrate(databaseConfig(process.env, 'migration'), await loadMigrations());
  log('migrations_complete', { applied });
} catch (error) {
  log('migration_failed', { code: errorCode(error) });
  process.exitCode = 1;
}
