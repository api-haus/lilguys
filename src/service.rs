//! Starting, stopping and finding the daemon — over the systemd unit when there is one, and by
//! hand when there is not, because nobody should have to know which.

use crate::doctor::{Check, Status};
use anyhow::{bail, Context, Result};
use std::path::PathBuf;
use std::process::{Command, Stdio};

pub const UNIT: &str = "lilguys";
const DAEMON: &str = "lilguysd";

/// What the systemd user unit is doing, if it exists at all.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Unit {
    /// No unit file, or no systemd. The daemon is then an ordinary process.
    Absent,
    Installed { active: bool, enabled: bool },
}

impl Unit {
    pub fn active(self) -> bool {
        matches!(self, Unit::Installed { active: true, .. })
    }
}

pub fn unit_path() -> PathBuf {
    dirs::config_dir().unwrap_or_default().join("systemd/user").join(format!("{UNIT}.service"))
}

pub fn unit() -> Unit {
    if !which("systemctl") || !unit_path().is_file() {
        return Unit::Absent;
    }
    Unit::Installed { active: systemctl_says("is-active"), enabled: systemctl_says("is-enabled") }
}

fn systemctl_says(question: &str) -> bool {
    Command::new("systemctl")
        .args(["--user", question, UNIT])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

fn systemctl(verb: &str) -> Result<()> {
    let status = Command::new("systemctl")
        .args(["--user", verb, UNIT])
        .status()
        .with_context(|| format!("systemctl --user {verb} {UNIT}"))?;
    match status.success() {
        true => Ok(()),
        false => bail!("systemctl --user {verb} {UNIT} exited with {status}"),
    }
}

/// Every running daemon, by reading `/proc` rather than shelling out to `pgrep`.
pub fn pids() -> Vec<u32> {
    let Ok(entries) = std::fs::read_dir("/proc") else { return Vec::new() };
    entries
        .flatten()
        .filter_map(|e| {
            let pid: u32 = e.file_name().to_str()?.parse().ok()?;
            let comm = std::fs::read_to_string(e.path().join("comm")).ok()?;
            (comm.trim() == DAEMON).then_some(pid)
        })
        .filter(|pid| *pid != std::process::id())
        .collect()
}

pub fn running() -> bool {
    !pids().is_empty()
}

/// The daemon beside this binary first: a `cargo build` tree and an installed one both work, and
/// neither picks up the other's copy.
pub fn daemon_binary() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|d| d.join(DAEMON)))
        .filter(|p| p.is_file())
        .unwrap_or_else(|| PathBuf::from(DAEMON))
}

/// Starts the daemon and says how. Already-running is success, because a caller that cannot tell
/// whether a step worked will run it again.
pub fn start() -> Result<String> {
    if running() {
        return Ok("already running".into());
    }
    if unit() != Unit::Absent {
        systemctl("start")?;
        return Ok(format!("systemctl --user start {UNIT}"));
    }
    spawn_detached()
}

/// Its own process group, or a caller that dies — a shell, a timed-out tool call — takes the
/// daemon with it.
fn spawn_detached() -> Result<String> {
    use std::os::unix::process::CommandExt;
    let bin = daemon_binary();
    let log = crate::log::default_dir().join("daemon.log");
    let _ = std::fs::create_dir_all(log.parent().unwrap_or(&log));
    let out = std::fs::OpenOptions::new().create(true).append(true).open(&log);
    let (stdout, stderr) = match out {
        Ok(f) => (Stdio::from(f.try_clone()?), Stdio::from(f)),
        Err(_) => (Stdio::null(), Stdio::null()),
    };
    let child = Command::new(&bin)
        .stdin(Stdio::null())
        .stdout(stdout)
        .stderr(stderr)
        .process_group(0)
        .spawn()
        .with_context(|| format!("start {}", bin.display()))?;
    Ok(format!("{} started as pid {} · log {}", bin.display(), child.id(), log.display()))
}

pub fn stop() -> Result<String> {
    if unit().active() {
        systemctl("stop")?;
        return Ok(format!("systemctl --user stop {UNIT}"));
    }
    let pids = pids();
    if pids.is_empty() {
        return Ok("not running".into());
    }
    for pid in &pids {
        // SAFETY: a pid read from /proc a moment ago; a stale one fails with ESRCH and no more.
        unsafe { libc::kill(*pid as libc::pid_t, libc::SIGTERM) };
    }
    Ok(format!("sent SIGTERM to {}", pids.iter().map(u32::to_string).collect::<Vec<_>>().join(", ")))
}

/// Stop, wait for the old process to actually go, then start: the layer surface is released on the
/// way out, and a new daemon that races the old one loses in ways nobody would blame on a restart.
pub fn restart() -> Result<String> {
    if unit().active() {
        systemctl("restart")?;
        return Ok(format!("systemctl --user restart {UNIT}"));
    }
    let stopped = stop()?;
    for _ in 0..50 {
        if !running() {
            return Ok(format!("{stopped} · {}", start()?));
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    bail!("{stopped}, but {DAEMON} is still running five seconds later")
}

/// Whether the daemon is up, and whether anything will bring it back after a reboot.
pub fn status_check() -> Check {
    let pids = pids();
    let unit = unit();
    let how = match unit {
        Unit::Absent => "no systemd unit".to_string(),
        Unit::Installed { active, enabled } => {
            format!("unit {} · {}", if active { "active" } else { "inactive" },
                    if enabled { "enabled at login" } else { "not enabled at login" })
        }
    };
    match pids.as_slice() {
        [] => Check {
            name: "daemon",
            status: Status::Warn,
            detail: format!("not running · {how}"),
            fix: Some("`lilguy start`".into()),
        },
        pids => Check {
            name: "daemon",
            status: Status::Ok,
            detail: format!(
                "pid {} · {how}",
                pids.iter().map(u32::to_string).collect::<Vec<_>>().join(", ")
            ),
            fix: None,
        },
    }
}

pub fn which(bin: &str) -> bool {
    if bin.contains('/') {
        return std::path::Path::new(bin).exists();
    }
    std::env::var_os("PATH")
        .map(|p| std::env::split_paths(&p).any(|d| d.join(bin).exists()))
        .unwrap_or(false)
}
