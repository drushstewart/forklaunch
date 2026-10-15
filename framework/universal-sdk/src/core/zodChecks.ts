import type Ajv from 'ajv';

// Reserved versioned capability; arbitrary JSON Schema keywords are unchanged.
const zodChecksKeyword = 'x-forklaunch-zod-checks';
interface ZodChecks {
  version: 1;
  utf16MinLength?: number;
  utf16MaxLength?: number;
  formats?: ('email' | 'uuid' | 'uri' | 'cuid' | 'cuid2' | 'ulid' | 'nanoid' | 'base64' | 'base64url')[];
  multipleOf?: (number | 'NaN' | 'Infinity' | '-Infinity')[];
}
function compileZodChecks(input: unknown): (value: unknown) => boolean {
  if (!input || typeof input !== 'object' || Array.isArray(input))
    throw new Error('Invalid Zod check profile');
  const value = input as Record<string, unknown>;
  if (
    value.version !== 1 ||
    Object.keys(value).some(
      (key) =>
        ![
          'version',
          'utf16MinLength',
          'utf16MaxLength',
          'multipleOf',
          'formats'
        ].includes(key)
    )
  )
    throw new Error('Invalid Zod check profile version or fields');
  for (const key of ['utf16MinLength', 'utf16MaxLength']) {
    if (
      Object.prototype.hasOwnProperty.call(value, key) &&
      (typeof value[key] !== 'number' ||
        !Number.isSafeInteger(value[key]) ||
        (value[key] as number) < 0)
    )
      throw new Error('Invalid Zod UTF-16 bound');
  }
  if (Object.prototype.hasOwnProperty.call(value, 'multipleOf')) {
    if (
      !Array.isArray(value.multipleOf) ||
      value.multipleOf.length > 10000 ||
      value.multipleOf.some(
        (divisor) =>
          !(typeof divisor === 'number' && Number.isFinite(divisor)) &&
          !['NaN', 'Infinity', '-Infinity'].includes(divisor)
      )
    )
      throw new Error('Invalid Zod multiple-of rules');
  }
  if (
    Object.prototype.hasOwnProperty.call(value, 'formats') &&
    (!Array.isArray(value.formats) ||
      value.formats.length > 10000 ||
      value.formats.some(
        (format) => !['email', 'uuid', 'uri', 'cuid', 'cuid2', 'ulid', 'nanoid', 'base64', 'base64url'].includes(format)
      ))
  )
    throw new Error('Invalid Zod format rules');
  if (
    (Array.isArray(value.formats) ? value.formats.length : 0) +
      (Array.isArray(value.multipleOf) ? value.multipleOf.length : 0) >
    10000
  )
    throw new Error('Zod check profile exceeds 10000 rules');
  const profile = value as unknown as ZodChecks;
  // Capture immutable admitted configuration; do not retain mutable user arrays.
  const minimum = profile.utf16MinLength,
    maximum = profile.utf16MaxLength;
  const divisors = profile.multipleOf?.map((divisor) =>
    typeof divisor === 'number' ? divisor : Number(divisor)
  );
  const formatChecks = profile.formats?.slice();
  const email =
    /^(?!\.)(?!.*\.\.)([A-Z0-9_'+\-\.]*)[A-Z0-9_+-]@([A-Z0-9][A-Z0-9\-]*\.)+[A-Z]{2,}$/i;
  const uuid =
    /^[0-9a-fA-F]{8}\b-[0-9a-fA-F]{4}\b-[0-9a-fA-F]{4}\b-[0-9a-fA-F]{4}\b-[0-9a-fA-F]{12}$/i;
  const simpleFormats = {
    cuid: /^c[^\s-]{8,}$/i,
    cuid2: /^[0-9a-z]+$/,
    ulid: /^[0-9A-HJKMNP-TV-Z]{26}$/i,
    nanoid: /^[a-z0-9_-]{21}$/i,
    base64: /^([0-9a-zA-Z+/]{4})*(([0-9a-zA-Z+/]{2}==)|([0-9a-zA-Z+/]{3}=))?$/,
    base64url: /^([0-9a-zA-Z-_]{4})*(([0-9a-zA-Z-_]{2}(==)?)|([0-9a-zA-Z-_]{3}(=)?))?$/
  };
  return (data: unknown) => {
    if (typeof data === 'string' && formatChecks)
      for (const format of formatChecks) {
        if (format in simpleFormats && !simpleFormats[format as keyof typeof simpleFormats].test(data)) return false;
        if (format === 'email' && !email.test(data)) return false;
        if (format === 'uuid' && !uuid.test(data)) return false;
        if (format === 'uri') {
          try {
            new URL(data);
          } catch {
            return false;
          }
        }
      }

    if (
      typeof data === 'string' &&
      ((minimum !== undefined && data.length < minimum) ||
        (maximum !== undefined && data.length > maximum))
    )
      return false;
    if (typeof data === 'number' && divisors) {
      for (const divisor of divisors) {
        const decimals = Math.max(
          (data.toString().split('.')[1] || '').length,
          (divisor.toString().split('.')[1] || '').length
        );
        const numerator = parseInt(data.toFixed(decimals).replace('.', ''));
        const denominator = parseInt(
          divisor.toFixed(decimals).replace('.', '')
        );
        if ((numerator % denominator) / Math.pow(10, decimals) !== 0)
          return false;
      }
    }
    return true;
  };
}

/** Add the reserved capability without replacing the supplied engine/options.
 * Reusing an engine with this module instance is supported. Separately bundled
 * consumers must use separate engines; their handler identities differ. A caller-installed handler under the reserved
 * name is rejected rather than silently trusting different validation semantics.
 */
export function registerZodChecks(ajv: Ajv): Ajv {
  const existing = ajv.getKeyword(zodChecksKeyword);
  if (existing) {
    if (
      typeof existing === 'object' &&
      'compile' in existing &&
      existing.compile === compileZodChecks
    )
      return ajv;
    throw new Error('Reserved Zod check keyword is already registered');
  }
  ajv.addKeyword({
    keyword: zodChecksKeyword,
    schemaType: 'object',
    compile: compileZodChecks,
    errors: false
  });
  return ajv;
}
