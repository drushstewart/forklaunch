import type { Auth } from "../../domain/services/auth.service.js";
import type { AuthRepository } from "../repositories/auth.repository.js";
// Real Better Auth creates password credentials; seeding never writes password hashes.
export async function seedDemo(
  auth: Auth,
  repository: AuthRepository,
  password: string,
) {
  const ids: string[] = [];
  for (const [email, name] of [
    ["alice@example.test", "Alice"],
    ["bob@example.test", "Bob"],
  ]) {
    const existing = await repository.findUser(email!);
    ids.push(
      existing ??
        (
          await auth.api.signUpEmail({
            body: { email: email!, name: name!, password },
          })
        ).user.id,
    );
  }
  await repository.seedWorkspaces(ids);
}
