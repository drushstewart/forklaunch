---
'@forklaunch/ws': minor
---

`ForklaunchWebSocket` and `ForklaunchWebSocketServer` answer pings themselves: an incoming ping is validated against the ping schema and echoed as a pong validated against the pong schema. A ping or close frame that fails validation closes the connection with 1008 (`invalid payload`), emits `error`, and terminates the socket if the close handshake has not finished within 5 s. Pass `autoPong: false` to turn automatic pongs off as before. `pong` and `close` accept raw `Buffer` payloads, which are validated and sent byte for byte. A 1006 close (no close handshake) is emitted as an `error` event instead of reaching `close` listeners with a validated reason. The client constructor also accepts `ws` options as its second argument.
