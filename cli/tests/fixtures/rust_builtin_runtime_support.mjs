import { existsSync } from 'node:fs';
import { readFile } from 'node:fs/promises';

// Read the emitted declaration unchanged; older captures retain their cache layout.
export async function openApiDocument(service) {
  const local = existsSync(`src/${service}/sdk/rust/.forklaunch-sdk-local`);
  const file = local
    ? `src/${service}/sdk/typescript/openapi.json`
    : `.forklaunch/contracts/${service}/openapi.json`;
  return JSON.parse(await readFile(file, 'utf8'));
}
