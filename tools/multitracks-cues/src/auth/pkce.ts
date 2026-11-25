import { createHash, randomBytes, timingSafeEqual } from "node:crypto";

const base64url = (value: Buffer): string => value.toString("base64url");

export function createPkce(): { verifier: string; challenge: string } {
  const verifier = base64url(randomBytes(48));
  const challenge = base64url(createHash("sha256").update(verifier).digest());
  return { verifier, challenge };
}

export function createState(): string {
  return base64url(randomBytes(32));
}

export function verifyState(expected: string, received: string | null): void {
  if (!received) throw new Error("OAuth callback did not include state.");
  const left = Buffer.from(expected);
  const right = Buffer.from(received);
  if (left.length !== right.length || !timingSafeEqual(left, right)) {
    throw new Error("OAuth state mismatch; authorization was rejected.");
  }
}

