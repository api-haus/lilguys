//! Hyprland IPC. Measured at 14.6 us per request — poll freely, never spawn `hyprctl`.

use anyhow::{bail, Context, Result};
use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;

pub struct Hypr {
    request: PathBuf,
    events: PathBuf,
}

impl Hypr {
    pub fn from_env() -> Result<Self> {
        let runtime = std::env::var("XDG_RUNTIME_DIR").context("XDG_RUNTIME_DIR unset")?;
        let sig = std::env::var("HYPRLAND_INSTANCE_SIGNATURE")
            .context("HYPRLAND_INSTANCE_SIGNATURE unset — not a Hyprland session")?;
        let dir = PathBuf::from(runtime).join("hypr").join(sig);
        if !dir.is_dir() {
            bail!("no hyprland instance dir at {}", dir.display());
        }
        Ok(Self { request: dir.join(".socket.sock"), events: dir.join(".socket2.sock") })
    }

    pub fn request(&self, cmd: &str) -> Result<String> {
        let mut sock = UnixStream::connect(&self.request)?;
        sock.write_all(cmd.as_bytes())?;
        let mut out = String::new();
        sock.read_to_string(&mut out)?;
        Ok(out)
    }

    pub fn cursor_pos(&self) -> Result<(f32, f32)> {
        let raw = self.request("cursorpos")?;
        let (x, y) = raw.split_once(',').context("malformed cursorpos")?;
        Ok((x.trim().parse()?, y.trim().parse()?))
    }

    /// Rect and a readable name for the first mapped window matching `needle` in class or title.
    /// No Wayland protocol exposes another window's geometry, so this is the only route.
    pub fn window_rect(&self, needle: &str) -> Option<([f32; 4], String)> {
        let raw = self.request("j/clients").ok()?;
        let clients: serde_json::Value = serde_json::from_str(&raw).ok()?;
        let needle = needle.to_lowercase();
        clients.as_array()?.iter().find_map(|c| {
            if !c.get("mapped")?.as_bool()? {
                return None;
            }
            let class = c.get("class")?.as_str()?;
            let title = c.get("title")?.as_str()?;
            if !class.to_lowercase().contains(&needle) && !title.to_lowercase().contains(&needle) {
                return None;
            }
            let at = c.get("at")?.as_array()?;
            let size = c.get("size")?.as_array()?;
            Some((
                [
                    at[0].as_f64()? as f32,
                    at[1].as_f64()? as f32,
                    size[0].as_f64()? as f32,
                    size[1].as_f64()? as f32,
                ],
                if class.is_empty() { title.to_string() } else { class.to_string() },
            ))
        })
    }

    /// Non-blocking line stream of compositor events (`workspace>>2`, `activewindow>>class,title`).
    pub fn events(&self) -> Result<EventStream> {
        let sock = UnixStream::connect(&self.events)?;
        sock.set_nonblocking(true)?;
        Ok(EventStream { lines: BufReader::new(sock), buf: String::new() })
    }
}

pub struct EventStream {
    lines: BufReader<UnixStream>,
    buf: String,
}

pub struct Event {
    pub kind: String,
    pub data: String,
}

impl EventStream {
    /// Drains whatever has arrived. Never blocks.
    pub fn poll(&mut self) -> Vec<Event> {
        let mut out = Vec::new();
        loop {
            self.buf.clear();
            match self.lines.read_line(&mut self.buf) {
                Ok(0) => break,
                Ok(_) => {
                    let line = self.buf.trim_end();
                    let (kind, data) = line.split_once(">>").unwrap_or((line, ""));
                    out.push(Event { kind: kind.to_string(), data: data.to_string() });
                }
                Err(_) => break,
            }
        }
        out
    }
}
