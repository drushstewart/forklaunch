import { z } from 'zod/v3';

const factoryUri = Symbol('ForkLaunch Zod WHATWG URL input grammar');
export const factoryUriFormat = 'forklaunch-whatwg-url';

export function markFactoryUri(schema: z.ZodString): z.ZodString {
  const definition = { ...schema._def, [factoryUri]: true };
  return new z.ZodString(definition);
}

export function hasFactoryUriGrammar(schema: z.ZodString): boolean {
  const definition = schema._def as z.ZodStringDef & {
    [factoryUri]?: boolean;
  };
  return (
    definition[factoryUri] === true &&
    definition.checks.length === 1 &&
    definition.checks[0].kind === 'url'
  );
}
