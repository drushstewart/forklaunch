import { createForkLaunchSdk } from './sdks/typescript/src/index';
async function useResponses(client = createForkLaunchSdk()) {
  const users = await client.accounts.users.listUsers({ query: {} });
  if (users.code === 200) { const id: string = users.response.records[0].id; void id; }
  // @ts-expect-error The optional query label is a string or null.
  await client.accounts.users.listUsers({ query: { label: 42 } });
  const deleted = await client.accounts.users.deleteUser({ params: { id: 'welcome' } });
  if (deleted.code === 204) { const empty: undefined = deleted.response; void empty; }
  const emptyReply = await client.accounts.empty.getEmpty();
  if (emptyReply.code === 200) { const empty: undefined = emptyReply.response; void empty; }
  const several = await client.accounts.status.getStatus({ params: { mode: 'created' } });
  if (several.code === 200) { const value: string = several.response.displayName; void value; }
  if (several.code === 201) { const value: string = several.response.createdAt; void value; }
  if (several.code === 202) { const value: string = several.response.jobId; void value; }
  if (several.code === 204 || several.code === 205) { const value: undefined = several.response; void value; }
  if (several.code === 400) { const value: string = several.response.errorCode; const mime: 'application/problem+json' = several.contentType; void value; void mime; }
  if (several.code === 409) { const value: number = several.response.retryAfter; void value; }
  const textReply = await client.accounts.text.getText();
  if (textReply.code === 200) { const value: string = textReply.response; void value; }
  const binaryReply = await client.accounts.binary.getBinary();
  if (binaryReply.code === 200) { const value: Uint8Array = binaryReply.response; void value; }
  const shapeReply = await client.accounts.shape.getShape({ params: { mode: 'null' } });
  if (shapeReply.code === 200) {
    const label: string | null = shapeReply.response.label;
    const number: number | null = shapeReply.response.numbers.one;
    const metadata: { id: string } | null = shapeReply.response.metadata;
    const id: string = shapeReply.response.open.id;
    // @ts-expect-error Implicit wire openness does not widen a DTO's declared keys.
    shapeReply.response.open.extra;
    const extra: unknown = shapeReply.response.constrained.extra;
    void label; void number; void metadata; void id; void extra;
  }
  await client.accounts.text.uploadText({ body: 'hello' });
  await client.accounts.binary.uploadBinary({ body: new Uint8Array([0, 255]) });
  // @ts-expect-error Binary bodies require bytes, not a JSON string.
  await client.accounts.binary.uploadBinary({ body: 'wrong' });
  // @ts-expect-error Text bodies require a string.
  await client.accounts.text.uploadText({ body: { wrong: true } });
  const nullReply = await client.accounts.nullJson.getNull();
  if (nullReply.code === 200) { const value: null = nullReply.response; void value; }
}
void useResponses;
