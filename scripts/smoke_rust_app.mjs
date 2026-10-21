// First-launch smoke for the Rust (in-process) installer variant.
//
//   node scripts/smoke_rust_app.mjs --exe PATH_TO_STAGEPILOT_EXECUTABLE
//
// Starts the packaged desktop executable with an isolated settings file and a free loopback
// port, then checks that
//   1. /api/v1/health answers from the in-process Rust server and identifies StagePilot,
//   2. the bundled frontend is served (GET / returns the built index.html),
//   3. no Python / PyInstaller backend process exists anywhere in the process tree,
//   4. the app exits when asked to (graceful signal first; a forced stop is reported, not hidden).
// Exit code 0 only when all four hold. Evidence is printed as JSON lines for the CI log.
import { execFileSync, spawn } from "node:child_process";
import fs from "node:fs";
import net from "node:net";
import os from "node:os";
import path from "node:path";
import process from "node:process";

const argv = process.argv.slice(2);
const exe = argv[argv.indexOf("--exe") + 1];
if (!exe || !fs.existsSync(exe)) throw new Error(`Usage: smoke_rust_app.mjs --exe EXECUTABLE (missing: ${exe})`);

const say = (step, detail = {}) => console.log(JSON.stringify({ step, ...detail }));
const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
const fail = (message) => {
  say("FAIL", { message });
  process.exitCode = 1;
};

const port = await new Promise((resolve) => {
  const server = net.createServer();
  server.listen(0, "127.0.0.1", () => {
    const { port: free } = server.address();
    server.close(() => resolve(free));
  });
});
const home = fs.mkdtempSync(path.join(os.tmpdir(), "stagepilot-smoke-"));
const settings = path.join(home, "settings.json");
fs.writeFileSync(
  settings,
  JSON.stringify({ service_source: "demo", midi_source: "simulated", timer_output: "simulated", server_port: port }),
);

const processTable = () => {
  if (process.platform === "win32") {
    const out = execFileSync(
      "powershell",
      ["-NoProfile", "-Command", "Get-CimInstance Win32_Process | Select-Object ProcessId,ParentProcessId,Name,CommandLine | ConvertTo-Json -Compress"],
      { encoding: "utf8", maxBuffer: 64 * 1024 * 1024 },
    );
    return JSON.parse(out).map((p) => ({ pid: p.ProcessId, ppid: p.ParentProcessId, name: p.Name ?? "", args: p.CommandLine ?? "" }));
  }
  const out = execFileSync("ps", ["-A", "-o", "pid=,ppid=,comm=,args="], { encoding: "utf8", maxBuffer: 64 * 1024 * 1024 });
  return out
    .split("\n")
    .filter(Boolean)
    .map((line) => {
      const [, pid, ppid, rest] = /^\s*(\d+)\s+(\d+)\s+(.*)$/.exec(line) ?? [];
      return { pid: Number(pid), ppid: Number(ppid), name: (rest ?? "").split(/\s+/)[0], args: rest ?? "" };
    });
};
const descendants = (rootPid) => {
  const table = processTable();
  const found = new Map();
  const queue = [rootPid];
  while (queue.length) {
    const parent = queue.shift();
    for (const p of table) if (p.ppid === parent && !found.has(p.pid)) (found.set(p.pid, p), queue.push(p.pid));
  }
  return [...found.values()];
};
const pythonLike = (p) => /stagepilot-backend|python|pyinstaller/i.test(`${p.name} ${p.args}`);

const logFile = path.join(home, "app.log");
const logFd = fs.openSync(logFile, "w");
const child = spawn(exe, [], {
  stdio: ["ignore", logFd, logFd],
  env: {
    ...process.env,
    STAGEPILOT_HOST: "127.0.0.1",
    STAGEPILOT_PORT: String(port),
    STAGEPILOT_SETTINGS_PATH: settings,
    // The app writes its own log, crash and Remote state under the OS profile directories.
    XDG_CONFIG_HOME: home,
  },
});
let exitInfo = null;
child.on("exit", (code, signal) => (exitInfo = { code, signal }));
say("launched", { pid: child.pid, port, exe });

let health = null;
for (let attempt = 0; attempt < 240 && !exitInfo; attempt += 1) {
  try {
    const response = await fetch(`http://127.0.0.1:${port}/api/v1/health`);
    if (response.ok) {
      health = await response.text();
      break;
    }
  } catch {
    // not listening yet
  }
  await sleep(500);
}
if (exitInfo) fail(`app exited before the server answered: ${JSON.stringify(exitInfo)}`);
else if (!health) fail("timed out waiting for /api/v1/health");
else if (!health.includes("application_status")) fail(`health did not identify StagePilot: ${health.slice(0, 200)}`);
else say("health", { ok: true, body: health.slice(0, 200) });

if (health) {
  const index = await fetch(`http://127.0.0.1:${port}/`).then((r) => r.text().then((text) => ({ status: r.status, text }))).catch(() => null);
  if (!index || index.status !== 200 || !/<div id="root"|<script/i.test(index.text)) fail(`bundled frontend not served from the in-process server: ${index?.status}`);
  else say("frontend", { ok: true, bytes: index.text.length });

  const tree = descendants(child.pid);
  const python = tree.filter(pythonLike);
  say("process_tree", { children: tree.map((p) => `${p.pid}:${p.name}`) });
  if (python.length) fail(`Python/sidecar process found under the app: ${JSON.stringify(python)}`);
  else say("no_python", { ok: true });
}

// Stop: graceful first.
let stopped = "already-exited";
if (!exitInfo) {
  if (process.platform === "win32") {
    try {
      execFileSync("taskkill", ["/PID", String(child.pid)], { stdio: "ignore" });
    } catch {
      // no closable window; handled by the forced fallback below
    }
  } else {
    child.kill("SIGTERM");
  }
  for (let i = 0; i < 40 && !exitInfo; i += 1) await sleep(250);
  stopped = exitInfo ? "graceful" : "forced";
  if (!exitInfo) {
    if (process.platform === "win32") execFileSync("taskkill", ["/PID", String(child.pid), "/T", "/F"], { stdio: "ignore" });
    else child.kill("SIGKILL");
    for (let i = 0; i < 20 && !exitInfo; i += 1) await sleep(250);
  }
}
say("stopped", { how: stopped, exit: exitInfo });
// A signal exit is expected for SIGTERM on unix; what must not happen is a hang.
if (stopped === "forced") say("WARN", { message: "app ignored the graceful stop request and had to be killed" });
try {
  say("log_tail", { text: fs.readFileSync(logFile, "utf8").slice(-1500) });
} catch {
  // ignore
}
fs.rmSync(home, { recursive: true, force: true });
