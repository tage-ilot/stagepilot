#!/usr/bin/env node
import path from "node:path";
import { mkdir } from "node:fs/promises";
import { Command, InvalidArgumentError } from "commander";
import { APP_VERSION, EXIT, SETLIST_ORDINAL_TEST_PROFILE } from "../constants.js";
import { ConfigurationStore, applicationDataDirectory } from "../config/store.js";
import { configurationSchema } from "../config/schema.js";
import { runDoctor } from "../doctor.js";
import { Reporter } from "../reporting/reporter.js";
import { sanitizedError } from "../security/redact.js";
import {
  applyCuePlan,
  connect,
  createAuthentication,
  inspectSetlist,
  listSetlists,
  saveSanitizedToolSchemas,
  verifyCuePlan,
} from "../services.js";
import { StagePilotCuesError } from "../errors.js";
import { ask, askSecret, confirmApply, print, renderPlan } from "./io.js";
import { PrivacyStore } from "../codex/privacy.js";
import { ensurePrivacy, runAgentRequest, withCodex } from "../agent/runner.js";
import { SessionStore } from "../agent/sessions.js";
import { createMultitracksCommand, shutdownMultitracks } from "../chatgpt/commands.js";

interface GlobalOptions {
  json?: boolean;
  quiet?: boolean;
  verbose?: boolean;
  color?: boolean;
}

const program = new Command();
program
  .name("stagepilot-cues")
  .description("Safely prepare and verify explicit MultiTracks MIDI cue test profiles.")
  .version(APP_VERSION)
  .option("--json", "print machine-readable JSON")
  .option("--quiet", "suppress normal terminal output")
  .option("--verbose", "show additional diagnostics")
  .option("--no-color", "disable colored output");

const globals = (): GlobalOptions => program.opts<GlobalOptions>();
const isoDate = (value: string): string => {
  if (!/^\d{4}-\d{2}-\d{2}$/.test(value) || Number.isNaN(Date.parse(`${value}T00:00:00Z`))) {
    throw new InvalidArgumentError("Use YYYY-MM-DD.");
  }
  return value;
};
const positiveInteger = (value: string): number => {
  const parsed = Number(value);
  if (!Number.isInteger(parsed) || parsed < 1) throw new InvalidArgumentError("Use a positive integer.");
  return parsed;
};
const cueProfile = (value: string): string => {
  if (value !== SETLIST_ORDINAL_TEST_PROFILE) {
    throw new InvalidArgumentError(`Use --cue-profile ${SETLIST_ORDINAL_TEST_PROFILE}.`);
  }
  return value;
};
const reportDirectory = (configured: string): string =>
  path.isAbsolute(configured) ? configured : path.join(applicationDataDirectory(), configured);

async function withConnection<T>(operation: (services: Awaited<ReturnType<typeof connect>>) => Promise<T>): Promise<T> {
  const services = await connect();
  try {
    return await operation(services);
  } finally {
    await services.close();
  }
}

const auth = program.command("auth").description("Manage MultiTracks OAuth authentication.");
const multitracksAuth = auth.command("multitracks").description("Manage the separate MultiTracks OAuth connection.");
const chatgptAuth = auth.command("chatgpt").description("Manage the StagePilot-scoped ChatGPT account via OpenAI OAuth.");

async function multiTracksLogin(): Promise<void> {
  const { configStore, auth: service } = createAuthentication();
  let configuration = await configStore.load();
  const organizations = await service.login(configuration);
  configuration = await configStore.load();
  if (organizations.length === 1) {
    const candidate = organizations[0]!;
    const answer = await ask(`Use MultiTracks organization '${candidate.name}' (${candidate.id})? [y/N] `);
    if (/^y(?:es)?$/i.test(answer)) configuration = await service.selectOrganization(configuration, candidate);
    else throw new StagePilotCuesError("Organization selection was not confirmed.", EXIT.AMBIGUOUS);
  } else if (organizations.length > 1) {
    print(organizations.map((organization, index) => `${index + 1}. ${organization.name} (${organization.id})`).join("\n"), globals());
    const selection = positiveInteger(await ask("Select the organization number: "));
    const candidate = organizations[selection - 1];
    if (!candidate) throw new StagePilotCuesError("Organization selection is out of range.", EXIT.AMBIGUOUS);
    if (!/^y(?:es)?$/i.test(await ask(`Confirm '${candidate.name}'? [y/N] `))) throw new StagePilotCuesError("Organization selection was not confirmed.", EXIT.AMBIGUOUS);
    configuration = await service.selectOrganization(configuration, candidate);
  }
  print({ authenticated: true, organization: configuration.organization ?? "not exposed by OAuth userinfo" }, globals());
}

auth.command("login").description("Log in through the system browser using PKCE.").action(async () => {
  process.stderr.write("Warning: 'auth login' is deprecated; use 'auth multitracks login'.\n");
  await multiTracksLogin();
});

auth.command("status").description("Show authentication state without exposing tokens.").action(async () => {
  process.stderr.write("Warning: 'auth status' is deprecated; use 'auth multitracks status'.\n");
  const { configStore, auth: service } = createAuthentication();
  print(await service.status(await configStore.load()), globals());
});

auth.command("logout").description("Revoke tokens when supported and remove local credentials.").action(async () => {
  process.stderr.write("Warning: 'auth logout' is deprecated; use 'auth multitracks logout'.\n");
  const { configStore, auth: service } = createAuthentication();
  await service.logout(await configStore.load());
  print("Logged out; local tokens and cached organization identity were removed.", globals());
});

multitracksAuth.command("login").action(multiTracksLogin);
multitracksAuth.command("status").action(async () => {
  const { configStore, auth: service } = createAuthentication();
  print(await service.status(await configStore.load()), globals());
});
multitracksAuth.command("logout").action(async () => {
  const { configStore, auth: service } = createAuthentication();
  await service.logout(await configStore.load());
  print("MultiTracks logout completed.", globals());
});

chatgptAuth.command("login").action(async () => {
  await withCodex(async ({ account }) => print(await account.login(), globals()));
});
chatgptAuth.command("status").action(async () => {
  await withCodex(async ({ account }) => print(await account.status(), globals()));
});
chatgptAuth.command("logout").action(async () => {
  await withCodex(async ({ account }) => account.logout());
  print("StagePilot-scoped ChatGPT logout confirmed.", globals());
});

program.command("ask")
  .argument("<request>")
  .option("--model <model>")
  .option("--accept-ai-data-sharing")
  .action(async (request: string, options: { model?: string; acceptAiDataSharing?: boolean }) => {
    await runAgentRequest(request, options);
  });

program.command("chat")
  .option("--model <model>")
  .option("--accept-ai-data-sharing")
  .action(async (options: { model?: string; acceptAiDataSharing?: boolean }) => {
    await ensurePrivacy(options.acceptAiDataSharing);
    await withCodex(async ({ account, threads }) => {
      if (!(await account.status()).authenticated) throw new Error("ChatGPT authentication is required.");
      let threadId = await threads.start(options.model);
      print("StagePilot agent chat. Commands: /status /new /sessions /logout /exit", globals());
      while (true) {
        const line = await ask("> ");
        if (line === "/exit") break;
        if (line === "/new") { threadId = await threads.start(options.model); print("New StagePilot agent session started.", globals()); continue; }
        if (line === "/status") { print(JSON.stringify(await account.status(), null, 2)); continue; }
        if (line === "/sessions") { print(JSON.stringify(await threads.sessions.list(), null, 2)); continue; }
        if (line === "/logout") { await account.logout(); print("Logged out.", globals()); break; }
        if (!line) continue;
        await threads.turn(threadId, line, (delta) => process.stdout.write(delta));
        process.stdout.write("\n");
      }
    });
  });

const agentCommand = program.command("agent").description("Manage optional agent mode.");
agentCommand.command("status").action(async () => {
  const privacy = await new PrivacyStore().status();
  await withCodex(async ({ account }) => print({ privacy, account: await account.status() }, globals()));
});
const privacy = agentCommand.command("privacy");
privacy.command("status").action(async () => print(await new PrivacyStore().status(), globals()));
privacy.command("reset").action(async () => { await new PrivacyStore().reset(); print("AI data-sharing consent reset.", globals()); });
const sessions = agentCommand.command("sessions");
sessions.command("list").action(async () => print(await new SessionStore().list(), globals()));
sessions.command("resume").argument("<thread-id>").argument("[request]").action(async (threadId: string, request?: string) => {
  if (request) await runAgentRequest(request, { resumeThreadId: threadId });
  else print(`Resume with: stagepilot-cues agent sessions resume ${threadId} "your request"`, globals());
});
sessions.command("delete").argument("<thread-id>").action(async (threadId: string) => {
  await new SessionStore().remove(threadId);
  print("StagePilot session deleted.", globals());
});

program.command("setup").description("Guide ChatGPT, MultiTracks, MIDI, and read-only validation.").action(async () => {
  print(`StagePilot setup\n\n[1/5] Runtime\n✓ Node ${process.versions.node}\n✓ StagePilot CLI built`, globals());
  print("\n[2/5] ChatGPT", globals());
  await withCodex(async ({ account }) => print(await account.status(), globals()));
  print("\n[3/5] MultiTracks", globals());
  const { configStore, auth: mtAuth } = createAuthentication();
  print(await mtAuth.status(await configStore.load()), globals());
  print("\n[4/5] MIDI\nRun 'stagepilot-cues configure' to select an exact dedicated bus.", globals());
  print("\n[5/5] Validation", globals());
  const checks = await runDoctor();
  print(checks.map((check) => `${check.status.toUpperCase()} ${check.name}: ${check.message}`).join("\n"), globals());
  print("No remote write was performed.", globals());
});

program.command("doctor").description("Run read-only environment, OAuth, MCP capability, and MIDI bus checks.").action(async () => {
  const checks = await runDoctor();
  print(globals().json ? checks : checks.map((check) => `${check.status.toUpperCase().padEnd(7)} ${check.name}: ${check.message}`).join("\n"), globals());
  if (checks.some((check) => check.status === "error")) process.exitCode = EXIT.AUTH;
});

program.command("tools")
  .description("List MCP tools and optionally save sanitized schemas.")
  .option("--output <file>", "write sanitized schemas to a local JSON file")
  .action(async (options: { output?: string }) => withConnection(async (services) => {
    const tools = services.client.listTools();
    if (options.output) {
      await mkdir(path.dirname(path.resolve(options.output)), { recursive: true });
      await saveSanitizedToolSchemas(services, path.resolve(options.output));
    }
    print(globals().json ? tools.map((tool) => tool.name) : tools.map((tool) => tool.name).join("\n"), globals());
  }));

const setlists = program.command("setlists").description("List and inspect MultiTracks setlists.");
setlists.command("list")
  .option("--from <date>", "first target date", isoDate)
  .option("--to <date>", "last target date", isoDate)
  .option("--limit <number>", "maximum results", positiveInteger, 50)
  .action(async (options: { from?: string; to?: string; limit: number }) => withConnection(async (services) => {
    const result = await listSetlists(services, options);
    print(globals().json ? result : result.map((item) => `${item.targetDate ?? "no date"}  ${item.name}  [${item.id}]`).join("\n") || "No setlists found.", globals());
  }));

setlists.command("inspect")
  .requiredOption("--setlist-id <id>")
  .requiredOption("--cue-profile <profile>", "explicit test cue profile", cueProfile)
  .option("--song-position <number>", "inspect one setlist position", positiveInteger)
  .action(async (options: { setlistId: string; songPosition?: number }) => runPlanCommand("inspect", options));

program.command("configure").description("Configure OAuth client, cue defaults, and an explicit MIDI bus.").action(async () => {
  const { configStore, auth: service } = createAuthentication();
  let current = await configStore.load();
  const clientId = await ask(`MultiTracks-issued OAuth client ID [${current.clientId ?? "not configured"}]: `);
  const clientSecret = clientId ? await askSecret("Optional MultiTracks-issued client secret (leave blank for none): ") : "";
  const redirectUri = await ask(`OAuth redirect URI [${current.redirectUri ?? "auto (dynamic port)"}]: `);
  if (clientId) current = await service.configureClient(current, clientId, clientSecret || undefined);
  if (redirectUri) current = { ...current, redirectUri };
  const channel = await ask(`MIDI channel [${current.channel}]: `);
  const note = await ask(`MIDI note [${current.note} / E7]: `);
  const reportDirectoryValue = await ask(`Report directory [${current.reportDirectory}]: `);
  current = configurationSchema.parse({
    ...current,
    channel: channel ? Number(channel) : current.channel,
    note: note ? Number(note) : current.note,
    reportDirectory: reportDirectoryValue || current.reportDirectory,
    color: !process.env.NO_COLOR,
  });
  try {
    await withConnection(async (services) => {
      const buses = await services.gateway.listMidiBuses();
      print(buses.map((bus) => `${bus.name ?? "unnamed"} | ${bus.type ?? "unknown type"} | ${bus.id}`).join("\n"), globals());
      const selectedId = await ask("Enter the exact stable ID of the dedicated MIDI bus (blank keeps current selection): ");
      if (selectedId) {
        const matches = buses.filter((bus) => bus.id === selectedId);
        if (matches.length !== 1) throw new StagePilotCuesError("Bus ID did not resolve to exactly one advertised bus.", EXIT.AMBIGUOUS);
        const selected = matches[0]!;
        if (/lyrics|lights|patch|guitar/i.test(`${selected.name ?? ""} ${selected.type ?? ""}`)) {
          throw new StagePilotCuesError("Refusing a protected or unrelated production bus; select a dedicated unused Aux bus.", EXIT.AMBIGUOUS);
        }
        current = { ...current, midiBus: { id: selected.id, name: selected.name, type: selected.type } };
      }
    });
  } catch (error) {
    print(`MIDI bus selection deferred: ${sanitizedError(error)}`, globals());
  }
  await configStore.save(current);
  print("Configuration saved. No remote MIDI data was changed.", globals());
});

program.command("prepare")
  .description("Dry-run the exact changes for a setlist (default safe mode).")
  .requiredOption("--setlist-id <id>")
  .requiredOption("--cue-profile <profile>", "explicit test cue profile", cueProfile)
  .option("--song-position <number>", "prepare one test song", positiveInteger)
  .action(async (options: { setlistId: string; songPosition?: number; cueProfile: string }) => runPlanCommand("prepare", options));

program.command("apply")
  .description("Explicitly apply and verify safe cue additions.")
  .requiredOption("--setlist-id <id>")
  .requiredOption("--cue-profile <profile>", "explicit test cue profile", cueProfile)
  .option("--song-position <number>", "apply to one test song", positiveInteger)
  .option("--yes", "skip typed confirmation only with --dangerous-development-confirmation")
  .option("--dangerous-development-confirmation", "allow --yes for controlled development")
  .action(async (options: { setlistId: string; songPosition?: number; yes?: boolean; dangerousDevelopmentConfirmation?: boolean; cueProfile: string }) => {
    const configStore = new ConfigurationStore();
    const configuration = await configStore.load();
    await withConnection(async (services) => {
      const positions = options.songPosition ? [options.songPosition] : undefined;
      const fresh = await inspectSetlist(services, configuration, options.setlistId, SETLIST_ORDINAL_TEST_PROFILE, positions);
      print(globals().json ? fresh : renderPlan(fresh), globals());
      if (options.yes && !options.dangerousDevelopmentConfirmation) {
        throw new StagePilotCuesError("--yes requires --dangerous-development-confirmation.", EXIT.INVALID);
      }
      if (!options.yes && !(await confirmApply(options.setlistId, options.songPosition))) {
        throw new StagePilotCuesError("Apply cancelled; no remote writes occurred.", EXIT.INVALID);
      }
      const startedAt = new Date().toISOString();
      const directory = reportDirectory(configuration.reportDirectory);
      const result = await applyCuePlan(services, configuration, options.setlistId, directory, SETLIST_ORDINAL_TEST_PROFILE, positions);
      const reporter = new Reporter(directory);
      const files = await reporter.write({
        startedAt,
        finishedAt: new Date().toISOString(),
        command: "apply",
        serverOrigin: new URL(configuration.serverUrl).origin,
        organization: configuration.organization,
        setlist: result.plan.setlist,
        configuration: result.plan.configuration,
        plan: result.plan.items,
        apply: result.results,
        finalStatus: result.success ? "success" : "failed",
      });
      print(globals().json ? { ...result, reports: files } : `${result.results.map((item) => `${item.status.toUpperCase()} ${item.songTitle}: ${item.message}`).join("\n")}\nReports: ${files.json}`, globals());
      if (!result.success) process.exitCode = EXIT.PARTIAL_FAILURE;
    });
  });

program.command("verify")
  .description("Read back and verify cues without writing.")
  .requiredOption("--setlist-id <id>")
  .requiredOption("--cue-profile <profile>", "explicit test cue profile", cueProfile)
  .option("--song-position <number>", "verify one setlist position", positiveInteger)
  .action(async (options: { setlistId: string; songPosition?: number; cueProfile: string }) => {
    const configStore = new ConfigurationStore();
    const configuration = await configStore.load();
    await withConnection(async (services) => {
      const startedAt = new Date().toISOString();
      const result = await verifyCuePlan(services, configuration, options.setlistId, SETLIST_ORDINAL_TEST_PROFILE, options.songPosition ? [options.songPosition] : undefined);
      const directory = reportDirectory(configuration.reportDirectory);
      const files = await new Reporter(directory).write({ startedAt, finishedAt: new Date().toISOString(), command: "verify", serverOrigin: new URL(configuration.serverUrl).origin, organization: configuration.organization, setlist: result.plan.setlist, configuration: result.plan.configuration, plan: result.plan.items, finalStatus: result.summary.success ? "success" : "failed" });
      print(globals().json ? { ...result, reports: files } : `${renderPlan(result.plan)}\n\nVerification: ${JSON.stringify(result.summary)}\nReports: ${files.json}`, globals());
      if (!result.summary.success) process.exitCode = EXIT.VERIFICATION;
    });
  });

program.command("test-real-setlist")
  .description("Guided read-only real-setlist inspection; always stops before the first write.")
  .action(async () => {
    const checks = await runDoctor();
    print(checks.map((check) => `${check.status.toUpperCase().padEnd(7)} ${check.name}: ${check.message}`).join("\n"), globals());
    if (checks.some((check) => check.status === "error")) {
      throw new StagePilotCuesError("Doctor found a blocking problem. Resolve it before inspecting a real setlist.", EXIT.AUTH);
    }
    const { configStore } = createAuthentication();
    const configuration = await configStore.load();
    await withConnection(async (services) => {
      const capability = services.client.validateCapabilities();
      if (capability.missing.length) {
        throw new StagePilotCuesError(`Required MCP tools are missing: ${capability.missing.join(", ")}.`, EXIT.CAPABILITY);
      }
      const directory = reportDirectory(configuration.reportDirectory);
      const schemaFile = path.join(directory, "multitracks-tools.sanitized.json");
      await mkdir(directory, { recursive: true });
      await saveSanitizedToolSchemas(services, schemaFile);
      const today = new Date().toISOString().slice(0, 10);
      const until = new Date(Date.now() + configuration.defaultDateWindowDays * 86_400_000).toISOString().slice(0, 10);
      const candidates = await listSetlists(services, { from: today, to: until, limit: 100 });
      print(candidates.map((item) => `${item.targetDate ?? "no date"}  ${item.name}  [${item.id}]`).join("\n") || "No upcoming setlists found.", globals());
      const setlistId = await ask("Enter one exact setlist ID to inspect: ");
      if (!setlistId || candidates.filter((item) => item.id === setlistId).length !== 1) {
        throw new StagePilotCuesError("The setlist ID did not resolve to exactly one listed setlist.", EXIT.AMBIGUOUS);
      }
      const plan = await inspectSetlist(services, configuration, setlistId, SETLIST_ORDINAL_TEST_PROFILE);
      const files = await new Reporter(directory).write({
        startedAt: plan.generatedAt,
        finishedAt: new Date().toISOString(),
        command: "prepare",
        serverOrigin: new URL(configuration.serverUrl).origin,
        organization: configuration.organization,
        setlist: plan.setlist,
        configuration: plan.configuration,
        plan: plan.items,
        finalStatus: plan.items.some((item) => ["ERROR", "SKIP_AMBIGUOUS", "SKIP_CONFLICT"].some((operation) => item.operations.includes(operation as never))) ? "failed" : "success",
      });
      print(`${renderPlan(plan)}\n\nSanitized schemas: ${schemaFile}\nReports: ${files.json}`, globals());
      const firstWritable = plan.items.find((item) => item.operations.some((operation) => operation.startsWith("CREATE_")));
      if (firstWritable) {
        print(`\nNo remote write occurred. Review the report, then test one song with:\n./bin/stagepilot-cues apply --setlist-id ${setlistId} --song-position ${firstWritable.setlistPosition} --cue-profile ${SETLIST_ORDINAL_TEST_PROFILE}`, globals());
      } else {
        print("\nNo safe one-song write is currently available; resolve every reported ambiguity or conflict first.", globals());
      }
    });
  });

program.command("sync-next")
  .description("Find one unambiguous upcoming setlist; dry-run unless --apply is supplied.")
  .option("--apply", "apply after typed confirmation")
  .requiredOption("--cue-profile <profile>", "explicit test cue profile", cueProfile)
  .option("--yes", "skip typed confirmation with --apply")
  .action(async (options: { apply?: boolean; yes?: boolean }) => {
    const configStore = new ConfigurationStore();
    const configuration = await configStore.load();
    await withConnection(async (services) => {
      const from = new Date().toISOString().slice(0, 10);
      const until = new Date(Date.now() + configuration.defaultDateWindowDays * 86_400_000).toISOString().slice(0, 10);
      let candidates = await listSetlists(services, { from, to: until, limit: 100 });
      if (configuration.setlistNameFilter) candidates = candidates.filter((item) => item.name === configuration.setlistNameFilter);
      if (candidates.length !== 1) throw new StagePilotCuesError(`Expected exactly one upcoming setlist; found ${candidates.length}. Use an exact name filter or an explicit setlist ID.`, EXIT.AMBIGUOUS);
      const setlistId = candidates[0]!.id;
      if (!options.apply) {
        const startedAt = new Date().toISOString();
        const plan = await inspectSetlist(services, configuration, setlistId, SETLIST_ORDINAL_TEST_PROFILE);
        const directory = reportDirectory(configuration.reportDirectory);
        const files = await new Reporter(directory).write({ startedAt, finishedAt: new Date().toISOString(), command: "prepare", serverOrigin: new URL(configuration.serverUrl).origin, organization: configuration.organization, setlist: plan.setlist, configuration: plan.configuration, plan: plan.items, finalStatus: "success" });
        print(globals().json ? { plan, reports: files } : `${renderPlan(plan)}\n\nReports: ${files.json}`, globals());
        return;
      }
      const plan = await inspectSetlist(services, configuration, setlistId, SETLIST_ORDINAL_TEST_PROFILE);
      print(renderPlan(plan), globals());
      if (!options.yes && !(await confirmApply(setlistId))) throw new StagePilotCuesError("Apply cancelled; no remote writes occurred.", EXIT.INVALID);
      const result = await applyCuePlan(services, configuration, setlistId, reportDirectory(configuration.reportDirectory), SETLIST_ORDINAL_TEST_PROFILE);
      print(result, globals());
      if (!result.success) process.exitCode = EXIT.PARTIAL_FAILURE;
    });
  });

async function runPlanCommand(command: "inspect" | "prepare", options: { setlistId: string; songPosition?: number }): Promise<void> {
  const configStore = new ConfigurationStore();
  const configuration = await configStore.load();
  await withConnection(async (services) => {
    const startedAt = new Date().toISOString();
    const plan = await inspectSetlist(services, configuration, options.setlistId, SETLIST_ORDINAL_TEST_PROFILE, options.songPosition ? [options.songPosition] : undefined);
    const directory = reportDirectory(configuration.reportDirectory);
    const files = await new Reporter(directory).write({ startedAt, finishedAt: new Date().toISOString(), command, serverOrigin: new URL(configuration.serverUrl).origin, organization: configuration.organization, setlist: plan.setlist, configuration: plan.configuration, plan: plan.items, finalStatus: plan.items.some((item) => item.operations.includes("ERROR")) ? "failed" : "success" });
    print(globals().json ? { plan, reports: files } : `${renderPlan(plan)}\n\nReports: ${files.json}`, globals());
  });
}

program.addCommand(createMultitracksCommand());

program.exitOverride();
program.configureOutput({ writeErr: (text) => process.stderr.write(text) });

program.parseAsync(process.argv).catch((error: unknown) => {
  const commanderCode = (error as { code?: string }).code;
  if (commanderCode === "commander.helpDisplayed" || commanderCode === "commander.version") return;
  const message = sanitizedError(error);
  if (globals().json) process.stderr.write(`${JSON.stringify({ error: message })}\n`);
  else process.stderr.write(`Error: ${message}\n`);
  process.exitCode = error instanceof StagePilotCuesError ? error.exitCode : EXIT.INVALID;
}).finally(async () => {
  await shutdownMultitracks();
});

