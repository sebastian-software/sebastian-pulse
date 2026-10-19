export type Environment = Record<string, string | undefined>;
export type DatabaseConfig = {
  connectionString: string;
  ssl: false | { rejectUnauthorized: true; ca?: string };
};

export class ConfigurationError extends Error {}

export function databaseConfig(env: Environment, purpose: 'runtime' | 'migration'): DatabaseConfig {
  const key = purpose === 'runtime' ? 'DATABASE_URL' : 'DATABASE_MIGRATION_URL';
  const value = env[key];
  if (!value) throw new ConfigurationError(`${key} is required`);
  let url: URL;
  try { url = new URL(value); }
  catch { throw new ConfigurationError(`${key} must be a PostgreSQL URL`); }
  if (!['postgres:', 'postgresql:'].includes(url.protocol) || !url.hostname || url.pathname.length < 2) {
    throw new ConfigurationError(`${key} must name a PostgreSQL host and database`);
  }
  // pg parses URL SSL options after pool options. Keep one authoritative TLS setting.
  if ([...url.searchParams.keys()].some((name) => name.toLowerCase().startsWith('ssl'))) {
    throw new ConfigurationError(`Use DATABASE_TLS and DATABASE_CA_CERT instead of URL SSL options`);
  }
  const mode = env.DATABASE_TLS ?? 'verify-full';
  if (mode === 'disable') {
    if (env.APP_ENV !== 'local' && env.APP_ENV !== 'test') {
      throw new ConfigurationError('Disabling database TLS requires APP_ENV=local or test');
    }
    return { connectionString: value, ssl: false };
  }
  if (mode !== 'verify-full') throw new ConfigurationError('DATABASE_TLS must be verify-full or disable');
  return {
    connectionString: value,
    ssl: { rejectUnauthorized: true, ...(env.DATABASE_CA_CERT ? { ca: env.DATABASE_CA_CERT } : {}) },
  };
}

export function runtimeConfig(env: Environment) {
  const port = Number(env.PORT ?? '3000');
  if (!Number.isInteger(port) || port < 1 || port > 65535) throw new ConfigurationError('PORT must be 1–65535');
  const release = env.RELEASE_ID ?? 'local';
  if (!/^[a-zA-Z0-9._-]{1,128}$/.test(release)) throw new ConfigurationError('RELEASE_ID must be a safe release identifier');
  if (env.DATABASE_MIGRATION_URL) throw new ConfigurationError('Do not provide migration credentials to the runtime');
  return { port, release, database: databaseConfig(env, 'runtime') };
}
