---
'@forklaunch/validator': minor
---

Schema export additions and fixes, used by the OpenAPI generator and by Rust and SDK consumers:

- New optional `SchemaValidator` members: `openapiRequestBody(schema)` (root projection that says whether a request body is required, without running defaults or refinements), `openapi2020(schema, options)` (explicit JSON Schema 2020-12 input export with opt-in `pureInput`, `zodChecks`, `zodRegex` and `zodDiagnostics` profiles), and `undefined`/`void` markers. Zod and TypeBox implement `openapiRequestBody`; Zod implements `openapi2020`, also exported as a static helper from `@forklaunch/validator/zod`.
- The Zod OpenAPI exporter keeps more of what the schema enforces: length, numeric and array bounds, finite record keys, tuples, overlapping unions and enums, string formats (email, UUID, URI, date, time, datetime, duration, IP/CIDR, JWT, emoji, base64) and literal patterns.
- The `email` and `uri` factory schemas export the grammar Zod actually accepts (WHATWG URLs for `uri`) instead of a hand-written `pattern`/`format`.
- `any` and `unknown` no longer export `type: 'object'` (Zod and TypeBox), so documented schemas accept the same values as runtime parsing.
