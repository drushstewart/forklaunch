import type { AuthConfig } from "./config.js";
import { createAuth } from "./domain/services/auth.service.js";
import { AuthRepository } from "./persistence/repositories/auth.repository.js";

export async function registerDependencies(config: AuthConfig) {
  const repository = new AuthRepository(config.databaseUrl);
  try {
    // Better Auth creates auth.user before workspace foreign keys are installed.
    const auth = await createAuth(config, repository.pool);
    await repository.initialize();
    return { config, auth, repository };
  } catch (error) {
    await repository.close();
    throw error;
  }
}
export type AuthDependencies = Awaited<ReturnType<typeof registerDependencies>>;
