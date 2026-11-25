import { z } from "zod";
import { SETLIST_ORDINAL_TEST_PROFILE } from "../constants.js";
import { ConfigurationStore } from "../config/store.js";
import type { CuePlan } from "../cues/models.js";
import { connect, inspectSetlist, listSetlists, verifyCuePlan, type ConnectedServices } from "../services.js";
import { redact } from "../security/redact.js";
import { ProposalStore } from "./proposals.js";

export interface DynamicToolSpec {
  type: "function";
  name: string;
  description: string;
  inputSchema: Record<string, unknown>;
}

const date = z.string().regex(/^\d{4}-\d{2}-\d{2}$/);
const listInput = z.object({ from: date.optional(), to: date.optional(), limit: z.number().int().min(1).max(100).default(50) }).strict();
const setlistInput = z.object({ setlistId: z.string().min(1) }).strict();
const prepareInput = z.object({ setlistId: z.string().min(1), songPosition: z.number().int().min(1).optional() }).strict();

const specs: DynamicToolSpec[] = [
  { type: "function", name: "stagepilot_connection_status", description: "Read sanitized StagePilot and MultiTracks readiness.", inputSchema: { type: "object", properties: {}, additionalProperties: false } },
  { type: "function", name: "stagepilot_list_upcoming_setlists", description: "List normalized upcoming MultiTracks setlists.", inputSchema: { type: "object", properties: { from: { type: "string" }, to: { type: "string" }, limit: { type: "integer", minimum: 1, maximum: 100 } }, additionalProperties: false } },
  { type: "function", name: "stagepilot_inspect_setlist", description: "Inspect one exact setlist using StagePilot's deterministic planner.", inputSchema: { type: "object", properties: { setlistId: { type: "string" } }, required: ["setlistId"], additionalProperties: false } },
  { type: "function", name: "stagepilot_prepare_ordinal_cues", description: "Prepare a dry-run ordinal E7 cue plan. Performs no writes.", inputSchema: { type: "object", properties: { setlistId: { type: "string" }, songPosition: { type: "integer", minimum: 1 } }, required: ["setlistId"], additionalProperties: false } },
  { type: "function", name: "stagepilot_verify_ordinal_cues", description: "Read back and verify ordinal E7 cues. Performs no writes.", inputSchema: { type: "object", properties: { setlistId: { type: "string" }, songPosition: { type: "integer", minimum: 1 } }, required: ["setlistId"], additionalProperties: false } },
  { type: "function", name: "stagepilot_propose_apply_one", description: "Create an in-memory proposal for exactly one safe cue. Performs no writes.", inputSchema: { type: "object", properties: { setlistId: { type: "string" }, songPosition: { type: "integer", minimum: 1 } }, required: ["setlistId", "songPosition"], additionalProperties: false } },
  { type: "function", name: "stagepilot_propose_apply_all", description: "Create an in-memory full-setlist proposal only when every item is safe. Performs no writes.", inputSchema: { type: "object", properties: { setlistId: { type: "string" } }, required: ["setlistId"], additionalProperties: false } },
];

function projectPlan(plan: CuePlan): unknown {
  return {
    setlist: { id: plan.setlist.id, name: plan.setlist.name, targetDate: plan.setlist.targetDate },
    configuration: plan.configuration,
    items: plan.items.map(({ setlistPosition, songOrdinal, velocity, songTitle, targetType, targetId, bankId, busId, resolvedPosition, operations, reason, risk }) =>
      ({ setlistPosition, songOrdinal, velocity, songTitle, targetType, targetId, bankId, busId, resolvedPosition, operations, reason, risk })),
  };
}

export class StagePilotAgentTools {
  readonly proposals = new ProposalStore();
  constructor(
    readonly configStore = new ConfigurationStore(),
    readonly connector: () => Promise<ConnectedServices> = () => connect(),
  ) {}
  definitions(): DynamicToolSpec[] { return structuredClone(specs); }

  async call(name: string, input: unknown): Promise<unknown> {
    const configuration = await this.configStore.load();
    if (name === "stagepilot_connection_status") {
      z.object({}).strict().parse(input);
      return { multitracksConfigured: Boolean(configuration.clientId), organization: configuration.organization, midiBus: configuration.midiBus, cueProfile: SETLIST_ORDINAL_TEST_PROFILE };
    }
    const services = await this.connector();
    try {
      if (name === "stagepilot_list_upcoming_setlists") return redact(await listSetlists(services, listInput.parse(input)));
      if (name === "stagepilot_inspect_setlist") {
        const { setlistId } = setlistInput.parse(input);
        return projectPlan(await inspectSetlist(services, configuration, setlistId, SETLIST_ORDINAL_TEST_PROFILE));
      }
      if (name === "stagepilot_prepare_ordinal_cues") {
        const { setlistId, songPosition } = prepareInput.parse(input);
        return projectPlan(await inspectSetlist(services, configuration, setlistId, SETLIST_ORDINAL_TEST_PROFILE, songPosition ? [songPosition] : undefined));
      }
      if (name === "stagepilot_verify_ordinal_cues") {
        const { setlistId, songPosition } = prepareInput.parse(input);
        const result = await verifyCuePlan(services, configuration, setlistId, SETLIST_ORDINAL_TEST_PROFILE, songPosition ? [songPosition] : undefined);
        return { plan: projectPlan(result.plan), summary: result.summary };
      }
      if (name === "stagepilot_propose_apply_one") {
        const parsed = prepareInput.extend({ songPosition: z.number().int().min(1) }).parse(input);
        const plan = await inspectSetlist(services, configuration, parsed.setlistId, SETLIST_ORDINAL_TEST_PROFILE, [parsed.songPosition]);
        const creates = plan.items.filter((item) => item.operations.some((operation) => operation.startsWith("CREATE_")));
        if (creates.length !== 1 || plan.items.some((item) => item.operations.some((operation) => ["ERROR", "SKIP_AMBIGUOUS", "SKIP_CONFLICT"].includes(operation)))) {
          throw new Error("Exactly one safe create operation was not available.");
        }
        return this.proposals.create(plan, [parsed.songPosition]);
      }
      if (name === "stagepilot_propose_apply_all") {
        const { setlistId } = setlistInput.parse(input);
        const plan = await inspectSetlist(services, configuration, setlistId, SETLIST_ORDINAL_TEST_PROFILE);
        if (plan.items.some((item) => item.operations.some((operation) => ["ERROR", "SKIP_AMBIGUOUS", "SKIP_CONFLICT"].includes(operation)))) throw new Error("The full-setlist plan contains unresolved risks.");
        const positions = plan.items.filter((item) => item.operations.some((operation) => operation.startsWith("CREATE_"))).map((item) => item.setlistPosition);
        if (!positions.length) throw new Error("The plan contains no missing cues to apply.");
        return this.proposals.create(plan, positions);
      }
      throw new Error(`Unknown StagePilot agent tool: ${name}`);
    } finally { await services.close(); }
  }
}

