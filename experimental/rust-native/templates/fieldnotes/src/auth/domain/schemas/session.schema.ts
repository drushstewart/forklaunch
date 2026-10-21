export interface SessionRequest {
  workspaceId: string;
}

export function parseSessionRequest(
  value: unknown,
): SessionRequest | undefined {
  if (typeof value !== "object" || value === null || !("workspaceId" in value))
    return;
  const workspaceId = value.workspaceId;
  if (
    typeof workspaceId !== "string" ||
    !/^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i.test(
      workspaceId,
    )
  )
    return;
  return { workspaceId };
}
