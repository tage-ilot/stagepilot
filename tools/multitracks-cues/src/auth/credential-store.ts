import {
  deletePassword,
  getKeyring,
  getPassword,
  initBackend,
  setPassword,
  type SecretStorageBackend,
} from "cross-keychain";
import { CredentialStoreError } from "../errors.js";
import { EXIT } from "../constants.js";

const SECURE_BACKENDS = new Set([
  "native-macos",
  "macos",
  "native-windows",
  "windows",
  "native-linux",
  "secret-service",
]);

export interface CredentialBackend {
  id: string;
  getPassword(service: string, account: string): Promise<string | null>;
  setPassword(service: string, account: string, password: string): Promise<void>;
  deletePassword(service: string, account: string): Promise<void>;
}

export class CredentialVault {
  private backend?: CredentialBackend;

  constructor(
    private readonly backendFactory: () => Promise<CredentialBackend> = async () => {
      await initBackend((candidate: SecretStorageBackend) => SECURE_BACKENDS.has(candidate.id));
      return getKeyring();
    },
  ) {}

  async availability(): Promise<{ available: boolean; backend?: string; error?: string }> {
    try {
      const backend = await this.getBackend();
      return { available: SECURE_BACKENDS.has(backend.id), backend: backend.id };
    } catch (error) {
      return { available: false, error: error instanceof Error ? error.message : String(error) };
    }
  }

  async get(service: string, account: string): Promise<string | null> {
    return (await this.getBackend()).getPassword(service, account);
  }

  async set(service: string, account: string, secret: string): Promise<void> {
    await (await this.getBackend()).setPassword(service, account, secret);
  }

  async delete(service: string, account: string): Promise<void> {
    try {
      await (await this.getBackend()).deletePassword(service, account);
    } catch (error) {
      if (!/not found|does not exist/i.test(error instanceof Error ? error.message : String(error))) {
        throw error;
      }
    }
  }

  private async getBackend(): Promise<CredentialBackend> {
    try {
      this.backend ??= await this.backendFactory();
      if (!SECURE_BACKENDS.has(this.backend.id)) {
        throw new Error(`Refusing insecure credential backend '${this.backend.id}'.`);
      }
      return this.backend;
    } catch (error) {
      throw new CredentialStoreError(
        `A secure operating-system credential store is unavailable: ${error instanceof Error ? error.message : String(error)}`,
        EXIT.CREDENTIAL_STORE,
      );
    }
  }
}

export const crossKeychainBackend: CredentialBackend = {
  id: "deferred",
  getPassword,
  setPassword,
  deletePassword,
};

