import type Ajv from 'ajv';
import type { AnySchema, ValidateFunction } from 'ajv';
import { OpenAPIObject } from 'openapi3-ts/oas31';

let nextDocumentId = 0;

/** Resolves response references against their document, without remote loading. */
export class ResponseSchemas {
  private documents = new WeakMap<OpenAPIObject, { ajv: Ajv; base: string }>();
  constructor(
    private readonly source: Ajv,
    private readonly factory?: () => Ajv
  ) {}

  validator(
    document: OpenAPIObject,
    pointer: string,
    schema: AnySchema
  ): ValidateFunction {
    const references = (value: unknown): boolean =>
      typeof value === 'object' &&
      value !== null &&
      ('$ref' in value ||
        '$id' in value ||
        Object.values(value).some(references));
    if (!references(schema)) {
      const validate = ((data: unknown) => {
        const valid = this.source.validate(schema, data);
        validate.errors = this.source.errors;
        return valid;
      }) as ValidateFunction;
      return validate;
    }
    let entry = this.documents.get(document);
    if (!entry) {
      // Separate registries may reuse the same absolute schema identifiers.
      // An instance per immutable registry document prevents cache cross-talk.
      // Direct constructor callers retain their complete supplied validator,
      // including custom keywords. Without a factory, conflicting explicit IDs
      // deliberately fail rather than replacing another document's schemas.
      const ajv = this.factory ? this.factory() : this.source;
      const base =
        (document as OpenAPIObject & { $id?: string }).$id ??
        `https://forklaunch.invalid/sdk-document/${++nextDocumentId}`;
      for (const schema of Object.values(document.components?.schemas ?? {})) {
        if (
          schema &&
          typeof schema === 'object' &&
          '$id' in schema &&
          typeof schema.$id === 'string'
        )
          ajv.addSchema(schema as AnySchema, new URL(schema.$id, base).href);
      }
      ajv.addSchema(document, base);
      entry = { ajv, base };
      this.documents.set(document, entry);
    }
    const validator = entry.ajv.getSchema(`${entry.base}#${pointer}`);
    if (!validator)
      throw new Error('Response schema not found in OpenAPI document');
    return validator;
  }
}

export function schemaPointer(
  path: string,
  method: string,
  status: number,
  contentType: string
): string {
  return [
    'paths',
    path,
    method.toLowerCase(),
    'responses',
    String(status),
    'content',
    contentType,
    'schema'
  ]
    .map((part) => `/${part.replace(/~/g, '~0').replace(/\//g, '~1')}`)
    .join('');
}
