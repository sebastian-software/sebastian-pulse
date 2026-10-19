import assert from 'node:assert/strict';
import { test } from 'node:test';
import { databaseConfig, runtimeConfig } from '../../src/config.ts';
import { errorCode } from '../../src/log.ts';

const DATABASE_URL = 'postgresql://app:fixture@db.example/analytics';

test('remote connections verify certificates and hostname by default', () => {
  assert.deepEqual(databaseConfig({ DATABASE_URL }, 'runtime').ssl, { rejectUnauthorized: true });
  assert.deepEqual(databaseConfig({ DATABASE_URL, DATABASE_CA_CERT: 'fixture' }, 'runtime').ssl,
    { rejectUnauthorized: true, ca: 'fixture' });
});

test('plaintext is explicit and restricted to local/test environments', () => {
  for (const APP_ENV of [undefined, 'production', 'preview']) {
    assert.throws(() => databaseConfig({ DATABASE_URL, APP_ENV, DATABASE_TLS: 'disable' }, 'runtime'));
  }
  assert.equal(databaseConfig({ DATABASE_URL, APP_ENV: 'test', DATABASE_TLS: 'disable' }, 'runtime').ssl, false);
  assert.throws(() => databaseConfig({ DATABASE_URL, DATABASE_TLS: 'require' }, 'runtime'));
});

test('URL TLS settings cannot weaken certificate verification', () => {
  for (const parameter of ['sslmode=disable', 'sslmode=no-verify', 'ssl=false', 'sslcert=anything']) {
    assert.throws(() => databaseConfig({ DATABASE_URL: `${DATABASE_URL}?${parameter}` }, 'runtime'));
  }
});

test('configuration errors do not echo credentials or accept migration credentials in runtime', () => {
  for (const DATABASE_URL of ['secret-fixture', 'https://app:secret-fixture@db.example/analytics']) {
    assert.throws(() => databaseConfig({ DATABASE_URL }, 'runtime'), (error: unknown) =>
      error instanceof Error && !error.message.includes('secret-fixture'));
  }
  assert.throws(() => runtimeConfig({ DATABASE_URL, DATABASE_MIGRATION_URL: DATABASE_URL }));
  assert.throws(() => runtimeConfig({ DATABASE_URL, PORT: '0' }));
  assert.throws(() => runtimeConfig({ DATABASE_URL, RELEASE_ID: 'unsafe\nvalue' }));
});

test('logs expose error codes without serializing driver messages or untrusted codes', () => {
  assert.equal(errorCode({ code: 'ECONNREFUSED', message: DATABASE_URL }), 'ECONNREFUSED');
  assert.equal(errorCode({ code: DATABASE_URL }), 'UNCLASSIFIED');
  assert.equal(errorCode(new Error(DATABASE_URL)), 'UNCLASSIFIED');
});
