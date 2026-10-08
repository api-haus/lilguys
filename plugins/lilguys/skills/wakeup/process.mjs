import { execFileSync } from "node:child_process";
import path from "node:path";

export function processTable() {
  if (process.platform === "win32") {
    const raw = execFileSync("powershell.exe", ["-NoProfile", "-Command",
      "Get-CimInstance Win32_Process | Select-Object ProcessId,ParentProcessId,Name,CommandLine | ConvertTo-Json -Compress"],
    { encoding: "utf8", windowsHide: true, timeout: 8000 });
    return [].concat(JSON.parse(raw)).map(p => ({ pid: p.ProcessId, parent: p.ParentProcessId, exe: p.Name, args: p.CommandLine ?? "" }));
  }
  return execFileSync("ps", ["-eo", "pid=,ppid=,comm=,args="], { encoding: "utf8", timeout: 8000 })
    .trim().split("\n").flatMap(line => {
      const m = line.trim().match(/^(\d+)\s+(\d+)\s+(\S+)\s+(.*)$/);
      return m ? [{ pid: Number(m[1]), parent: Number(m[2]), exe: m[3], args: m[4] }] : [];
    });
}

export function harnessOf(p) {
  const exe = path.basename(p.exe).replace(/\.exe$/i, "");
  if (["claude", "codex"].includes(exe)) return exe;
  // Inspect the executable/script prefix only: a prompt mentioning a package is not a harness.
  const prefix = p.args.match(/^(?:"[^"]+"|\S+)\s+(?:"([^"]+)"|(\S+))/);
  const script = prefix?.[1] ?? prefix?.[2] ?? "";
  if (/@anthropic-ai[\\/]claude-code/.test(script)) return "claude";
  if (/@openai[\\/]codex/.test(script)) return "codex";
  return null;
}

export function findHarness(table, pid = process.ppid) {
  const seen = new Set();
  while (pid > 1 && !seen.has(pid)) {
    seen.add(pid);
    const p = table.find(p => p.pid === pid);
    if (!p) return null;
    const harness = harnessOf(p);
    if (harness) return { pid, harness };
    pid = p.parent;
  }
  return null;
}

export function alive(pid) {
  if (!Number.isInteger(pid) || pid <= 1) return false;
  try { process.kill(pid, 0); return true; } catch (e) { return e.code === "EPERM"; }
}

export function terminateTree(pid) {
  if (!alive(pid)) return;
  if (process.platform === "win32") {
    execFileSync("taskkill.exe", ["/PID", String(pid), "/T", "/F"], { windowsHide: true, stdio: "ignore", timeout: 8000 });
  } else {
    try { process.kill(-pid, "SIGTERM"); } catch { process.kill(pid, "SIGTERM"); }
  }
}
