import type Ajv from 'ajv';

// A reserved factory format, distinct from arbitrary RFC 3986 uri schemas.
export function registerFactoryUriFormat(ajv: Ajv): Ajv {
  ajv.addFormat('forklaunch-whatwg-url', {
    type: 'string',
    validate: (value: string) => {
      try {
        new URL(value);
        return true;
      } catch {
        return false;
      }
    }
  });
  return ajv;
}
