import { createForkLaunchSdk, type TreesTreesWriteTreeRequest } from './index.js';

export async function verify(baseUrl: string) {
  const sdk = createForkLaunchSdk({ baseUrl });
  const read = await sdk.trees.trees.readTree();
  if (read.code !== 200) throw Error('Tree response missing');
  const tree = read.response;
  if (tree.label !== '水 🦀' || tree.children[0]?.label !== 'leaf' || tree.parent !== null || tree.optional !== undefined) throw Error('Recursive tree type/data mismatch');
  const input: TreesTreesWriteTreeRequest = { body: tree };
  // @ts-expect-error Recursive children retain their object type.
  const bad: TreesTreesWriteTreeRequest = { body: { label: 'bad', children: [42], parent: null } };
  void bad;
  const echo = await sdk.trees.trees.writeTree(input);
  if (echo.code !== 200 || JSON.stringify(echo.response) !== JSON.stringify(tree)) throw Error('Recursive request changed');
  const forest = await sdk.trees.forests.readForest();
  if (forest.code !== 200 || forest.response[1]?.[0]?.length !== 0) throw Error('Recursive array mismatch');
  const choice = await sdk.trees.choices.readChoice();
  if (choice.code !== 200 || typeof choice.response === 'string' || Array.isArray(choice.response)) throw Error('Recursive union mismatch');
  const nested = choice.response['nested'];
  if (!Array.isArray(nested) || nested[0] !== 'leaf') throw Error('Recursive dictionary mismatch');
  const mutual = await sdk.trees.mutual.readMutual();
  if (mutual.code !== 200 || mutual.response.peer.children[0]?.id !== 'inner') throw Error('Mutual recursive model mismatch');
  console.log('PASS typed TypeScript SDK recursive request/response, optional/null, arrays and mutual records');
}
