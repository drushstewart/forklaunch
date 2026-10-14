import { z } from 'zod/v3';

// Kept on the definition so ordinary wrapper/export cloning preserves identity
// without inspecting titles, user annotations, or executing schema callbacks.
const factoryEmail = Symbol('ForkLaunch Zod email input grammar');

export const factoryEmailPattern =
  "^(?!\\.)(?!.*\\.\\.)([A-Za-z0-9_'+\\-\\.]*)[A-Za-z0-9_+-]@([A-Za-z0-9][A-Za-z0-9\\-]*\\.)+[A-Za-z]{2,}$";

export function markFactoryEmail(schema: z.ZodString): z.ZodString {
  const definition = { ...schema._def, [factoryEmail]: true };
  return new z.ZodString(definition);
}

export function hasFactoryEmailGrammar(schema: z.ZodString): boolean {
  const definition = schema._def as z.ZodStringDef & {
    [factoryEmail]?: boolean;
  };
  return (
    definition[factoryEmail] === true &&
    definition.checks.length === 1 &&
    definition.checks[0].kind === 'email'
  );
}
