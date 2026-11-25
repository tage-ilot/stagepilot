import path from "node:path";
import { CodexAccountService } from "../codex/account-service.js";
import { SETLIST_ORDINAL_TEST_PROFILE } from "../constants.js";
import { ConfigurationStore, applicationDataDirectory } from "../config/store.js";
import { applyCuePlan, connect, inspectSetlist } from "../services.js";
import { ask, renderPlan } from "../cli/io.js";
import { digestPlan } from "./proposals.js";
import { AgentThreadService } from "./thread-service.js";
import { AI_DISCLOSURE, PrivacyStore } from "../codex/privacy.js";
import { createOpenAIClient } from "../ai/client.js";

export async function withCodex<T>(operation: (context: { account: CodexAccountService; threads: AgentThreadService }) => Promise<T>): Promise<T> {
  const client = createOpenAIClient();
  const account = new CodexAccountService();
  const threads = new AgentThreadService(client, undefined);
  return operation({ account, threads });
}

export async function ensurePrivacy(acceptFlag = false): Promise<void> {
  const store = new PrivacyStore();
  if ((await store.status()).accepted) return;
  process.stdout.write(`${AI_DISCLOSURE}\n\n`);
  if (acceptFlag) { await store.accept(); return; }
  if (!process.stdin.isTTY) throw new Error("AI data-sharing consent is required. Re-run interactively or use --accept-ai-data-sharing.");
  if (!/^y(?:es)?$/i.test(await ask("Accept and continue? [y/N] "))) throw new Error("AI data-sharing consent was not accepted.");
  await store.accept();
}

export async function runAgentRequest(request: string, options: { model?: string; acceptAiDataSharing?: boolean; resumeThreadId?: string } = {}): Promise<string> {
  await ensurePrivacy(options.acceptAiDataSharing);
  return withCodex(async ({ account, threads }) => {
    if (!(await account.status()).authenticated) throw new Error("ChatGPT authentication is required. Run stagepilot-cues auth chatgpt login.");
    const threadId = options.resumeThreadId ? await threads.resume(options.resumeThreadId, options.model) : await threads.start(options.model);
    const answer = await threads.turn(threadId, request, (delta) => process.stdout.write(delta));
    process.stdout.write("\n");
    const proposal = threads.tools.proposals.latest;
    if (!proposal) return answer;
    const configuration = await new ConfigurationStore().load();
    const services = await connect();
    try {
      const positions = proposal.confirmation.startsWith("APPLY ALL") ? undefined : proposal.positions;
      const fresh = await inspectSetlist(services, configuration, proposal.setlistId, SETLIST_ORDINAL_TEST_PROFILE, positions);
      if (digestPlan(fresh) !== proposal.planDigest) {
        threads.tools.proposals.invalidate(proposal.id);
        throw new Error("The deterministic plan changed after proposal creation. Prepare a new proposal.");
      }
      process.stdout.write(`\n${renderPlan(fresh)}\n`);
      if ((await ask(`Type ${proposal.confirmation} to execute this proposal, or press Enter to decline: `)) !== proposal.confirmation) {
        process.stdout.write("Proposal declined. No remote write occurred.\n");
        return answer;
      }
      const reportDirectory = path.isAbsolute(configuration.reportDirectory)
        ? configuration.reportDirectory
        : path.join(applicationDataDirectory(), configuration.reportDirectory);
      const result = await applyCuePlan(services, configuration, proposal.setlistId, reportDirectory, SETLIST_ORDINAL_TEST_PROFILE, positions);
      if (!result.success) throw new Error("StagePilot could not verify every requested cue.");
      await threads.turn(threadId, `StagePilot deterministic execution result: ${JSON.stringify({ verified: true, results: result.results })}. Summarize this verified result without adding claims.`);
      return answer;
    } finally { await services.close(); }
  });
}

export function codexVersionLabel(): string { return "openai-oauth"; }

