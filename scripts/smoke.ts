const base = new URL(process.env.SMOKE_BASE_URL ?? 'http://127.0.0.1:3000');
if (!['http:', 'https:'].includes(base.protocol) || base.username || base.password) throw new Error('Invalid SMOKE_BASE_URL');
for (const path of ['/health/live', '/health/ready']) {
  const response = await fetch(new URL(path, base), { signal: AbortSignal.timeout(5_000) });
  if (!response.ok) throw new Error(`${path} returned ${response.status}`);
  const result: unknown = await response.json();
  console.log(path, result);
}
