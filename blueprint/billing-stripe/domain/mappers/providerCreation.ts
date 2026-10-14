/** Structural creation-result validation only; no prior identity or ownership proof. */
export function validateProviderCreation(provider: unknown, object: string): void {
  if (!provider || typeof provider !== 'object' || Array.isArray(provider)) {
    throw new Error('Invalid Stripe billing response');
  }
  const result = provider as Record<string, unknown>;
  if (
    typeof result.id !== 'string' ||
    !result.id.trim() ||
    Buffer.byteLength(result.id, 'utf8') > 1024 ||
    /[\u0000-\u001f\u007f-\u009f]/u.test(result.id) ||
    result.object !== object
  ) {
    throw new Error('Invalid Stripe billing response');
  }
}
