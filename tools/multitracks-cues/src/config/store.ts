import { constants as fsConstants } from "node:fs";
import { access, copyFile, mkdir, open, readFile, rename } from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import { configurationSchema, defaultConfiguration, type Configuration } from "./schema.js";

export function applicationDataDirectory(
  platform = process.platform,
  env: NodeJS.ProcessEnv = process.env,
  home = os.homedir(),
): string {
  if (platform === "darwin") {
    return path.posix.join(home, "Library", "Application Support", "StagePilot", "multitracks-cues");
  }
  if (platform === "win32") {
    const appData = env.APPDATA ?? path.join(home, "AppData", "Roaming");
    return path.win32.join(appData, "StagePilot", "multitracks-cues");
  }
  return path.join(env.XDG_CONFIG_HOME ?? path.join(home, ".config"), "StagePilot", "multitracks-cues");
}

export function configurationPath(): string {
  return process.env.STAGEPILOT_CUES_CONFIG_PATH ?? path.join(applicationDataDirectory(), "config.json");
}

export class ConfigurationStore {
  constructor(readonly filePath = configurationPath()) {}

  async load(): Promise<Configuration> {
    try {
      const raw = await readFile(this.filePath, "utf8");
      const parsed: unknown = JSON.parse(raw);
      return configurationSchema.parse(parsed);
    } catch (error) {
      const code = (error as NodeJS.ErrnoException).code;
      if (code === "ENOENT") return defaultConfiguration;
      const backup = `${this.filePath}.corrupt-${new Date().toISOString().replaceAll(":", "-")}`;
      await mkdir(path.dirname(this.filePath), { recursive: true });
      try {
        await access(this.filePath, fsConstants.F_OK);
        await copyFile(this.filePath, backup);
      } catch {
        // The invalid file may have disappeared between read and preservation.
      }
      throw new Error(`Configuration is invalid. A diagnostic copy was preserved at ${backup}.`, {
        cause: error,
      });
    }
  }

  async save(configuration: Configuration): Promise<void> {
    const valid = configurationSchema.parse(configuration);
    const directory = path.dirname(this.filePath);
    await mkdir(directory, { recursive: true });
    const temporary = path.join(directory, `.${path.basename(this.filePath)}.${process.pid}.tmp`);
    const handle = await open(temporary, "w", 0o600);
    try {
      await handle.writeFile(`${JSON.stringify(valid, null, 2)}\n`, "utf8");
      await handle.sync();
    } finally {
      await handle.close();
    }
    await rename(temporary, this.filePath);
  }
}

