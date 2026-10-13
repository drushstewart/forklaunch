// Node WebSocket runtime emitted with raw-schema clients.
import { WebSocket, type ClientOptions, type RawData } from 'ws';
import { Ajv, type AnySchema, type ValidateFunction } from 'ajv';
import { Ajv2020 } from 'ajv/dist/2020.js';
import { Ajv2019 } from 'ajv/dist/2019.js';
import AjvDraft4Module from 'ajv-draft-04';
import addFormatsModule, { type FormatsPlugin } from 'ajv-formats';
import type { IncomingHttpHeaders } from 'node:http';

export interface SocketClientOptions {
  headers?: ClientOptions['headers'];
  protocols?: string | string[];
  webSocketOptions?: Omit<
    ClientOptions,
    'headers' | 'maxPayload' | 'autoPong' | 'handshakeTimeout'
  >;
  handshakeTimeoutMs?: number;
  writeTimeoutMs?: number;
  closeTimeoutMs?: number;
  maxMessageBytes?: number;
  maxQueuedEvents?: number;
  maxQueuedBytes?: number;
  signal?: AbortSignal;
}
export interface SocketReadOptions {
  signal?: AbortSignal;
  timeoutMs?: number;
}
export interface SocketWriteOptions {
  signal?: AbortSignal;
}
export interface SocketUpgradeResponse {
  readonly statusCode: number;
  readonly headers: IncomingHttpHeaders;
  readonly protocol: string;
}
export type SocketEvent<I, P, Q, C> =
  | { type: 'message'; value: I }
  | { type: 'ping'; value: P }
  | { type: 'pong'; value: Q }
  | { type: 'close'; code: number; reason: C };
type SocketSchemaSet = Record<
  'incoming' | 'outgoing' | 'ping' | 'pong' | 'close',
  AnySchema
>;
type SocketWaiter<T> = {
  resolve(value: T | null): void;
  reject(error: Error): void;
  cleanup(): void;
};

function socketInteger(
  value: number | undefined,
  fallback: number,
  label: string,
): number {
  const result = value ?? fallback;
  if (!Number.isSafeInteger(result) || result <= 0)
    throw new Error(`Invalid ${label}`);
  return result;
}
function socketAbort(): Error {
  const error = new Error('WebSocket operation aborted');
  error.name = 'AbortError';
  return error;
}
function socketTimeout(
  value: number | undefined,
  fallback: number,
  label: string,
): number {
  const result = socketInteger(value, fallback, label);
  if (result > 2_147_483_647)
    throw new Error(`Invalid ${label}: exceeds Node timer range`);
  return result;
}
function socketError(error: unknown): Error {
  return error instanceof Error ? error : new Error(String(error));
}
function socketDecimal(value: number): [bigint, number] {
  const [coefficient, exponent = '0'] = String(value).toLowerCase().split('e');
  const [integer, fraction = ''] = coefficient.split('.');
  return [BigInt(integer + fraction), Number(exponent) - fraction.length];
}
function socketMultipleOf(value: number, divisor: number): boolean {
  let [numerator, scale] = socketDecimal(value);
  let [denominator, divisorScale] = socketDecimal(divisor);
  scale -= divisorScale;
  if (scale >= 0) numerator *= 10n ** BigInt(scale);
  else denominator *= 10n ** BigInt(-scale);
  return numerator % denominator === 0n;
}
function socketValidator(schema: AnySchema): ValidateFunction {
  const dialect = typeof schema === 'object' ? schema.$schema : undefined;
  // CommonJS constructor declarations are exposed as a namespace in NodeNext.
  const AjvDraft4 = AjvDraft4Module as unknown as typeof Ajv;
  const engine =
    typeof dialect === 'string' && dialect.includes('/draft-04/')
      ? new AjvDraft4({
          strict: false,
          validateFormats: true,
          ownProperties: true,
        })
      : typeof dialect === 'string' && dialect.includes('2020-12')
        ? new Ajv2020({
            strict: false,
            validateFormats: true,
            ownProperties: true,
          })
        : typeof dialect === 'string' && dialect.includes('2019-09')
          ? new Ajv2019({
              strict: false,
              validateFormats: true,
              ownProperties: true,
            })
          : new Ajv({
              strict: false,
              validateFormats: true,
              ownProperties: true,
            });
  if (typeof dialect === 'string' && dialect.includes('/draft-06/'))
    engine.addMetaSchema(socketDraft6MetaSchema);
  if (typeof dialect === 'string' && dialect.includes('/draft-04/')) {
    // The Draft 4 adapter also registers newer AJV applicator/const keywords.
    // They are annotations in this dialect and must not narrow accepted values.
    for (const keyword of [
      'const',
      'contains',
      'propertyNames',
      'if',
      'then',
      'else',
    ])
      engine.removeKeyword(keyword);
  }
  if (typeof dialect === 'string' && dialect.includes('/draft-06/'))
    for (const keyword of ['if', 'then', 'else']) engine.removeKeyword(keyword);
  // CJS package declarations are exposed as a namespace under NodeNext ESM.
  const addFormats = addFormatsModule as unknown as FormatsPlugin;
  addFormats(engine);
  engine.removeKeyword('multipleOf');
  engine.addKeyword({
    keyword: 'multipleOf',
    type: 'number',
    schemaType: 'number',
    compile: (divisor: number) => (value: number) =>
      socketMultipleOf(value, divisor),
  });
  // No loadSchema callback: external resources are never fetched.
  return engine.compile(socketAjvSchema(schema));
}
// AJV excludes properties named __proto__ from ordinary property compilation.
// Keep their original pointer targets and also validate them as exact patterns.
function socketAjvSchema(schema: AnySchema): AnySchema {
  const copy = JSON.parse(JSON.stringify(schema)) as AnySchema;
  const dictionaries = new Set([
    'properties',
    'patternProperties',
    '$defs',
    'definitions',
    'dependentSchemas',
    'dependencies',
  ]);
  const singles = new Set([
    'additionalProperties',
    'additionalItems',
    'unevaluatedProperties',
    'unevaluatedItems',
    'contains',
    'not',
    'if',
    'then',
    'else',
    'propertyNames',
    'contentSchema',
  ]);
  const dialect = typeof copy === 'object' ? copy.$schema : undefined;
  const draft4 = typeof dialect === 'string' && dialect.includes('/draft-04/');
  const draft6 = typeof dialect === 'string' && dialect.includes('/draft-06/');
  if (draft4 || draft6) {
    const ignored = new Set([
      'if',
      'then',
      'else',
      'dependentSchemas',
      'prefixItems',
      'unevaluatedProperties',
      'unevaluatedItems',
      'contentSchema',
      ...(draft4 ? ['const', 'contains', 'propertyNames'] : []),
    ]);
    const active = new WeakSet<object>();
    const pending: unknown[] = [copy];
    const annotations: unknown[] = [];
    // AJV discovers resource IDs before consulting its dialect vocabulary.
    // Identify real schema positions, including explicit local references into
    // ignored annotation storage, before cleaning its private compilation copy.
    while (pending.length) {
      const node = pending.pop();
      if (
        !node ||
        typeof node !== 'object' ||
        Array.isArray(node) ||
        active.has(node)
      )
        continue;
      active.add(node);
      const object = node as Record<string, unknown>;
      if (typeof object.$ref === 'string' && object.$ref.startsWith('#')) {
        const pointer = decodeURIComponent(object.$ref.slice(1));
        let target: unknown = copy;
        if (pointer.startsWith('/')) {
          for (const segment of pointer.slice(1).split('/')) {
            const key = segment.replace(/~1/g, '/').replace(/~0/g, '~');
            target =
              target &&
              typeof target === 'object' &&
              Object.prototype.hasOwnProperty.call(target, key)
                ? (target as Record<string, unknown>)[key]
                : undefined;
          }
        } else if (pointer !== '') target = undefined;
        pending.push(target);
      }
      for (const [key, child] of Object.entries(object)) {
        if (ignored.has(key)) annotations.push(child);
        else if (
          dictionaries.has(key) &&
          child &&
          typeof child === 'object' &&
          !Array.isArray(child)
        ) {
          for (const value of Object.values(child)) pending.push(value);
        } else if (singles.has(key)) pending.push(child);
        else if (
          ['items', 'prefixItems', 'allOf', 'anyOf', 'oneOf'].includes(key)
        ) {
          if (Array.isArray(child)) {
            for (const value of child) pending.push(value);
          } else pending.push(child);
        }
      }
    }
    function cleanAnnotation(value: unknown): void {
      if (!value || typeof value !== 'object' || active.has(value)) return;
      if (Array.isArray(value)) {
        value.forEach(cleanAnnotation);
        return;
      }
      const object = value as Record<string, unknown>;
      for (const keyword of ['$id', 'id', '$anchor', '$dynamicAnchor']) {
        if (typeof object[keyword] === 'string') delete object[keyword];
      }
      Object.values(object).forEach(cleanAnnotation);
    }
    annotations.forEach(cleanAnnotation);
  }
  function visit(node: unknown, location: string[]): void {
    if (!node || typeof node !== 'object' || Array.isArray(node)) return;
    const object = node as Record<string, unknown>;
    if (typeof object.$schema === 'string') {
      if (object.$schema.includes('/draft-04/'))
        object.$schema = 'http://json-schema.org/draft-04/schema#';
      else if (object.$schema.includes('/draft-06/'))
        object.$schema = 'http://json-schema.org/draft-06/schema#';
      else if (object.$schema.includes('/draft-07/'))
        object.$schema = 'http://json-schema.org/draft-07/schema#';
      else if (object.$schema.includes('/draft/2019-09/'))
        object.$schema = 'https://json-schema.org/draft/2019-09/schema';
      else if (object.$schema.includes('/draft/2020-12/'))
        object.$schema = 'https://json-schema.org/draft/2020-12/schema';
    }
    const base = typeof object.$id === 'string' ? [] : location;
    const properties = object.properties as Record<string, unknown> | undefined;
    if (
      properties &&
      Object.prototype.hasOwnProperty.call(properties, '__proto__')
    ) {
      const patterns = (object.patternProperties ??=
        Object.create(null)) as Record<string, unknown>;
      let pattern = '^__proto__(?![\\s\\S])';
      // Existing patterns may be $ref targets; retain their original meaning.
      while (Object.prototype.hasOwnProperty.call(patterns, pattern))
        pattern += '(?:)';
      const pointer = [...base, 'properties', '__proto__']
        .map((token) =>
          encodeURIComponent(token.replace(/~/g, '~0').replace(/\//g, '~1')),
        )
        .join('/');
      const reference = { $ref: `#/${pointer}` };
      patterns[pattern] = reference;
    }
    for (const [key, child] of Object.entries(object)) {
      if (
        dictionaries.has(key) &&
        child &&
        typeof child === 'object' &&
        !Array.isArray(child)
      ) {
        for (const [name, value] of Object.entries(child))
          visit(value, [...base, key, name]);
      } else if (singles.has(key)) visit(child, [...base, key]);
      else if (
        ['items', 'prefixItems', 'allOf', 'anyOf', 'oneOf'].includes(key)
      ) {
        if (Array.isArray(child))
          child.forEach((value, index) =>
            visit(value, [...base, key, String(index)]),
          );
        else visit(child, [...base, key]);
      }
    }
  }
  visit(copy, []);
  return copy;
}
function socketJsonBounds(value: unknown, byteLimit: number): void {
  const pending: Array<[unknown, number, boolean?]> = [[value, 0]];
  const seen = new Set<object>();
  // A valid bounded wire document cannot contain more nodes than its bytes.
  let nodes = byteLimit + 1;
  while (pending.length) {
    const [current, depth, leaving] = pending.pop()!;
    if (leaving) {
      seen.delete(current as object);
      continue;
    }
    if (--nodes < 0 || depth >= 128)
      throw new Error('WebSocket JSON representation limit');
    if (typeof current === 'number' && !Number.isFinite(current))
      throw new Error('Non-finite WebSocket JSON number');
    if (typeof current === 'string') {
      if (Buffer.byteLength(current) > byteLimit)
        throw new Error('WebSocket payload byte limit');
      for (let index = 0; index < current.length; index++) {
        const code = current.charCodeAt(index);
        if (code >= 0xd800 && code <= 0xdbff) {
          const next = current.charCodeAt(++index);
          if (!(next >= 0xdc00 && next <= 0xdfff))
            throw new Error('Unpaired WebSocket JSON surrogate');
        } else if (code >= 0xdc00 && code <= 0xdfff)
          throw new Error('Unpaired WebSocket JSON surrogate');
      }
    }
    if (current && typeof current === 'object') {
      if (seen.has(current)) throw new Error('Circular WebSocket JSON object');
      seen.add(current);
      pending.push([current, depth, true]);
      const values = Array.isArray(current) ? current : Object.values(current);
      if (values.length > nodes - pending.length)
        throw new Error('WebSocket JSON node limit');
      for (const child of values) pending.push([child, depth + 1]);
    }
  }
}
function socketBytes(data: RawData): Buffer {
  return Buffer.isBuffer(data)
    ? data
    : Array.isArray(data)
      ? Buffer.concat(data)
      : Buffer.from(data);
}

class SocketCodec<T> {
  private readonly validate?: ValidateFunction;
  constructor(
    schema: AnySchema,
    private readonly limit: number,
    private readonly binary = false,
    private readonly jsonEncoding = false,
  ) {
    if (!binary) this.validate = socketValidator(schema);
  }
  private check(value: unknown): T {
    socketJsonBounds(value, this.limit);
    if (!this.validate?.(value))
      throw new Error(
        `Invalid WebSocket payload: ${JSON.stringify(this.validate?.errors)}`,
      );
    return value as T;
  }
  decode(bytes: Buffer): T {
    if (bytes.length > this.limit)
      throw new Error('WebSocket payload byte limit');
    if (this.binary) return Uint8Array.from(bytes) as T;
    const text = this.jsonEncoding ? new TextDecoder('utf-8', {fatal:true, ignoreBOM:true}).decode(bytes) : bytes.toString('utf8');
    let value: unknown;
    try {
      value = JSON.parse(text);
    } catch {
      if (this.jsonEncoding) throw new Error('Invalid JSON WebSocket payload');
      value = text;
    }
    return this.check(value);
  }
  encode(value: T): Buffer {
    if (this.binary) {
      if (!(value instanceof Uint8Array)) throw new Error('Expected binary WebSocket Uint8Array');
      if (value.byteLength > this.limit) throw new Error('WebSocket payload byte limit');
      return Buffer.from(value);
    }
    // Validate the serialized JSON projection, matching serde's outgoing values.
    socketJsonBounds(value, this.limit);
    const json = JSON.stringify(value);
    if (json === undefined || (!this.jsonEncoding && value === null))
      throw new Error('Null/undefined WebSocket messages are unsupported');
    if (Buffer.byteLength(json) > this.limit * 6 + 2)
      throw new Error('WebSocket payload byte limit');
    const projected: unknown = JSON.parse(json);
    this.check(projected);
    const bytes = Buffer.from(!this.jsonEncoding && typeof projected === 'string' ? projected : json);
    if (bytes.length > this.limit)
      throw new Error('WebSocket payload byte limit');
    return bytes;
  }
}

class SocketRuntime<I, O, P, Q, C> implements AsyncIterable<
  SocketEvent<I, P, Q, C>
> {
  private readonly ws: WebSocket;
  private readonly incoming: SocketCodec<I>;
  private readonly outgoing: SocketCodec<O>;
  private readonly pingCodec: SocketCodec<P>;
  private readonly pongCodec: SocketCodec<Q>;
  private readonly closeCodec: SocketCodec<C>;
  private readonly options: Required<
    Pick<
      SocketClientOptions,
      | 'writeTimeoutMs'
      | 'closeTimeoutMs'
      | 'maxMessageBytes'
      | 'maxQueuedEvents'
      | 'maxQueuedBytes'
    >
  >;
  private readonly queue: Array<{
    event: SocketEvent<I, P, Q, C>;
    bytes: number;
  }> = [];
  private queuedBytes = 0;
  private waiter?: SocketWaiter<SocketEvent<I, P, Q, C>>;
  private failure?: Error;
  private failureDelivered = false;
  private ended = false;
  private disposed = false;
  private writing = false;
  private closeTimer?: ReturnType<typeof setTimeout>;
  private readonly controlWrites = new Set<() => void>();
  private upgrade?: SocketUpgradeResponse;
  private readonly opened: Promise<void>;

  protected constructor(
    url: string,
    schemas: SocketSchemaSet,
    options: SocketClientOptions,
    binary: Partial<Record<keyof SocketSchemaSet, boolean>> = {},
    jsonEncoding: Partial<Record<keyof SocketSchemaSet, boolean>> = {},
  ) {
    const handshakeTimeoutMs = socketTimeout(
      options.handshakeTimeoutMs,
      10_000,
      'handshake timeout',
    );
    this.options = {
      writeTimeoutMs: socketTimeout(
        options.writeTimeoutMs,
        5_000,
        'write timeout',
      ),
      closeTimeoutMs: socketTimeout(
        options.closeTimeoutMs,
        5_000,
        'close timeout',
      ),
      maxMessageBytes: socketInteger(
        options.maxMessageBytes,
        1024 * 1024,
        'message byte limit',
      ),
      maxQueuedEvents: socketInteger(
        options.maxQueuedEvents,
        128,
        'queue event limit',
      ),
      maxQueuedBytes: socketInteger(
        options.maxQueuedBytes,
        4 * 1024 * 1024,
        'queue byte limit',
      ),
    };
    this.incoming = new SocketCodec(
      schemas.incoming,
      this.options.maxMessageBytes,
      binary.incoming,
      jsonEncoding.incoming,
    );
    this.outgoing = new SocketCodec(
      schemas.outgoing,
      this.options.maxMessageBytes,
      binary.outgoing,
      jsonEncoding.outgoing,
    );
    this.pingCodec = new SocketCodec(
      schemas.ping,
      Math.min(125, this.options.maxMessageBytes),
      binary.ping,
      jsonEncoding.ping,
    );
    this.pongCodec = new SocketCodec(
      schemas.pong,
      Math.min(125, this.options.maxMessageBytes),
      binary.pong,
      jsonEncoding.pong,
    );
    this.closeCodec = new SocketCodec(
      schemas.close,
      Math.min(123, this.options.maxMessageBytes),
      binary.close,
      jsonEncoding.close,
    );
    if (options.signal?.aborted) throw socketAbort();
    this.ws = new WebSocket(url, options.protocols ?? [], {
      ...options.webSocketOptions,
      headers: options.headers,
      maxPayload: this.options.maxMessageBytes,
      autoPong: false,
      handshakeTimeout: handshakeTimeoutMs,
    });
    this.opened = new Promise((resolve, reject) => {
      const abort = () => {
        const error = socketAbort();
        this.fail(error);
        reject(error);
      };
      const cleanup = () => {
        options.signal?.removeEventListener('abort', abort);
        this.ws.off('open', open);
        this.ws.off('error', error);
        this.ws.off('close', close);
      };
      const open = () => {
        cleanup();
        resolve();
      };
      const error = (cause: Error) => {
        cleanup();
        reject(cause);
      };
      const close = () => {
        cleanup();
        reject(new Error('WebSocket closed before upgrade'));
      };
      this.ws.once('open', open);
      this.ws.once('error', error);
      this.ws.once('close', close);
      options.signal?.addEventListener('abort', abort, { once: true });
    });
    this.opened.catch(() => {});
    this.ws.on('upgrade', (response) => {
      this.upgrade = {
        statusCode: response.statusCode ?? 101,
        headers: { ...response.headers },
        protocol: '',
      };
    });
    this.ws.on('open', () => {
      if (this.upgrade)
        this.upgrade = { ...this.upgrade, protocol: this.ws.protocol };
    });
    this.ws.on('error', (error) => this.fail(error));
    this.ws.on('message', (data) =>
      this.receive(
        () => ({
          type: 'message',
          value: this.incoming.decode(socketBytes(data)),
        }),
        socketBytes(data).length,
      ),
    );
    this.ws.on('pong', (bytes) =>
      this.receive(
        () => ({ type: 'pong', value: this.pongCodec.decode(bytes) }),
        bytes.length,
      ),
    );
    this.ws.on('ping', (bytes) =>
      this.receive(() => {
        const value = this.pingCodec.decode(bytes);
        this.pongCodec.decode(bytes); // Validate the exact automatic reply, without re-encoding.
        this.automaticPong(bytes);
        return { type: 'ping', value };
      }, bytes.length),
    );
    this.ws.on('close', (code, bytes) => {
      clearTimeout(this.closeTimer);
      this.clearControlWrites();
      if (code === 1006 && !this.disposed && !this.failure)
        this.fail(new Error('WebSocket closed without a handshake'));
      if (!this.disposed && !this.failure)
        this.receive(
          () => {
            if (
              bytes.length + (code === 1005 ? 0 : 2) >
              this.options.maxMessageBytes
            )
              throw new Error('WebSocket close frame byte limit');
            return {
              type: 'close',
              code,
              reason: this.closeCodec.decode(bytes),
            };
          },
          bytes.length + (code === 1005 ? 0 : 2),
        );
      this.ended = true;
      const waiter = this.takeWaiter();
      if (waiter)
        this.failure ? waiter.reject(this.failure) : waiter.resolve(null);
    });
  }

  protected async waitForOpen(): Promise<void> {
    await this.opened;
    if (this.failure) throw this.failure;
  }
  get response(): SocketUpgradeResponse {
    if (!this.upgrade)
      throw new Error('WebSocket upgrade response is unavailable');
    return this.upgrade;
  }
  get protocol(): string {
    return this.ws.protocol;
  }
  private takeWaiter(): SocketWaiter<SocketEvent<I, P, Q, C>> | undefined {
    const waiter = this.waiter;
    this.waiter = undefined;
    waiter?.cleanup();
    return waiter;
  }
  private fail(error: unknown, policy = false): void {
    if (this.disposed || this.failure) return;
    this.failure = socketError(error);
    this.ended = true;
    clearTimeout(this.closeTimer);
    this.clearControlWrites();
    const waiter = this.takeWaiter();
    if (waiter) {
      this.failureDelivered = true;
      waiter.reject(this.failure);
    }
    if (policy && this.ws.readyState === WebSocket.OPEN) {
      this.ws.close(1008, 'invalid payload');
      this.closeTimer = setTimeout(
        () => this.ws.terminate(),
        this.options.closeTimeoutMs,
      );
    } else this.ws.terminate();
  }
  private receive(make: () => SocketEvent<I, P, Q, C>, bytes: number): void {
    if (this.failure || this.disposed) return;
    try {
      const event = make();
      if (this.failure || this.disposed) return;
      const waiter = this.takeWaiter();
      if (waiter) {
        waiter.resolve(event);
        return;
      }
      if (
        this.queue.length >= this.options.maxQueuedEvents ||
        this.queuedBytes + bytes > this.options.maxQueuedBytes
      )
        throw new Error('WebSocket receive queue limit');
      this.queue.push({ event, bytes });
      this.queuedBytes += bytes;
    } catch (error) {
      this.fail(error, true);
    }
  }
  private clearControlWrites(): void {
    for (const cleanup of this.controlWrites) cleanup();
  }
  private automaticPong(bytes: Buffer): void {
    if (
      this.controlWrites.size >= this.options.maxQueuedEvents ||
      this.ws.bufferedAmount + bytes.length + 14 >
        this.options.maxMessageBytes + 128
    )
      throw new Error('WebSocket automatic reply buffer limit');
    let settled = false;
    const cleanup = () => {
      if (settled) return;
      settled = true;
      clearTimeout(timer);
      this.controlWrites.delete(cleanup);
    };
    const timer = setTimeout(() => {
      cleanup();
      this.fail(new Error('WebSocket pong write timed out'));
    }, this.options.writeTimeoutMs);
    this.controlWrites.add(cleanup);
    try {
      this.ws.pong(bytes, undefined, (error) => {
        if (settled) return;
        cleanup();
        if (error) this.fail(error);
      });
    } catch (error) {
      cleanup();
      throw error;
    }
  }
  async recv(
    options: SocketReadOptions = {},
  ): Promise<SocketEvent<I, P, Q, C> | null> {
    if (options.signal?.aborted) throw socketAbort();
    const timeout =
      options.timeoutMs === undefined
        ? undefined
        : socketTimeout(options.timeoutMs, 1, 'read timeout');
    if (this.waiter)
      throw new Error('Only one WebSocket receive may be pending');
    const queued = this.queue.shift();
    if (queued) {
      this.queuedBytes -= queued.bytes;
      return queued.event;
    }
    if (this.failure && !this.failureDelivered) {
      this.failureDelivered = true;
      throw this.failure;
    }
    if (this.ended) return null;
    return new Promise((resolve, reject) => {
      let timer: ReturnType<typeof setTimeout> | undefined;
      const abort = () => {
        this.takeWaiter()?.reject(socketAbort());
      };
      const cleanup = () => {
        clearTimeout(timer);
        options.signal?.removeEventListener('abort', abort);
      };
      this.waiter = { resolve, reject, cleanup };
      if (timeout !== undefined)
        timer = setTimeout(() => {
          this.takeWaiter()?.reject(new Error('WebSocket read timed out'));
        }, timeout);
      options.signal?.addEventListener('abort', abort, { once: true });
    });
  }
  private async write(
    kind: 'message' | 'ping' | 'pong',
    bytes: Buffer,
    options: SocketWriteOptions,
  ): Promise<void> {
    if (options.signal?.aborted) throw socketAbort();
    if (this.failure) throw this.failure;
    if (this.ended || this.ws.readyState !== WebSocket.OPEN)
      throw new Error('WebSocket is closed');
    if (this.writing)
      throw new Error('Only one WebSocket write may be pending');
    if (
      this.ws.bufferedAmount + bytes.length + 14 >
      this.options.maxMessageBytes + 128
    )
      throw new Error('WebSocket write buffer limit');
    this.writing = true;
    try {
      await new Promise<void>((resolve, reject) => {
        let settled = false;
        const finish = (error?: Error) => {
          if (settled) return;
          settled = true;
          clearTimeout(timer);
          options.signal?.removeEventListener('abort', abort);
          if (error) {
            this.fail(error);
            this.failureDelivered = true;
            reject(error);
          } else resolve();
        };
        const abort = () => finish(socketAbort());
        const timer = setTimeout(
          () => finish(new Error('WebSocket write timed out')),
          this.options.writeTimeoutMs,
        );
        options.signal?.addEventListener('abort', abort, { once: true });
        try {
          if (kind === 'message') this.ws.send(bytes, finish);
          else if (kind === 'ping') this.ws.ping(bytes, undefined, finish);
          else this.ws.pong(bytes, undefined, finish);
        } catch (error) {
          finish(socketError(error));
        }
      });
    } finally {
      this.writing = false;
    }
  }
  async send(value: O, options: SocketWriteOptions = {}): Promise<void> {
    await this.write('message', this.outgoing.encode(value), options);
  }
  async ping(value?: P, options: SocketWriteOptions = {}): Promise<void> {
    await this.write(
      'ping',
      value === undefined ? Buffer.alloc(0) : this.pingCodec.encode(value),
      options,
    );
  }
  async pong(value?: Q, options: SocketWriteOptions = {}): Promise<void> {
    await this.write(
      'pong',
      value === undefined ? Buffer.alloc(0) : this.pongCodec.encode(value),
      options,
    );
  }
  close(code = 1000, reason?: C): void {
    if (
      !Number.isInteger(code) ||
      !(
        (code >= 1000 && code <= 1014 && ![1004, 1005, 1006].includes(code)) ||
        (code >= 3000 && code <= 4999)
      )
    )
      throw new Error('Reserved WebSocket close code');
    if (this.failure) throw this.failure;
    if (this.ended || this.ws.readyState !== WebSocket.OPEN)
      throw new Error('WebSocket is closed');
    if (this.writing)
      throw new Error('Only one WebSocket write may be pending');
    const bytes =
      reason === undefined ? undefined : this.closeCodec.encode(reason);
    if ((bytes?.length ?? 0) + 2 > this.options.maxMessageBytes)
      throw new Error('WebSocket close frame byte limit');
    this.closeTimer = setTimeout(
      () => this.fail(new Error('WebSocket close timed out')),
      this.options.closeTimeoutMs,
    );
    try {
      this.ws.close(code, bytes);
    } catch (error) {
      clearTimeout(this.closeTimer);
      throw error;
    }
  }
  terminate(): void {
    if (this.disposed) return;
    this.disposed = true;
    this.failureDelivered = true;
    this.ended = true;
    this.queue.length = 0;
    this.queuedBytes = 0;
    clearTimeout(this.closeTimer);
    this.clearControlWrites();
    this.takeWaiter()?.resolve(null);
    this.ws.terminate();
  }
  [Symbol.asyncIterator](): AsyncIterator<SocketEvent<I, P, Q, C>> {
    return {
      next: async () => {
        const value = await this.recv();
        return value === null
          ? { done: true, value: undefined }
          : { done: false, value };
      },
      return: async () => {
        this.terminate();
        return { done: true, value: undefined };
      },
    };
  }
}
