import type {
  AsyncAPIObject,
  ChannelObject,
  ChannelsObject,
  MessageObject,
  MessagesObject,
  OperationObject,
  OperationsObject,
  ReferenceObject
} from '@asyncapi/parser/esm/spec-types/v3';

import type { AnySchemaValidator } from '@forklaunch/validator';

import { EventSchema, EventSchemaEntry } from '../types/eventSchema.types';

export type AsyncApiDocument = AsyncAPIObject;

function pointerSegment(value: string): string {
  return encodeURIComponent(
    value.replace(/~/g, '~0').replace(/\//g, '~1')
  ).replace(
    /[!'()*]/g,
    (character) => `%${character.charCodeAt(0).toString(16).toUpperCase()}`
  );
}

// AsyncAPI's default Schema Object uses object-form schemas. Keep idiomatic
// validator instances intact; normalize plain JSON Schema positions only.
function payloadSchema(schema: unknown, depth = 0): unknown {
  if (depth > 64) throw new Error('AsyncAPI schema nesting exceeds 64 levels');
  if (typeof schema === 'boolean') return schema ? {} : { not: {} };
  if (!schema || typeof schema !== 'object' || Array.isArray(schema))
    return schema;
  const prototype = Object.getPrototypeOf(schema);
  if (prototype !== null && prototype.constructor?.name !== 'Object')
    return schema;
  const output: Record<string, unknown> = { ...schema };
  for (const [key, value] of Object.entries(output)) {
    if (
      ['additionalProperties', 'additionalItems'].includes(key) &&
      typeof value === 'boolean'
    )
      continue;
    if (
      [
        'properties',
        'patternProperties',
        'dependentSchemas',
        'dependencies',
        '$defs',
        'definitions'
      ].includes(key) &&
      value &&
      typeof value === 'object' &&
      !Array.isArray(value)
    ) {
      output[key] = Object.fromEntries(
        Object.entries(value).map(([name, child]) => [
          name,
          payloadSchema(child, depth + 1)
        ])
      );
    } else if (
      ['allOf', 'anyOf', 'oneOf', 'prefixItems', 'items'].includes(key) &&
      Array.isArray(value)
    ) {
      output[key] = value.map((child) => payloadSchema(child, depth + 1));
    } else if (
      [
        'additionalProperties',
        'additionalItems',
        'unevaluatedProperties',
        'unevaluatedItems',
        'contentSchema',
        'contains',
        'not',
        'if',
        'then',
        'else',
        'propertyNames',
        'items'
      ].includes(key)
    ) {
      output[key] = payloadSchema(value, depth + 1);
    }
  }
  return output;
}

export function generateAsyncApi<SV extends AnySchemaValidator>(
  eventSchemas: EventSchema<SV>,
  options?: {
    title?: string;
    version?: string;
    description?: string;
    id?: AsyncApiDocument['id'];
    defaultContentType?: AsyncApiDocument['defaultContentType'];
  }
): AsyncApiDocument {
  const channels: ChannelsObject = Object.create(null);

  const appendMessageToChannel = (
    collection?: Record<string, EventSchemaEntry<SV>>
  ) => {
    if (!collection) {
      return;
    }

    Object.entries(collection).forEach(([messageKey, candidate]) => {
      if (
        !candidate ||
        typeof candidate !== 'object' ||
        Array.isArray(candidate) ||
        !('shape' in candidate)
      ) {
        return;
      }

      const entry = candidate as EventSchemaEntry<SV>;
      const schema = entry.shape;

      // Determine which channels to use
      const channelNames: string[] = entry.channel
        ? [entry.channel]
        : entry.channels && entry.channels.length > 0
          ? entry.channels
          : [messageKey];

      const messageName =
        entry.operation ?? entry.operations?.[0] ?? messageKey;

      // Add message to all specified channels
      channelNames.forEach((channelName) => {
        if (!channelName || /[?#]/.test(channelName)) {
          throw new Error(
            'AsyncAPI channel addresses must be nonempty and exclude query/fragment delimiters'
          );
        }
        const existingChannel = channels[channelName];
        if (!existingChannel || '$ref' in existingChannel) {
          channels[channelName] = {
            address: channelName,
            messages: Object.create(null)
          };
        }

        const channelEntry = channels[channelName] as ChannelObject;
        channelEntry.messages = (channelEntry.messages ??
          Object.create(null)) as MessagesObject;
        const messages = channelEntry.messages as MessagesObject;

        const message: MessageObject = {
          name: messageName,
          payload: payloadSchema(schema) as MessageObject['payload']
        };

        messages[messageName] = message;
      });
    });
  };

  appendMessageToChannel(eventSchemas.clientMessages);
  appendMessageToChannel(eventSchemas.serverMessages);
  appendMessageToChannel(eventSchemas.errors);

  const operations: OperationsObject = Object.create(null);

  const addOperationsForCollection = (
    collection: Record<string, EventSchemaEntry<SV>> | undefined,
    action: 'send' | 'receive'
  ) => {
    if (!collection) {
      return;
    }

    Object.entries(collection).forEach(([messageKey, candidate]) => {
      if (
        !candidate ||
        typeof candidate !== 'object' ||
        Array.isArray(candidate) ||
        !('shape' in candidate)
      ) {
        return;
      }

      const entry = candidate as EventSchemaEntry<SV>;

      // Determine which channels to use
      const channelNames: string[] = entry.channel
        ? [entry.channel]
        : entry.channels && entry.channels.length > 0
          ? entry.channels
          : [messageKey];

      const messageName =
        entry.operation ?? entry.operations?.[0] ?? messageKey;

      // Create operations for all specified channels
      channelNames.forEach((channelName) => {
        const operationKey = `${action}-${channelName}-${messageName}`;

        const operation: OperationObject = {
          action,
          channel: {
            $ref: `#/channels/${pointerSegment(channelName)}`
          } as ReferenceObject,
          messages: [
            {
              $ref: `#/channels/${pointerSegment(channelName)}/messages/${pointerSegment(messageName)}`
            } as ReferenceObject
          ]
        };

        operations[operationKey] = operation;
      });
    });
  };

  addOperationsForCollection(eventSchemas.clientMessages, 'receive');
  addOperationsForCollection(eventSchemas.serverMessages, 'send');

  const { title, version, description } = options ?? {};

  const asyncApiDocument: AsyncApiDocument = {
    asyncapi: '3.0.0',
    info: {
      title: title || process.env.API_TITLE || 'Forklaunch WebSocket API',
      version: version || '1.0.0',
      ...(description ? { description } : {})
    },
    ...(options?.id ? { id: options.id } : {}),
    ...(options?.defaultContentType
      ? { defaultContentType: options.defaultContentType }
      : {}),
    ...(Object.keys(channels).length > 0 ? { channels } : {}),
    ...(Object.keys(operations).length > 0 ? { operations } : {})
  };

  return asyncApiDocument;
}
