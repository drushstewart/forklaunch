---
'@forklaunch/universal-sdk': minor
---

Per-request HMAC signing for internal (`access: 'internal'`) routes. Pass `hmac: { secretKey, keyId? }` to `universalSdk` (or the portable runtime config) and every API request is signed with a fresh timestamp and random nonce over the final method, full URL path (no query) and body bytes. Registry discovery stays unsigned. `createHmacAuthorization` and `hmacSignedFetch` (with the `HmacSigningOptions` type) expose the same signer for hand-written calls; they use Web Crypto, so they run in browsers, Node and Bun. The signed path is the full wire path that `@forklaunch/core` now verifies by default.

Also: response validation accepts the native integer formats Rust services emit (`int8` to `uint128`, `uint`, `int`) as bounds checks instead of warning about unknown formats, and versioned multipart bodies iterate `FormData` with `forEach` so they work in runtimes without `FormData` iterators.
