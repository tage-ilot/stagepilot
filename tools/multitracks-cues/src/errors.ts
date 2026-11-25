export class StagePilotCuesError extends Error {
  constructor(
    message: string,
    readonly exitCode: number,
    readonly details?: Record<string, unknown>,
  ) {
    super(message);
    this.name = new.target.name;
  }
}

export class AuthenticationError extends StagePilotCuesError {}
export class CapabilityError extends StagePilotCuesError {}
export class AmbiguityError extends StagePilotCuesError {}
export class SchemaError extends StagePilotCuesError {}
export class CredentialStoreError extends StagePilotCuesError {}
export class VerificationError extends StagePilotCuesError {}

