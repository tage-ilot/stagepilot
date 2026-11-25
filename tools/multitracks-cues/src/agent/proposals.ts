import { createHash, randomBytes } from "node:crypto";
import type { CuePlan } from "../cues/models.js";

export interface ActionProposal {
  id: string;
  createdAt: string;
  expiresAt: string;
  setlistId: string;
  positions: number[];
  cueProfile: "setlist-ordinal-test";
  planDigest: string;
  expectedOperations: number;
  confirmation: string;
}

export function canonicalPlan(plan: CuePlan): unknown {
  return {
    setlistId: plan.setlist.id,
    cueProfile: plan.configuration.cueProfile,
    busId: plan.configuration.busId,
    items: plan.items.map((item) => ({
      setlistPosition: item.setlistPosition,
      songOrdinal: item.songOrdinal,
      velocity: item.velocity,
      targetType: item.targetType,
      targetId: item.targetId,
      bankId: item.bankId,
      resolvedPosition: item.resolvedPosition,
      operations: item.operations,
    })),
  };
}

export function digestPlan(plan: CuePlan): string {
  return createHash("sha256").update(JSON.stringify(canonicalPlan(plan))).digest("hex");
}

export class ProposalStore {
  readonly #proposals = new Map<string, ActionProposal>();
  latest?: ActionProposal;

  create(plan: CuePlan, positions: number[], ttlMs = 5 * 60_000): ActionProposal {
    const expectedOperations = plan.items.filter((item) => item.operations.some((operation) => operation.startsWith("CREATE_"))).length;
    const id = randomBytes(18).toString("base64url");
    const proposal: ActionProposal = {
      id,
      createdAt: new Date().toISOString(),
      expiresAt: new Date(Date.now() + ttlMs).toISOString(),
      setlistId: plan.setlist.id,
      positions,
      cueProfile: "setlist-ordinal-test",
      planDigest: digestPlan(plan),
      expectedOperations,
      confirmation: positions.length === 1 ? `APPLY ${plan.setlist.id} ${positions[0]}` : `APPLY ALL ${plan.setlist.id}`,
    };
    this.#proposals.set(id, proposal);
    this.latest = proposal;
    return proposal;
  }

  get(id: string, now = Date.now()): ActionProposal {
    const proposal = this.#proposals.get(id);
    if (!proposal) throw new Error("Unknown action proposal. Prepare a new proposal.");
    if (Date.parse(proposal.expiresAt) <= now) {
      this.#proposals.delete(id);
      throw new Error("Action proposal expired. Prepare a new proposal.");
    }
    return proposal;
  }

  invalidate(id: string): void { this.#proposals.delete(id); }
}

