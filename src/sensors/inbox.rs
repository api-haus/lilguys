//! The socket a feeding mechanic pushes into. One JSON feeling per line, from any process.

use super::{Feeling, Observation};
use anyhow::Result;
use std::io::{BufRead, BufReader};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;

pub fn socket_path() -> PathBuf {
    std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join("lilguys.sock")
}

/// Listens until the channel closes. Anything that can write a line to a unix socket can give a
/// lilguy a feeling, which is the whole extension surface for hunger, mood, pets, or a plugin
/// nobody has written yet.
pub fn spawn(tx: calloop::channel::Sender<Observation>) -> Result<PathBuf> {
    let path = socket_path();
    // A stale socket from a crashed run would block the bind; nothing else owns this name.
    let _ = std::fs::remove_file(&path);
    let listener = UnixListener::bind(&path)?;
    let out = path.clone();

    std::thread::Builder::new()
        .name("inbox".into())
        .spawn(move || {
            for stream in listener.incoming().flatten() {
                // One connection per thread: a plugin that holds its socket open must not deafen
                // the buddy to every other plugin behind it in the accept queue.
                let tx = tx.clone();
                std::thread::Builder::new()
                    .name("inbox-conn".into())
                    .spawn(move || {
                        let _ = serve(stream, &tx);
                    })
                    .ok();
            }
        })
        .ok();
    Ok(out)
}

/// One connection may send many lines and stay open, or send one and close. Both are fine.
fn serve(stream: UnixStream, tx: &calloop::channel::Sender<Observation>) -> Result<(), ()> {
    for line in BufReader::new(stream).lines().map_while(Result::ok) {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        match serde_json::from_str::<Feeling>(line) {
            Ok(feeling) => {
                if tx.send(Observation::Feeling(feeling)).is_err() {
                    return Err(());
                }
            }
            Err(e) => crate::log::note("inbox-reject", &format!("{e}: {}", super::clip(line, 120))),
        }
    }
    Ok(())
}
