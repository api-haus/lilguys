import { test } from "node:test";
import assert from "node:assert/strict";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import http from "node:http";
import { spawn } from "node:child_process";
import { fileURLToPath } from "node:url";
import { findHarness, harnessOf } from "./process.mjs";
import { control } from "./runner.mjs";

const runner = fileURLToPath(new URL("./runner.mjs", import.meta.url));
const office = fileURLToPath(new URL("./office.mjs", import.meta.url));
const sleep = ms => new Promise(r => setTimeout(r, ms));

test("process detection follows parents and ignores prompts mentioning a harness", () => {
  assert.equal(harnessOf({ exe: "powershell.exe", args: 'powershell -Command "read @anthropic-ai/claude-code"' }), null);
  assert.equal(harnessOf({ exe: "node", args: 'node "C:\\Program Files\\@anthropic-ai\\claude-code\\cli.js"' }), "claude");
  assert.deepEqual(findHarness([{ pid: 20, parent: 10, exe: "sh", args: "sh" }, { pid: 10, parent: 1, exe: "claude.exe", args: "claude.exe" }], 20), { pid: 10, harness: "claude" });
  assert.equal(findHarness([{ pid: 20, parent: 20, exe: "sh", args: "sh" }], 20), null);
});

test("only the linked owner can pause or resume, and prose is not a control", () => {
  assert.equal(control({ via: "owner", text: "agent: /stop" }, "owner"), "pause");
  assert.equal(control({ via: "brother", text: "/stop" }, "owner"), null);
  assert.equal(control({ via: "owner", text: "please stop deleting things" }, "owner"), null);
  assert.equal(control({ via: "owner", text: "go" }, "owner"), "resume");
});

async function fixture(t) {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "office-runner-test-"));
  const state = path.join(dir, "state");
  const workspace = path.join(dir, "workspace"); fs.mkdirSync(workspace);
  const promptFile = path.join(dir, "task.txt"); fs.writeFileSync(promptFile, "Verify the runner in this temporary workspace only.");
  const fixtureWorker = path.join(dir, "worker.mjs");
  fs.writeFileSync(fixtureWorker, `
import fs from 'node:fs';
import {execFileSync} from 'node:child_process';
const office=${JSON.stringify(office)};
let prompt=''; for await (const c of process.stdin) prompt+=c;
if (!prompt.includes('OFFICE BRIEFING')) throw Error('no briefing');
const anchor=process.env.LILGUYS_OFFICE_SESSION_PID;
const session=JSON.parse(fs.readFileSync(process.env.LILGUYS_OFFICE_STATE+'/'+anchor+'.json'));
const context=execFileSync(process.execPath,[office,'hook'],{input:JSON.stringify({hook_event_name:'SessionStart'}),encoding:'utf8'});
const n=Number(fs.existsSync('count.txt')?fs.readFileSync('count.txt','utf8'):0)+1;
fs.writeFileSync('count.txt',String(n));
fs.writeFileSync('context-'+n+'.txt',context);
if(process.env.FIXTURE_WAIT) await new Promise(r=>setTimeout(r,Number(process.env.FIXTURE_WAIT)));
console.log(JSON.stringify({type:'assistant',message:{content:[{type:'tool_use',name:'Read'}]}}));
if(process.env.FIXTURE_FAIL) { console.error('expected failure'); process.exit(1); }
for(const f of fs.readdirSync(session.runnerDir+'/inbox').filter(f=>f.endsWith('.seen'))) {
 const m=JSON.parse(fs.readFileSync(session.runnerDir+'/inbox/'+f));
 execFileSync(process.execPath,[office,'done',String(m.id)],{encoding:'utf8'});
}
execFileSync(process.execPath,[office,'log','entry','Verified cycle '+n],{encoding:'utf8'});
execFileSync(process.execPath,[office,'say','--brief','Runner test','DONE test cycle '+n],{encoding:'utf8'});
console.log(JSON.stringify({type:'result',subtype:'success',is_error:false,total_cost_usd:0.01,result:'cycle '+n}));
`);
  const data = { entries: [], messages: [], reports: [], activity: [], wakeups: 0, sleeps: 0, id: 0, offline: 0 };
  const enqueue = (text, via = "owner") => data.messages.push({ id: ++data.id, text, brief: text, via, sender: via, room: "tests", recipient: "worker", state: "queued" });
  const server = http.createServer(async (req, res) => {
    if (req.headers.authorization !== "Bearer test-key") { res.writeHead(401); res.end('{}'); return; }
    let raw = ""; for await (const c of req) raw += c;
    const b = raw ? JSON.parse(raw) : {};
    const route = new URL(req.url, "http://localhost").pathname;
    if (data.offline > 0) { data.offline--; res.writeHead(503); res.end('{"error":"temporary outage"}'); return; }
    let out = {};
    const box = () => ({ open: data.messages.filter(m => m.state === "open"), queued: data.messages.filter(m => m.state === "queued").length });
    switch(route) {
      case '/wakeup': data.wakeups++; out={me:{owner:'owner',room:'tests'},briefing:{entries:data.entries},present:[]}; break;
      case '/deliver': { const fresh=data.messages.filter(m=>m.state==='queued').slice(0,3-box().open.length); fresh.forEach(m=>m.state='open'); out={delivered:fresh,...box()}; break; }
      case '/mail': out=box(); break;
      case '/briefing': out={entries:data.entries, pending:null,priority:null}; break;
      case '/done': b.ids.forEach(id=>{const m=data.messages.find(m=>m.id===id);if(m)m.state='closed';});out={closed:b.ids,...box()};break;
      case '/log': data.entries.push({text:b.text,kind:b.kind,at:Date.now()}); break;
      case '/say': data.reports.push(b.text);out={room:'tests'};break;
      case '/activity': data.activity.push(b); break;
      case '/sleep': data.sleeps++; break;
      default: res.writeHead(404); res.end('{}'); return;
    }
    res.setHeader('content-type','application/json');res.end(JSON.stringify(out));
  });
  await new Promise(r => server.listen(0, "127.0.0.1", r));
  const configFile = path.join(dir, "config.json");
  fs.writeFileSync(configFile, JSON.stringify({ url:`http://127.0.0.1:${server.address().port}`,owner:'owner',token:'test-key' }));
  const children = [];
  const launch = (extra = [], env = {}) => {
    const entry = env.FIXTURE_SUPERVISOR ? fileURLToPath(new URL('./supervise.mjs',import.meta.url)) : runner;
    const p = spawn(process.execPath, [entry, "start", "--workspace", workspace, "--identity", "worker", "--prompt-file", promptFile,
      "--claude", process.execPath, "--claude-arg", fixtureWorker, "--interval-seconds", "0.08", "--poll-seconds", "0.05", ...extra],
    { env:{...process.env,LILGUYS_OFFICE_STATE:state,LILGUYS_OFFICE_CONFIG:configFile,...env},stdio:['ignore','pipe','pipe'] });
    children.push(p); let output="";p.stdout.on('data',c=>output+=c);p.stderr.on('data',c=>output+=c);
    const done = new Promise(resolve=>p.on('close',code=>resolve({code,output})));
    return {p,done};
  };
  const status = () => {
    try { const root=path.join(state,'runners');const sub=fs.readdirSync(root).find(f=>fs.statSync(path.join(root,f)).isDirectory());return JSON.parse(fs.readFileSync(path.join(root,sub,'status.json'))); }catch{return {};}
  };
  const command = cmd => { const root=path.dirname(path.join(status().workspace??workspace,'x')); return spawn(process.execPath,[runner,cmd,'--workspace',root],{env:{...process.env,LILGUYS_OFFICE_STATE:state},stdio:'ignore'}); };
  t.after(async()=>{for(const p of children)if(p.exitCode===null)p.kill();server.closeAllConnections();await new Promise(r=>server.close(r));fs.rmSync(dir,{recursive:true,force:true});});
  return { dir,state,workspace,data,enqueue,launch,status,command };
}

async function until(check, message, timeout = 15000) {
  const start=Date.now();while(Date.now()-start<timeout){if(check())return;await sleep(50);}throw Error(message);
}

test("mail reaches the worker; reports and the next cycle need no new prompt; restart reads the log", {timeout:30000}, async t => {
  const f=await fixture(t); f.enqueue('Owner asks for a verified step'); f.data.offline=2;
  const first=f.launch(['--cycles','2']); const result=await first.done;
  const debug = fs.readdirSync(path.join(f.state,'runners')).filter(x=>!x.endsWith('.json')).flatMap(x=>fs.readdirSync(path.join(f.state,'runners',x)).filter(y=>y.startsWith('cycle-')).map(y=>fs.readFileSync(path.join(f.state,'runners',x,y),'utf8'))).join('\n');
  assert.equal(result.code,0,result.output);assert.equal(f.data.reports.length,2,result.output+'\n'+debug);
  assert.equal(fs.readFileSync(path.join(f.workspace,'count.txt'),'utf8'),'2');
  assert.match(fs.readFileSync(path.join(f.workspace,'context-1.txt'),'utf8'),/your own user owner/);
  assert.equal(f.data.messages[0].state,'closed');assert.equal(f.data.sleeps,1);
  const second=f.launch(['--cycles','1']);assert.equal((await second.done).code,0);
  assert.equal(fs.readFileSync(path.join(f.workspace,'count.txt'),'utf8'),'3');
  assert.equal(f.data.wakeups,2); assert.ok(f.data.entries.some(e=>e.text==='Verified cycle 3'));
});

test("duplicate worker is refused; foreign stop is data; owner pause/resume and local stop work", {timeout:30000}, async t => {
  const f=await fixture(t);const first=f.launch([], {FIXTURE_WAIT:'350'});
  await until(()=>f.status().state==='working','worker did not start');
  const duplicate=f.launch(['--cycles','1']);assert.equal((await duplicate.done).code,1);
  f.enqueue('/stop','brother');await until(()=>f.data.messages[0]?.state==='closed','foreign message was not handled');
  assert.equal(first.p.exitCode,null);
  f.enqueue('/pause');await until(()=>f.status().state==='paused','owner pause did not work');
  assert.equal(first.p.exitCode,null);f.enqueue('/go');await until(()=>f.status().state==='working','owner resume did not work');
  f.command('stop');const end=await first.done;assert.equal(end.code,0,end.output);assert.equal(f.status().state,'stopped');
});

test("three failed cycles pause instead of silently spending forever", {timeout:30000}, async t => {
  const f=await fixture(t);const worker=f.launch([], {FIXTURE_FAIL:'1'});
  await until(()=>f.status().state==='paused','failure backoff did not pause');
  assert.equal(fs.readFileSync(path.join(f.workspace,'count.txt'),'utf8'),'3');
  f.command('stop');assert.equal((await worker.done).code,0);
});

test("daily run limit survives restarts and prevents another model launch", {timeout:30000}, async t => {
  const f=await fixture(t);const worker=f.launch(['--max-runs-per-day','1']);
  await until(()=>f.status().state==='budget_limit','daily limit was not enforced');
  assert.equal(fs.readFileSync(path.join(f.workspace,'count.txt'),'utf8'),'1');
  f.command('stop');assert.equal((await worker.done).code,0);
  const resumed=f.launch(['--max-runs-per-day','1']);
  await until(()=>f.status().state==='budget_limit' && f.status().pid===resumed.p.pid,'restart lost the daily limit');
  assert.equal(fs.readFileSync(path.join(f.workspace,'count.txt'),'utf8'),'1');
  f.command('stop');assert.equal((await resumed.done).code,0);
});

test("supervisor recovers an abruptly killed runner and stops on explicit local stop", {timeout:30000}, async t => {
  const f=await fixture(t);const supervisor=f.launch(['--restart-seconds','0.1'], {FIXTURE_SUPERVISOR:'1',FIXTURE_WAIT:'700'});
  await until(()=>f.status().state==='working' && fs.existsSync(path.join(f.workspace,'count.txt')),'first child did not start');
  const deadPid=f.status().pid;process.kill(deadPid,'SIGKILL');
  await until(()=>f.status().state==='working' && f.status().pid!==deadPid,'supervisor did not restart');
  await until(()=>Number(fs.readFileSync(path.join(f.workspace,'count.txt'),'utf8'))>=2,'replacement child did not start');
  f.command('stop');assert.equal((await supervisor.done).code,0);
});
