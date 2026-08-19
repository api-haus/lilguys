//! MPRIS over D-Bus. The cheapest sense there is: a push signal that hands over a YouTube watch id.

use super::Observation;
use anyhow::Result;
use std::collections::HashMap;
use zbus::blocking::{fdo::DBusProxy, Connection, MessageIterator};
use zbus::zvariant::{OwnedValue, Value};
use zbus::MatchRule;

const PLAYER_IFACE: &str = "org.mpris.MediaPlayer2.Player";
const MPRIS_PREFIX: &str = "org.mpris.MediaPlayer2.";

/// Runs until the channel closes. Owns its own D-Bus connection on its own thread, so a stalled
/// bus can never stall the render loop.
pub fn spawn(tx: calloop::channel::Sender<Observation>) {
    std::thread::Builder::new()
        .name("mpris".into())
        .spawn(move || {
            if let Err(e) = run(&tx) {
                eprintln!("sensor mpris stopped: {e:#}");
            }
        })
        .ok();
}

fn run(tx: &calloop::channel::Sender<Observation>) -> Result<()> {
    let conn = Connection::session()?;
    let rule = MatchRule::builder()
        .msg_type(zbus::message::Type::Signal)
        .interface("org.freedesktop.DBus.Properties")?
        .member("PropertiesChanged")?
        .add_arg(PLAYER_IFACE)?
        .build();

    let dbus = DBusProxy::new(&conn)?;
    let mut owners = well_known_names(&dbus);
    let mut last: HashMap<String, String> = HashMap::new();
    let mut playing_now: HashMap<String, bool> = HashMap::new();

    for msg in MessageIterator::for_match_rule(rule, &conn, None)? {
        let debug = std::env::var_os("LILGUYS_DEBUG_MPRIS").is_some();
        let msg = match msg {
            Ok(m) => m,
            Err(e) => {
                if debug { eprintln!("mpris: iterator error {e}"); }
                continue;
            }
        };
        if debug { eprintln!("mpris: signal from {:?}", msg.header().sender()); }
        let sender = msg.header().sender().map(|s| s.to_string()).unwrap_or_default();
        let player = match owners.get(&sender) {
            Some(name) => name.clone(),
            // A player that started after us; one relist is cheaper than tracking NameOwnerChanged.
            None => {
                owners = well_known_names(&dbus);
                owners.get(&sender).cloned().unwrap_or_else(|| sender.clone())
            }
        };

        let Ok((_iface, changed, _invalidated)) =
            msg.body().deserialize::<(String, HashMap<String, OwnedValue>, Vec<String>)>()
        else {
            if debug { eprintln!("mpris: body deserialize failed: {:?}", msg.body().signature()); }
            continue;
        };
        if debug { eprintln!("mpris: keys {:?}", changed.keys().collect::<Vec<_>>()); }

        // Metadata and PlaybackStatus arrive in separate signals, so the last known state carries.
        if let Some(status) = changed.get("PlaybackStatus").and_then(as_str) {
            playing_now.insert(player.clone(), status == "Playing");
        }
        let playing = playing_now.get(&player).copied().unwrap_or(true);
        let Some(meta) = changed.get("Metadata") else { continue };
        let meta = match <HashMap<String, OwnedValue>>::try_from(meta.clone()) {
            Ok(m) => m,
            Err(e) => {
                if debug { eprintln!("mpris: metadata not a dict: {e}"); }
                continue;
            }
        };
        if debug { eprintln!("mpris: meta {:?}", meta.keys().collect::<Vec<_>>()); }

        let title = meta.get("xesam:title").and_then(as_str).unwrap_or_default();
        let artist = meta.get("xesam:artist").and_then(first_str).unwrap_or_default();
        let url = meta.get("xesam:url").and_then(as_str).unwrap_or_default();
        if debug { eprintln!("mpris: title={title:?} artist={artist:?} url={url:?} playing={playing}"); }
        if title.is_empty() && url.is_empty() {
            continue;
        }

        // Browsers re-emit the same metadata on every seek and buffer.
        let stamp = format!("{url}|{title}|{playing}");
        if last.get(&player) == Some(&stamp) {
            continue;
        }
        last.insert(player.clone(), stamp);

        if tx
            .send(Observation::Media {
                player: player.trim_start_matches(MPRIS_PREFIX).to_string(),
                title,
                artist,
                url,
                playing,
            })
            .is_err()
        {
            return Ok(());
        }
    }
    Ok(())
}

/// Unique bus name to `org.mpris.MediaPlayer2.<player>`, so the log names a player not a `:1.124`.
fn well_known_names(dbus: &DBusProxy<'_>) -> HashMap<String, String> {
    let mut map = HashMap::new();
    let Ok(names) = dbus.list_names() else { return map };
    for name in names {
        let name = name.to_string();
        if !name.starts_with(MPRIS_PREFIX) {
            continue;
        }
        if let Ok(owner) = dbus.get_name_owner(name.as_str().try_into().unwrap()) {
            map.insert(owner.to_string(), name);
        }
    }
    map
}

// `OwnedValue` derefs to `Value`; downcasting it instead yields nothing and drops every field.
fn as_str(v: &OwnedValue) -> Option<String> {
    match &**v {
        Value::Str(s) => Some(s.to_string()),
        _ => None,
    }
}

fn first_str(v: &OwnedValue) -> Option<String> {
    match &**v {
        Value::Str(s) => Some(s.to_string()),
        Value::Array(a) => a.iter().find_map(|x| match x {
            Value::Str(s) => Some(s.to_string()),
            _ => None,
        }),
        _ => None,
    }
}
