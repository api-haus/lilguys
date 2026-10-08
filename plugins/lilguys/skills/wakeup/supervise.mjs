#!/usr/bin/env node
// Restart a crashed runner on the owner's machine; never restart after an explicit stop.
import fs from "node:fs";
import path from "node:path";
import { spawn } from "node:child_process";
import { fileURLToPath } from "node:url";
import { location, options, write } from "./runner.mjs";
import { STATE, slug } from "./office.mjs";
import { alive } from "./process.mjs";

const args = process.argv.slice(2);
const opts = options([...args]);
if (opts.command !== "start") throw new Error("supervise.mjs accepts start; use runner.mjs for status/pause/resume/stop");
if (!opts.identity || !opts["prompt-file"] || !fs.readFileSync(path.resolve(opts["prompt-file"]), "utf8").trim()) throw new Error("start requires --identity and a nonempty --prompt-file");
const identity = slug(opts.identity);
const restartSeconds = Number(opts["restart-seconds"] ?? 30);
if (!Number.isFinite(restartSeconds) || restartSeconds <= 0) throw new Error("--restart-seconds must be positive");
const dir = location(opts.workspace ?? process.cwd());
fs.mkdirSync(dir, { recursive: true });
const stop = path.join(dir, "STOP");
const supervisor = path.join(dir, "supervisor.json");
const read = file => { try { return JSON.parse(fs.readFileSync(file, "utf8")); } catch { return {}; } };
const held = read(supervisor);
if (alive(held.pid)) throw new Error(`supervisor ${held.pid} is already running`);
if (held.pid) fs.rmSync(supervisor, { force: true });
const lease = fs.openSync(supervisor, "wx", 0o600);
fs.writeFileSync(lease, JSON.stringify({pid:process.pid})); fs.closeSync(lease);
fs.rmSync(stop, { force: true });
let child, ending = false;
const end = () => { ending = true; fs.writeFileSync(stop, "supervisor interrupted\n"); };
process.on("SIGINT", end); process.on("SIGTERM", end);
const delay = ms => new Promise(r => setTimeout(r, ms));
try {
  while (!ending && !fs.existsSync(stop)) {
    // A current interactive identity finishes its own work before this runner takes over.
    const active = fs.readdirSync(STATE).filter(f=>/^\d+\.json$/.test(f)).map(f=>read(path.join(STATE,f)))
      .filter(s=>s.name===identity && alive(s.anchor));
    if (active.length) {
      write(supervisor,{pid:process.pid,state:"waiting_for_identity",identity,waitingForPid:active[0].anchor});
      console.log(`${new Date().toISOString()} waiting for identity ${identity}, PID ${active[0].anchor}`);
      await delay(5000); continue;
    }
    child = spawn(process.execPath, [fileURLToPath(new URL("./runner.mjs", import.meta.url)), ...args], {windowsHide:true,stdio:"inherit"});
    write(supervisor,{pid:process.pid,state:"supervising",identity,runnerPid:child.pid});
    await new Promise(resolve=>{child.once("error",e=>{console.error(e.message);resolve();});child.once("close",resolve);});
    child = null;
    if (ending || fs.existsSync(stop)) break;
    console.log(`${new Date().toISOString()} runner exited; restarting in ${restartSeconds} seconds`);
    const until = Date.now() + restartSeconds * 1000;
    while(Date.now()<until && !ending && !fs.existsSync(stop)) await delay(Math.min(1000,restartSeconds*1000));
  }
} finally {
  if (read(supervisor).pid === process.pid) fs.rmSync(supervisor,{force:true});
}
