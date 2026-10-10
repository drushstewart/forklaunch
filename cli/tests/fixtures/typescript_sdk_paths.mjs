import assert from 'node:assert/strict';
import { pathToFileURL } from 'node:url';
const { createForkLaunchSdk } = await import(pathToFileURL(process.argv[2]));
let captured;
const params = {
  id: 'a/b ?#%é🦀',
  id2: 'second',
  other: ':id2/{id}',
  literal: 'ignored'
};
const client = createForkLaunchSdk({
  baseUrl: 'https://fixture.invalid',
  beforeRequest: meta => ({
    ...meta,
    path: '/users/:id/:id2/{other}/literal:id/{missing}',
    input: { params }
  }),
  fetch: async url => {
    captured = url;
    return new Response('{}', { headers: { 'content-type': 'application/json' } });
  }
});
await client.accounts.users.list();
assert.equal(captured, 'https://fixture.invalid/users/a%2Fb%20%3F%23%25%C3%A9%F0%9F%A6%80/second/%3Aid2%2F%7Bid%7D/literal:id/{missing}');
console.log('PASS generated TypeScript SDK path parameter conformance');
