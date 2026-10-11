# Native Better Auth module

This crate implements the configured Better Auth 1.7.6 protocol incrementally.
It currently provides compatible scrypt password hashes and signed session
cookies, Ed25519 JWT issuance and public JWKs. It does not yet expose the authentication HTTP API or persistent
session/organization management and is not selectable as a CLI module preset.

`credentials` uses N=16384, r=16, p=1, a 64-byte derived key, NFKC-normalized
passwords and a random 16-byte salt encoded as lowercase hex text. The salt text
itself is the scrypt input, matching `@better-auth/utils`. Call these CPU-bound
operations using a bounded blocking executor from async HTTP handlers.

`cookies` implements Better Call's SHA-256 HMAC, padded standard base64,
JavaScript URI-component encoding, first-cookie selection and configured
24-hour persistent/transient/secure cookie options. Verification returns a
token only; callers must check persistent session existence, expiry, revocation
and tenant membership. Signing a cookie does not authenticate a user.

`cli/tests/rust_better_auth_crypto.mjs` compares both directions with the actual
installed JavaScript libraries. The existing Better Auth protocol and callback
fixtures cover the configured TypeScript service separately.

`jwt::SigningKey` issues the configured EdDSA payload (subject, email, active
organization, issuer/audience, issued time and 15-minute expiry). Public JWKs
contain no private material. The PKCS#8 persistence representation must be
wrapped in tenant-bound encryption by storage. Key restoration preserves IDs
and signatures; database lifecycle/rotation is still pending. The JWT fixture
compares native tokens/JWKS with actual configured Better Auth signing and JOSE
verification, including expiry, issuer/audience and signature failures.
