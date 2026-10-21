import { loadConfig } from "../config.js";
import { registerDependencies } from "../registrations.js";
const { repository } = await registerDependencies(loadConfig());
await repository.close();
