---
'@forklaunch/express': patch
---

The final error handler sends its plain-text diagnostic with Express's own `send`, so it is no longer validated against the failing route's contract (which could throw again and return Express's HTML stack trace). `res.setHeader` with an array value now sets repeated header values instead of joining them with newlines.
