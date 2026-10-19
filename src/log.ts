// Do not serialize driver errors, URLs, environment values, headers, or payloads.
export function errorCode(error: unknown): string {
  if (typeof error === 'object' && error !== null && 'code' in error &&
    typeof error.code === 'string' && /^[A-Z0-9_]{2,40}$/.test(error.code)) return error.code;
  return 'UNCLASSIFIED';
}

export function log(event: string, fields: Record<string, string | number> = {}) {
  console.log(JSON.stringify({ time: new Date().toISOString(), event, ...fields }));
}
