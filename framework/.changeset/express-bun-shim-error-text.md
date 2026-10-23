---
'@forklaunch/express': patch
---

The Bun cluster shim's last-resort 500 response no longer stringifies non-Error throws in development; it shows an Error's message in development and `Internal Server Error` otherwise.
