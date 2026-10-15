import { z, type ZodTypeAny } from 'zod/v3';

// Explicit supported nodes prevent the general exporter's catch-all fallback from
// silently admitting JSON for non-JSON or unimplemented Zod types.
const supported = new Set([
  'ZodObject',
  'ZodRecord',
  'ZodString',
  'ZodNumber',
  'ZodBoolean',
  'ZodDate',
  'ZodNull',
  'ZodOptional',
  'ZodNullable',
  'ZodDefault',
  'ZodArray',
  'ZodLiteral',
  'ZodEnum',
  'ZodNativeEnum',
  'ZodIntersection',
  'ZodUnion',
  'ZodDiscriminatedUnion',
  'ZodNever',
  'ZodBranded',
  'ZodUndefined',
  'ZodTuple',
  'ZodAny',
  'ZodUnknown',
  'ZodVoid',
  'ZodReadonly'
]);

/** Ordinary constructed schemas only; arbitrary getters, Proxies and shape thunks
 * are not sandboxed. Date retains the existing declared wire representation.
 */
export function assertPureInputSchema(schema: ZodTypeAny): void {
  if (schema._def.typeName === 'ZodLiteral') {
    const value = schema._def.value;
    if (
      !(
        value === null ||
        value === undefined ||
        typeof value === 'string' ||
        typeof value === 'boolean' ||
        (typeof value === 'number' && Number.isFinite(value))
      )
    ) {
      throw new Error('Pure input export does not support non-JSON literal');
    }
  }
  if (
    schema._def.typeName === 'ZodNativeEnum' &&
    !z.util
      .getValidEnumValues(schema._def.values)
      .every(
        (value) =>
          typeof value === 'string' ||
          (typeof value === 'number' && Number.isFinite(value))
      )
  ) {
    throw new Error(
      'Pure input export does not support non-JSON native enum values'
    );
  }
  if (schema._def.coerce === true) {
    throw new Error('Pure input export does not support built-in coercion');
  }
  if (!supported.has(schema._def.typeName)) {
    throw new Error(
      `Pure input export does not support ${schema._def.typeName}`
    );
  }
}

/** Structural omission only; computed defaults and user predicates are never evaluated. */
export function pureInputOptional(
  schema: ZodTypeAny,
  charge: (depth: number) => void,
  depth: number,
  active = new Set<ZodTypeAny>()
): boolean {
  charge(depth);
  assertPureInputSchema(schema);
  if (active.has(schema))
    throw new Error('Pure input schema cycle is unsupported');
  active.add(schema);
  try {
    const def = schema._def as {
      typeName: string;
      innerType?: ZodTypeAny;
      type?: ZodTypeAny;
      options?: ZodTypeAny[];
      left?: ZodTypeAny;
      right?: ZodTypeAny;
    };
    switch (def.typeName) {
      case 'ZodLiteral':
        return schema._def.value === undefined;
      case 'ZodOptional':
      case 'ZodDefault':
      case 'ZodUndefined':
      case 'ZodVoid':
      case 'ZodAny':
      case 'ZodUnknown':
        return true;
      case 'ZodNullable':
      case 'ZodReadonly':
        return pureInputOptional(def.innerType!, charge, depth + 1, active);
      case 'ZodBranded':
        return pureInputOptional(def.type!, charge, depth + 1, active);
      case 'ZodUnion':
        return def.options!.some((option) =>
          pureInputOptional(option, charge, depth + 1, active)
        );
      case 'ZodIntersection':
        return (
          pureInputOptional(def.left!, charge, depth + 1, active) &&
          pureInputOptional(def.right!, charge, depth + 1, active)
        );
      default:
        return false;
    }
  } finally {
    active.delete(schema);
  }
}
