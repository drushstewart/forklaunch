export type ForkLaunchQueryValue = string | number | boolean | null | readonly ForkLaunchQueryValue[] | { readonly [key: string]: ForkLaunchQueryValue } | undefined;
export type ForkLaunchPathValue = string | number | boolean;
export type ForkLaunchHeaders = Record<string, string>;
export type ForkLaunchSdkFetchInit = Omit<RequestInit, 'method' | 'body' | 'headers'>;

export type ForkLaunchSdkRequest<Params = never, Query = never, Body = never, Headers = ForkLaunchHeaders> =
  ForkLaunchSdkFetchInit &
  ([Params] extends [never] ? {} : { params: Params }) &
  ([Query] extends [never] ? {} : { query: Query }) &
  ([Body] extends [never] ? {} : { body: Body }) &
  ([Headers] extends [never] ? {} : { headers?: Headers });

export type ForkLaunchSdkResponse<Responses extends Record<number, unknown>, Headers = never, ContentTypes extends Partial<Record<number, string>> = {}> = {
  [Code in keyof Responses & number]: {
    code: Code;
    status: Code;
    contentType: Code extends keyof ContentTypes ? ContentTypes[Code] : Code extends keyof ForkLaunchErrorResponses ? 'text/plain' : 'application/json';
    response: Responses[Code];
  } & ([Headers] extends [never] ? {} : { headers: Headers });
}[keyof Responses & number];

export type ForkLaunchSdkConfig = {
  baseUrl?: string;
  serviceBaseUrls?: Partial<Record<string, string>>;
  fetch?: typeof fetch;
  headers?: ForkLaunchHeaders;
  beforeRequest?: (request: ForkLaunchSdkRequestMeta) => ForkLaunchSdkRequestMeta | Promise<ForkLaunchSdkRequestMeta>;
};

export type ForkLaunchSdkRequestMeta = {
  service: string;
  router: string;
  operation: string;
  method: string;
  path: string;
  requestKind?: 'json' | 'text' | 'binary' | 'urlEncoded' | 'multipart';
  requestContentType?: string;
  responseKinds?: Partial<Record<number, 'empty' | 'json' | 'text' | 'binary'>>;
  input: {
    params?: Record<string, ForkLaunchPathValue>;
    query?: Record<string, ForkLaunchQueryValue>;
    body?: unknown;
    headers?: ForkLaunchHeaders;
  } & ForkLaunchSdkFetchInit;
};

export type ForkLaunchErrorResponses = { 400: string; 401: string; 403: string; 404: string; 500: string };

{{{form_stringify_source}}}

{{#operation_types}}
export type {{request_type}} = ForkLaunchSdkRequest<{{{request_params_type}}}, {{{request_query_type}}}, {{{request_body_type}}}, {{{request_headers_type}}}>;
{{#responses}}
export type {{response_type}} = {{{response_body_type}}};
{{/responses}}
export type {{responses_type}}Declared = { {{#responses}}{{status_code}}: {{response_type}}; {{/responses}} };
export type {{responses_type}} = Omit<ForkLaunchErrorResponses, keyof {{responses_type}}Declared> & {{responses_type}}Declared;
export type {{responses_type}}ContentTypes = { {{#responses}}{{status_code}}: '{{content_type}}'; {{/responses}} };

{{/operation_types}}
function pathWithParams(path: string, params?: Record<string, ForkLaunchPathValue>) {
  if (!params) return path;
  return path.split('/').map(segment => {
    const name = segment.startsWith(':') ? segment.slice(1)
      : segment.startsWith('{') && segment.endsWith('}') ? segment.slice(1, -1) : undefined;
    return name !== undefined && Object.prototype.hasOwnProperty.call(params, name)
      ? encodeURIComponent(String(params[name])) : segment;
  }).join('/');
}

function encodeForm(body: unknown): URLSearchParams {
  if (body instanceof URLSearchParams) return body;
  if (body === null || typeof body !== 'object' || Array.isArray(body)) throw new TypeError('Form body must be an object or URLSearchParams');
  const params = new URLSearchParams();
  for (const [key, raw] of Object.entries(body)) {
    params.append(key, safeStringify(raw));
  }
  return params;
}

function encodeMultipart(body: unknown): FormData {
  if (body instanceof FormData) return body;
  if (body === null || typeof body !== 'object' || Array.isArray(body)) throw new TypeError('Multipart body must be an object or FormData');
  const form = new FormData();
  for (const [name, raw] of Object.entries(body)) {
    for (const value of Array.isArray(raw) ? raw : [raw]) {
      form.append(name, value instanceof Blob ? value : safeStringify(value));
    }
  }
  return form;
}

async function request<Responses extends Record<number, unknown>, ContentTypes extends Partial<Record<number, string>> = {}>(
  config: ForkLaunchSdkConfig,
  meta: ForkLaunchSdkRequestMeta
): Promise<ForkLaunchSdkResponse<Responses, never, ContentTypes>> {
  const fetcher = config.fetch ?? fetch;
  const resolved = config.beforeRequest ? await config.beforeRequest(meta) : meta;
  const { params, query: queryInput, body, headers, ...fetchInit } = resolved.input;
  const query = queryInput ? `?${new URLSearchParams(Object.entries(queryInput).filter(([, value]) => value !== undefined).map(([key, value]) => [key, typeof value === 'object' ? JSON.stringify(value) : String(value)]))}` : '';
  const path = pathWithParams(resolved.path, params);
  const baseUrl = config.serviceBaseUrls?.[resolved.service] ?? config.baseUrl ?? '';
  const requestHeaders = new Headers(config.headers);
  for (const [name, value] of Object.entries(headers ?? {})) requestHeaders.set(name, value);
  if (body !== undefined) requestHeaders.set('content-type', resolved.requestContentType ?? 'application/json');
  if (resolved.requestKind === 'multipart') {
    // The encoder must select the boundary, including after hooks/default headers.
    requestHeaders.delete('content-type');
  }
  const response = await fetcher(`${baseUrl}${path}${query}`, {
    ...fetchInit,
    method: resolved.method,
    headers: requestHeaders,
    body: body === undefined ? undefined : resolved.requestKind === 'multipart' ? encodeMultipart(body) : resolved.requestKind === 'binary' ? new Uint8Array(body as Uint8Array).buffer : resolved.requestKind === 'text' ? String(body) : resolved.requestKind === 'urlEncoded' ? encodeForm(body) : JSON.stringify(body)
  });
  const contentType = response.headers.get('content-type')?.split(';')[0].trim().toLowerCase() ?? (response.ok ? 'application/json' : 'text/plain');
  const kind = resolved.responseKinds?.[response.status];
  let parsedResponse: unknown;
  if (kind === 'binary') {
    parsedResponse = new Uint8Array(await response.arrayBuffer());
  } else {
    const text = await response.text();
    parsedResponse = kind === 'empty' ? undefined : kind === 'text' ? text : text
      ? kind === 'json' || contentType === 'application/json' || contentType.endsWith('+json') ? JSON.parse(text) : text
      : undefined;
  }
  return {
    code: response.status,
    status: response.status,
    contentType,
    response: parsedResponse
  } as ForkLaunchSdkResponse<Responses, never, ContentTypes>;
}

export function createForkLaunchSdk(config: ForkLaunchSdkConfig = {}) {
  return {
{{#services}}
    {{service_key}}: {
{{#routers}}
      {{router_key}}: {
{{#operations}}
        {{operation_key}}: (input: {{request_type}}{{#optional_input}} = {}{{/optional_input}}) => request<{{responses_type}}, {{responses_type}}ContentTypes>(config, { service: '{{service_name}}', router: '{{router_name}}', operation: '{{operation_name}}', method: '{{method}}', path: '{{path}}', requestKind: '{{request_kind}}', requestContentType: '{{request_content_type}}', responseKinds: { {{#responses}}{{status_code}}: '{{response_kind}}', {{/responses}} }, input }){{^last}},{{/last}}
{{/operations}}
      }{{^last}},{{/last}}
{{/routers}}
    }{{^last}},{{/last}}
{{/services}}
  };
}

export const sdk = createForkLaunchSdk();
export type ForkLaunchGeneratedSdk = ReturnType<typeof createForkLaunchSdk>;
{{#aliases}}
export const {{alias}} = sdk.{{service_key}};
{{/aliases}}
