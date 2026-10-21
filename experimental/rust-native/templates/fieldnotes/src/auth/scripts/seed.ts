import { loadConfig, required } from "../config.js";
import { registerDependencies } from "../registrations.js";
import { seedDemo } from "../persistence/seeders/demo.seeder.js";
const { auth, repository } = await registerDependencies(loadConfig());
try {
  await seedDemo(auth, repository, required("DEMO_PASSWORD"));
} finally {
  await repository.close();
}
