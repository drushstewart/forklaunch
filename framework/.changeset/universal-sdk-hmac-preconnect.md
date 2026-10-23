---
'@forklaunch/universal-sdk': patch
---

`hmacSignedFetch` stays assignable to Bun's `typeof fetch`: the signed wrapper now carries `preconnect` and forwards it to the underlying fetch when that fetch has one.
