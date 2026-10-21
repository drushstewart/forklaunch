import { loadConfig, required } from "./config.js";
import { registerDependencies } from "./registrations.js";
import { seedDemo } from "./persistence/seeders/demo.seeder.js";

export async function bootstrap() {
  const dependencies = await registerDependencies(loadConfig());
  try {
    if (dependencies.config.seedDemo)
      await seedDemo(
        dependencies.auth,
        dependencies.repository,
        required("DEMO_PASSWORD"),
      );
    return dependencies;
  } catch (error) {
    await dependencies.repository.close();
    throw error;
  }
}
