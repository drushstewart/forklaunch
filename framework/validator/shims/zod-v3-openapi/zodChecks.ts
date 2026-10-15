import type { ZodNumberCheck, ZodStringCheck } from 'zod/v3';

export const zodChecksKeyword = 'x-forklaunch-zod-checks';
const maximumRules = 10000;
const own = (value: object, key: string) =>
  Object.prototype.hasOwnProperty.call(value, key);
type Metadata = Record<string, unknown>;
interface Checks {
  version: 1;
  utf16MinLength?: number;
  utf16MaxLength?: number;
  formats?: ('email' | 'uuid' | 'uri' | 'cuid' | 'cuid2' | 'ulid' | 'nanoid' | 'base64' | 'base64url')[];
  multipleOf?: (number | 'NaN' | 'Infinity' | '-Infinity')[];
}
function admit(checks: readonly unknown[], metadata: readonly Metadata[]) {
  if (checks.length > maximumRules)
    throw new Error('Zod check profile exceeds 10000 rules');
  if (metadata.some((value) => own(value, zodChecksKeyword)))
    throw new Error('The reserved Zod check keyword cannot be overridden');
}
export function projectStringChecks(
  checks: readonly ZodStringCheck[],
  metadata: readonly Metadata[],
  factoryEmail = false
): Checks | undefined {
  admit(checks, metadata);
  const formats = checks
    .filter(
      (check) =>
        ['email', 'uuid', 'url', 'cuid', 'cuid2', 'ulid', 'nanoid', 'base64', 'base64url'].includes(check.kind) &&
        !metadata.some((value) =>
          own(
            value,
            check.kind === 'email' && factoryEmail ? 'pattern' : 'format'
          )
        )
    )
    .map((check) =>
      check.kind === 'url' ? ('uri' as const) : (check.kind as NonNullable<Checks['formats']>[number])
    );
  const formatResult: Checks | undefined = formats.length
    ? { version: 1, formats }
    : undefined;
  if (
    formats.length &&
    checks.some((check) =>
      ['trim', 'toLowerCase', 'toUpperCase'].includes(check.kind)
    )
  )
    throw new Error(
      'Zod check projection cannot combine string transformations with format checks'
    );
  const bounds = checks.filter(
    (check) =>
      check.kind === 'min' || check.kind === 'max' || check.kind === 'length'
  );
  if (!bounds.length) return formatResult;
  const minOverride = metadata.some((value) => own(value, 'minLength'));
  const maxOverride = metadata.some((value) => own(value, 'maxLength'));
  const needsMin = !minOverride && bounds.some((check) => check.kind !== 'max');
  const needsMax = !maxOverride && bounds.some((check) => check.kind !== 'min');
  if (!needsMin && !needsMax) return formatResult;
  if (
    checks.some(
      (check) =>
        check.kind === 'trim' ||
        check.kind === 'toLowerCase' ||
        check.kind === 'toUpperCase'
    )
  )
    throw new Error(
      'Zod check projection cannot combine string transformations with length checks'
    );
  const result: Checks = formatResult ?? { version: 1 };
  for (const check of bounds) {
    if (
      (check.kind === 'min' && minOverride) ||
      (check.kind === 'max' && maxOverride)
    )
      continue;
    if (!Number.isSafeInteger(check.value) || check.value < 0)
      throw new Error(
        'Zod check length bounds must be nonnegative safe integers'
      );
    if (!minOverride && (check.kind === 'min' || check.kind === 'length'))
      result.utf16MinLength = Math.max(result.utf16MinLength ?? 0, check.value);
    if (!maxOverride && (check.kind === 'max' || check.kind === 'length'))
      result.utf16MaxLength = Math.min(
        result.utf16MaxLength ?? check.value,
        check.value
      );
  }
  return Object.keys(result).length > 1 ? result : undefined;
}
export function projectNumberChecks(
  checks: readonly ZodNumberCheck[],
  metadata: readonly Metadata[]
): Checks | undefined {
  admit(checks, metadata);
  if (metadata.some((value) => own(value, 'multipleOf'))) return undefined;
  const divisors = checks
    .filter((check) => check.kind === 'multipleOf')
    .map((check) =>
      Number.isNaN(check.value)
        ? ('NaN' as const)
        : check.value === Infinity
          ? ('Infinity' as const)
          : check.value === -Infinity
            ? ('-Infinity' as const)
            : check.value
    );
  return divisors.length ? { version: 1, multipleOf: divisors } : undefined;
}
