---
'@forklaunch/universal-sdk': patch
---

`universalSdk({ registryOptions: { raw } })` rejects an invalid in-process registry document at creation again. 1.4.0 deferred every registry error to the first call so an unreachable registry would not fail creation; a supplied document is always reachable.
