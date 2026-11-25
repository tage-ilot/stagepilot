import { Command } from "commander";
import { blockingAsk, newConversation, shutdownBrowser, ensureSession } from "./chatgpt-client.js";
import { loadSession, clearConversation } from "./session-store.js";

export function createMultitracksCommand(): Command {
  const multitracks = new Command("multitracks")
    .description("Interact with MultiTracks MCP via ChatGPT web interface")
    .action(async () => {
      multitracks.help();
    });

  // Login command
  multitracks
    .command("login")
    .description("Open ChatGPT in browser and wait for login")
    .action(async () => {
      console.log("Opening ChatGPT in browser...");
      console.log("Please log in manually. The browser will wait for you.\n");
      await ensureSession();
      console.log("\n✓ Login successful! You can now use other commands.");
      console.log("  Session will persist across runs.\n");
    });

  // Thread management commands
  const thread = multitracks
    .command("thread")
    .description("Manage ChatGPT conversation threads");

  thread
    .command("status")
    .description("Show current conversation thread info")
    .action(async () => {
      const session = await loadSession();
      if (session.conversationId) {
        console.log("Current conversation thread:");
        console.log(`  ID: ${session.conversationId}`);
        console.log(`  URL: ${session.conversationUrl}`);
        console.log(`  Last used: ${session.lastUsed}`);
      } else {
        console.log("No active conversation thread.");
        console.log("  A new thread will be created on your next ask.");
      }
    });

  thread
    .command("clear")
    .description("Clear the current conversation (start fresh next time)")
    .action(async () => {
      await clearConversation();
      console.log("Conversation cleared. Next command will start a new thread.");
    });

  thread
    .command("url")
    .description("Print the current conversation URL")
    .action(async () => {
      const session = await loadSession();
      if (session.conversationUrl) {
        console.log(session.conversationUrl);
      } else {
        console.log("No active conversation.");
      }
    });

  // Setlist commands
  const setlist = multitracks
    .command("setlist")
    .description("Manage setlists");

  setlist
    .command("list")
    .description("List your setlists")
    .action(async () => {
      const result = await blockingAsk("@Multitracks show me my recent setlists");
      console.log(result.response);
    });

  setlist
    .command("get")
    .description("Get setlist details")
    .argument("[name]", "setlist name or description")
    .action(async (name?: string) => {
      const prompt = name
        ? `@Multitracks get my setlist for ${name}`
        : "@Multitracks get my setlist for this Sunday";
      const result = await blockingAsk(prompt);
      console.log(result.response);
    });

  setlist
    .command("create")
    .description("Create a new setlist")
    .argument("<name>", "setlist name")
    .argument("<date>", "target date (YYYY-MM-DD)")
    .action(async (name: string, date: string) => {
      const result = await blockingAsk(`@Multitracks create a setlist called "${name}" for ${date}`);
      console.log(result.response);
    });

  setlist
    .command("duplicate")
    .description("Duplicate an existing setlist")
    .argument("<source>", "source setlist name")
    .argument("<date>", "new target date (YYYY-MM-DD)")
    .action(async (source: string, date: string) => {
      const result = await blockingAsk(`@Multitracks duplicate my "${source}" setlist for ${date}`);
      console.log(result.response);
    });

  // Song commands
  const song = multitracks
    .command("song")
    .description("Manage songs in setlists");

  song
    .command("add")
    .description("Add a song to a setlist")
    .argument("<song>", "song name")
    .argument("[setlist]", "setlist name (default: this Sunday)")
    .action(async (song: string, setlist?: string) => {
      const prompt = setlist
        ? `@Multitracks add "${song}" to my "${setlist}" setlist`
        : `@Multitracks add "${song}" to my Sunday setlist`;
      const result = await blockingAsk(prompt);
      console.log(result.response);
    });

  song
    .command("remove")
    .description("Remove a song from a setlist")
    .argument("<song>", "song name")
    .argument("[setlist]", "setlist name")
    .action(async (song: string, setlist?: string) => {
      const prompt = setlist
        ? `@Multitracks remove "${song}" from my "${setlist}" setlist`
        : `@Multitracks remove "${song}" from my Sunday setlist`;
      const result = await blockingAsk(prompt);
      console.log(result.response);
    });

  song
    .command("set-key")
    .description("Set the key for a song in a setlist")
    .argument("<song>", "song name")
    .argument("<key>", "musical key (e.g., G, D, A minor)")
    .argument("[setlist]", "setlist name")
    .action(async (song: string, key: string, setlist?: string) => {
      const prompt = setlist
        ? `@Multitracks set the key of "${song}" to ${key} in my "${setlist}" setlist`
        : `@Multitracks set the key of "${song}" to ${key} in my Sunday setlist`;
      const result = await blockingAsk(prompt);
      console.log(result.response);
    });

  song
    .command("set-tempo")
    .description("Set the tempo for a song in a setlist")
    .argument("<song>", "song name")
    .argument("<tempo>", "BPM (beats per minute)")
    .argument("[setlist]", "setlist name")
    .action(async (song: string, tempo: string, setlist?: string) => {
      const prompt = setlist
        ? `@Multitracks set the tempo of "${song}" to ${tempo} BPM in my "${setlist}" setlist`
        : `@Multitracks set the tempo of "${song}" to ${tempo} BPM in my Sunday setlist`;
      const result = await blockingAsk(prompt);
      console.log(result.response);
    });

  // Automation Cues commands (primary focus)
  const cue = multitracks
    .command("cue")
    .description("Manage automation cues for songs");

  cue
    .command("list")
    .description("List all automation cues for a song")
    .argument("<song>", "song name")
    .argument("[setlist]", "setlist name")
    .action(async (song: string, setlist?: string) => {
      const prompt = setlist
        ? `@Multitracks show me all the automation cues set up on "${song}" in my "${setlist}" setlist`
        : `@Multitracks show me all the automation cues set up on "${song}" in my Sunday setlist`;
      const result = await blockingAsk(prompt);
      console.log(result.response);
    });

  cue
    .command("targets")
    .description("List automatable targets for a song")
    .argument("<song>", "song name")
    .argument("[setlist]", "setlist name")
    .action(async (song: string, setlist?: string) => {
      const prompt = setlist
        ? `@Multitracks what tracks can I automate on "${song}" in my "${setlist}" setlist?`
        : `@Multitracks what tracks can I automate on "${song}" in my Sunday setlist?`;
      const result = await blockingAsk(prompt);
      console.log(result.response);
    });

  cue
    .command("create")
    .description("Create a new automation cue")
    .argument("<song>", "song name")
    .argument("<section>", "song section (e.g., chorus, bridge, verse)")
    .argument("[name]", "cue name")
    .argument("[setlist]", "setlist name")
    .action(async (song: string, section: string, name?: string, setlist?: string) => {
      const cueName = name ? ` called "${name}"` : "";
      const prompt = setlist
        ? `@Multitracks create an automation cue at the ${section} of "${song}"${cueName} in my "${setlist}" setlist`
        : `@Multitracks create an automation cue at the ${section} of "${song}"${cueName} in my Sunday setlist`;
      const result = await blockingAsk(prompt);
      console.log(result.response);
    });

  cue
    .command("delete")
    .description("Delete an automation cue")
    .argument("<song>", "song name")
    .argument("<name>", "cue name")
    .argument("[setlist]", "setlist name")
    .action(async (song: string, name: string, setlist?: string) => {
      const prompt = setlist
        ? `@Multitracks delete the "${name}" automation cue from "${song}" in my "${setlist}" setlist`
        : `@Multitracks delete the "${name}" automation cue from "${song}" in my Sunday setlist`;
      const result = await blockingAsk(prompt);
      console.log(result.response);
    });

  cue
    .command("rename")
    .description("Rename an automation cue")
    .argument("<song>", "song name")
    .argument("<old-name>", "current cue name")
    .argument("<new-name>", "new cue name")
    .argument("[setlist]", "setlist name")
    .action(async (song: string, oldName: string, newName: string, setlist?: string) => {
      const prompt = setlist
        ? `@Multitracks rename the "${oldName}" automation cue to "${newName}" on "${song}" in my "${setlist}" setlist`
        : `@Multitracks rename the "${oldName}" automation cue to "${newName}" on "${song}" in my Sunday setlist`;
      const result = await blockingAsk(prompt);
      console.log(result.response);
    });

  cue
    .command("clear")
    .description("Clear all automation cues for a song")
    .argument("<song>", "song name")
    .argument("[setlist]", "setlist name")
    .action(async (song: string, setlist?: string) => {
      const prompt = setlist
        ? `@Multitracks clear all automation cues for "${song}" in my "${setlist}" setlist`
        : `@Multitracks clear all automation cues for "${song}" in my Sunday setlist`;
      const result = await blockingAsk(prompt);
      console.log(result.response);
    });

  // Track mute commands
  const track = multitracks
    .command("track")
    .description("Manage track automation");

  track
    .command("mute")
    .description("Mute a track at a specific section")
    .argument("<song>", "song name")
    .argument("<track>", "track name (e.g., electric guitar, drums)")
    .argument("<section>", "song section (e.g., chorus, bridge)")
    .argument("[setlist]", "setlist name")
    .action(async (song: string, track: string, section: string, setlist?: string) => {
      const prompt = setlist
        ? `@Multitracks mute the ${track} at the ${section} of "${song}" in my "${setlist}" setlist`
        : `@Multitracks mute the ${track} at the ${section} of "${song}" in my Sunday setlist`;
      const result = await blockingAsk(prompt);
      console.log(result.response);
    });

  track
    .command("volume")
    .description("Set track volume at a specific section")
    .argument("<song>", "song name")
    .argument("<track>", "track name")
    .argument("<volume>", "volume percentage (0-100)")
    .argument("<section>", "song section")
    .argument("[setlist]", "setlist name")
    .action(async (song: string, track: string, volume: string, section: string, setlist?: string) => {
      const prompt = setlist
        ? `@Multitracks set the ${track} to ${volume}% volume at the ${section} of "${song}" in my "${setlist}" setlist`
        : `@Multitracks set the ${track} to ${volume}% volume at the ${section} of "${song}" in my Sunday setlist`;
      const result = await blockingAsk(prompt);
      console.log(result.response);
    });

  track
    .command("ramp")
    .description("Ramp track volume between two points")
    .argument("<song>", "song name")
    .argument("<track>", "track name")
    .argument("<start-volume>", "start volume percentage")
    .argument("<end-volume>", "end volume percentage")
    .argument("<section>", "song section")
    .argument("[setlist]", "setlist name")
    .action(async (song: string, track: string, startVol: string, endVol: string, section: string, setlist?: string) => {
      const prompt = setlist
        ? `@Multitracks ramp the ${track} from ${startVol}% to ${endVol}% over the ${section} of "${song}" in my "${setlist}" setlist`
        : `@Multitracks ramp the ${track} from ${startVol}% to ${endVol}% over the ${section} of "${song}" in my Sunday setlist`;
      const result = await blockingAsk(prompt);
      console.log(result.response);
    });

  // Bus commands
  const bus = multitracks
    .command("bus")
    .description("Manage bus automation");

  bus
    .command("mute")
    .description("Mute a bus at a specific section")
    .argument("<song>", "song name")
    .argument("<bus>", "bus name (e.g., click, guide)")
    .argument("<section>", "song section")
    .argument("[setlist]", "setlist name")
    .action(async (song: string, busName: string, section: string, setlist?: string) => {
      const prompt = setlist
        ? `@Multitracks mute the ${busName} bus at the ${section} of "${song}" in my "${setlist}" setlist`
        : `@Multitracks mute the ${busName} bus at the ${section} of "${song}" in my Sunday setlist`;
      const result = await blockingAsk(prompt);
      console.log(result.response);
    });

  bus
    .command("volume")
    .description("Set bus volume at a specific section")
    .argument("<song>", "song name")
    .argument("<bus>", "bus name")
    .argument("<volume>", "volume percentage")
    .argument("<section>", "song section")
    .argument("[setlist]", "setlist name")
    .action(async (song: string, busName: string, volume: string, section: string, setlist?: string) => {
      const prompt = setlist
        ? `@Multitracks set the ${busName} bus to ${volume}% volume at the ${section} of "${song}" in my "${setlist}" setlist`
        : `@Multitracks set the ${busName} bus to ${volume}% volume at the ${section} of "${song}" in my Sunday setlist`;
      const result = await blockingAsk(prompt);
      console.log(result.response);
    });

  bus
    .command("ramp")
    .description("Ramp bus volume between two points")
    .argument("<song>", "song name")
    .argument("<bus>", "bus name")
    .argument("<start-volume>", "start volume percentage")
    .argument("<end-volume>", "end volume percentage")
    .argument("<section>", "song section")
    .argument("[setlist]", "setlist name")
    .action(async (song: string, busName: string, startVol: string, endVol: string, section: string, setlist?: string) => {
      const prompt = setlist
        ? `@Multitracks ramp the ${busName} bus from ${startVol}% to ${endVol}% over the ${section} of "${song}" in my "${setlist}" setlist`
        : `@Multitracks ramp the ${busName} bus from ${startVol}% to ${endVol}% over the ${section} of "${song}" in my Sunday setlist`;
      const result = await blockingAsk(prompt);
      console.log(result.response);
    });

  // Click and Guide commands
  const click = multitracks
    .command("click")
    .description("Manage click track automation");

  click
    .command("mute")
    .description("Mute click at a specific section")
    .argument("<song>", "song name")
    .argument("<section>", "song section")
    .argument("[setlist]", "setlist name")
    .action(async (song: string, section: string, setlist?: string) => {
      const prompt = setlist
        ? `@Multitracks mute the click track at the ${section} of "${song}" in my "${setlist}" setlist`
        : `@Multitracks mute the click track at the ${section} of "${song}" in my Sunday setlist`;
      const result = await blockingAsk(prompt);
      console.log(result.response);
    });

  const guide = multitracks
    .command("guide")
    .description("Manage guide vocal automation");

  guide
    .command("mute")
    .description("Mute guide at a specific section")
    .argument("<song>", "song name")
    .argument("<section>", "song section")
    .argument("[setlist]", "setlist name")
    .action(async (song: string, section: string, setlist?: string) => {
      const prompt = setlist
        ? `@Multitracks mute the guide vocal at the ${section} of "${song}" in my "${setlist}" setlist`
        : `@Multitracks mute the guide vocal at the ${section} of "${song}" in my Sunday setlist`;
      const result = await blockingAsk(prompt);
      console.log(result.response);
    });

  // MIDI commands
  const midi = multitracks
    .command("midi")
    .description("Manage MIDI output automation");

  midi
    .command("mute")
    .description("Mute MIDI output at a specific section")
    .argument("<song>", "song name")
    .argument("<section>", "song section")
    .argument("[setlist]", "setlist name")
    .action(async (song: string, section: string, setlist?: string) => {
      const prompt = setlist
        ? `@Multitracks mute MIDI output at the ${section} of "${song}" in my "${setlist}" setlist`
        : `@Multitracks mute MIDI output at the ${section} of "${song}" in my Sunday setlist`;
      const result = await blockingAsk(prompt);
      console.log(result.response);
    });

  // Pad commands
  const pad = multitracks
    .command("pad")
    .description("Manage pad player automation");

  pad
    .command("mute")
    .description("Mute pad at a specific section")
    .argument("<song>", "song name")
    .argument("<section>", "song section")
    .argument("[setlist]", "setlist name")
    .action(async (song: string, section: string, setlist?: string) => {
      const prompt = setlist
        ? `@Multitracks mute the pad at the ${section} of "${song}" in my "${setlist}" setlist`
        : `@Multitracks mute the pad at the ${section} of "${song}" in my Sunday setlist`;
      const result = await blockingAsk(prompt);
      console.log(result.response);
    });

  // Free-form ask command
  multitracks
    .command("ask")
    .description("Ask anything about MultiTracks")
    .argument("<prompt>", "your question or request")
    .action(async (prompt: string) => {
      const result = await blockingAsk(`@Multitracks ${prompt}`);
      console.log(result.response);
    });

  // New conversation command
  multitracks
    .command("new")
    .description("Start a new ChatGPT conversation")
    .action(async () => {
      const result = await newConversation();
      console.log(result.message);
    });

  return multitracks;
}

export async function shutdownMultitracks(): Promise<void> {
  await shutdownBrowser();
}

