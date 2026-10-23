---
'@forklaunch/universal-sdk': patch
---

Typed SDK calls accept a `content-type` header on bodyless responses (HEAD, 204, 205, 304) when the operation declares an empty response. Real servers commonly label these; the client previously rejected them as an undeclared content type.
