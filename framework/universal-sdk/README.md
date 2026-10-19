
### Native service declarations

A native service publishes type-only `sdk.ts` declarations, including its named
DTOs and `NotesSdkClient`. Consumers import those types and use this package:

```ts
import type { NotesSdkClient, HealthResponse } from '@app/notes/sdk';
import { universalSdk } from '@forklaunch/universal-sdk';

const notesSdkClient = universalSdk<NotesSdkClient>;
const notes = await notesSdkClient({
  host: 'https://notes.example.test',
  registryOptions: { url: 'https://notes.example.test/api-definitions' }
});
```

Native registry operations carry `x-forklaunch-operation` descriptors. The
framework validates their format and dispatches through the checked portable
engine, retaining schema, status, media, header, date and SSE validation. Unknown
formats fail closed. A type import alone cannot supply runtime validation;
consumers must use the matching checked registry and this runtime version.
Ordinary TypeScript OpenAPI registries keep the existing dispatch path.

`src/portableRuntime.ts` is a framework-owned generated snapshot of the canonical
Rust compiler runtime. The core `sdk_package_layout` test checks exact equality;
regenerate from its `FORKLAUNCH_PACKAGE_SDK_FIXTURE` output when changing that
runtime. Service packages contain declarations, not copies of this engine.
