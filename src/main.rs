//! lilguysd — an always-on desktop buddy on a wlr-layer-shell overlay.

use anyhow::{Context, Result};
use calloop::{
    timer::{TimeoutAction, Timer},
    EventLoop,
};
use calloop_wayland_source::WaylandSource;
use lilguysd::{
    app::App, config, config::Config, doctor, gpu::painter::Painter, guy, hypr::Hypr, log, mind,
    sensors, voice, FONT,
};
use smithay_client_toolkit::{
    compositor::{CompositorState, Region},
    output::OutputState,
    registry::RegistryState,
    seat::SeatState,
    shell::{
        wlr_layer::{Anchor, KeyboardInteractivity, Layer, LayerShell},
        WaylandSurface,
    },
};
use std::time::Duration;
use wayland_client::{globals::registry_queue_init, Connection};

fn main() -> Result<()> {
    match std::env::args().nth(1).as_deref() {
        Some("--print-config") => {
            print!("{}", config::DEFAULT_TOML);
            return Ok(());
        }
        Some("--check") => return check(),
        // Whatever each guy is actually told, verbatim — including the line naming the others,
        // which the shared layers do not contain.
        Some("--print-prompt") => {
            let (cfg, from) = Config::load()?;
            let dir = from.as_deref().and_then(|p| p.parent());
            let roster = cfg.roster(dir)?;
            let names: Vec<String> = roster
                .iter()
                .map(|(g, c)| g.name.clone().unwrap_or_else(|| c.name.clone()))
                .collect();
            for (i, (_, character)) in roster.iter().enumerate() {
                let others: Vec<String> =
                    names.iter().enumerate().filter(|(j, _)| *j != i).map(|(_, n)| n.clone()).collect();
                println!("═══ {} ═══\n", names[i]);
                println!("{}\n", guy::Guy::system_prompt(&cfg, character, &names[i], &others, dir));
            }
            return Ok(());
        }
        Some(other) => anyhow::bail!("unknown argument {other}; try --check, --print-config or --print-prompt"),
        None => {}
    }

    let (cfg, from) = Config::load()?;
    match from.as_ref() {
        Some(p) => eprintln!("config: {}", p.display()),
        None => eprintln!("config: built-in defaults; `lilguysd --print-config` writes a starting point"),
    }
    match log::init(&cfg.log) {
        Some(dir) => eprintln!("log: {}", dir.display()),
        None => eprintln!("log: disabled"),
    }
    let config_dir = from.as_deref().and_then(|p| p.parent()).map(|p| p.to_path_buf());

    let conn = Connection::connect_to_env().context("no wayland display")?;
    let (globals, event_queue) = registry_queue_init(&conn)?;
    let qh = event_queue.handle();

    let compositor = CompositorState::bind(&globals, &qh).context("wl_compositor missing")?;
    let layer_shell = LayerShell::bind(&globals, &qh).context("wlr-layer-shell missing")?;

    let surface = compositor.create_surface(&qh);
    let layer =
        layer_shell.create_layer_surface(&qh, surface, Layer::Overlay, Some("lilguys"), None);
    layer.set_anchor(Anchor::all());
    layer.set_keyboard_interactivity(KeyboardInteractivity::None);
    // -1 keeps the buddy out of every other client's exclusive-zone arithmetic.
    layer.set_exclusive_zone(-1);
    // Empty until the first tick sizes it to the character. An unset region is INFINITE, which
    // would swallow every click on the desktop for as long as startup takes.
    if let Ok(region) = Region::new(&compositor) {
        layer.wl_surface().set_input_region(Some(region.wl_region()));
    }
    layer.commit();

    let mut event_loop: EventLoop<App> = EventLoop::try_new()?;
    let handle = event_loop.handle();
    let (sense_tx, sense_rx) = calloop::channel::channel();

    // Everyone on the roster gets their own mind, their own voice queue, and their own gate.
    let roster = cfg.roster(config_dir.as_deref())?;
    let all_names: Vec<String> = roster
        .iter()
        .map(|(g, c)| g.name.clone().unwrap_or_else(|| c.name.clone()))
        .collect();
    let mut guys = Vec::new();
    for (i, (entry, character)) in roster.iter().enumerate() {
        let others: Vec<String> =
            all_names.iter().enumerate().filter(|(j, _)| *j != i).map(|(_, n)| n.clone()).collect();
        let system = guy::Guy::system_prompt(&cfg, character, &character.name, &others, config_dir.as_deref());

        // One channel per guy, each closing over its own index, so a message always knows whose
        // it is without a wrapper type.
        let (voice_tx, voice_rx) = calloop::channel::channel::<voice::State>();
        handle
            .insert_source(voice_rx, move |event, _, app: &mut App| {
                if let calloop::channel::Event::Msg(state) = event {
                    if let Some(guy) = app.guys.get_mut(i) {
                        guy.body.speaking = state == voice::State::Speaking;
                        if state == voice::State::Silent {
                            guy.voice.finished_one();
                        }
                    }
                }
            })
            .map_err(|e| anyhow::anyhow!("voice channel: {e}"))?;

        let (mind_tx, mind_rx) = calloop::channel::channel::<mind::Reaction>();
        handle
            .insert_source(mind_rx, move |event, _, app: &mut App| {
                if let calloop::channel::Event::Msg(r) = event {
                    app.enact(i, r);
                }
            })
            .map_err(|e| anyhow::anyhow!("mind channel: {e}"))?;

        // A guy's own voice, then their character's, then whatever [voice] says.
        let spec = entry.voice.as_deref().or(character.voice.as_deref());
        let voice = voice::Voice::new(&cfg.voice, spec, voice_tx);
        let to_mind = match cfg.mind.enabled {
            true => {
                let provider = entry.provider.clone().unwrap_or_else(|| cfg.mind.provider.clone());
                match mind::spawn(&cfg, &provider, system, mind_tx) {
                    Ok(tx) => Some(tx),
                    Err(e) => {
                        eprintln!("{}: mind disabled: {e:#}", character.name);
                        None
                    }
                }
            }
            false => None,
        };
        guys.push(guy::Guy::new(&cfg, entry, character, [1.0, 1.0], voice, to_mind));
    }
    eprintln!(
        "roster: {}",
        guys.iter().map(|g| format!("{} ({})", g.name, g.character)).collect::<Vec<_>>().join(", ")
    );

    if cfg.senses.media {
        sensors::mpris::spawn(sense_tx.clone());
    }
    match sensors::inbox::spawn(sense_tx) {
        Ok(path) => eprintln!("inbox: {}", path.display()),
        Err(e) => eprintln!("inbox unavailable: {e}"),
    }

    let font = std::fs::read(FONT).with_context(|| format!("read font {FONT}"))?;
    let sensors = sensors::wayland::Sensors::bind(&globals, &qh, &cfg.senses);
    let mut app = App::new(
        RegistryState::new(&globals),
        SeatState::new(&globals, &qh),
        OutputState::new(&globals, &qh),
        compositor,
        layer,
        Painter::new(&font)?,
        Hypr::from_env()?,
        sensors,
        cfg,
        guys,
    );

    WaylandSource::new(conn, event_queue).insert(handle.clone())?;
    handle
        .insert_source(sense_rx, |event, _, app: &mut App| {
            if let calloop::channel::Event::Msg(o) = event {
                app.sensors.bus.push(o);
            }
        })
        .map_err(|e| anyhow::anyhow!("sense channel: {e}"))?;
    handle
        .insert_source(Timer::immediate(), |_, _, app: &mut App| {
            app.tick();
            TimeoutAction::ToDuration(Duration::from_secs_f32(app.tick_interval()))
        })
        .map_err(|e| anyhow::anyhow!("timer: {e}"))?;

    while !app.exit {
        event_loop.dispatch(Duration::from_millis(50), &mut app)?;
    }
    Ok(())
}

/// The same report `lilguy doctor` prints, so there is one set of checks and one place to fix them.
fn check() -> Result<()> {
    let report = doctor::run(&mut |c| {
        println!("{}", c.line());
        if let (Some(fix), true) = (c.fix.as_ref(), c.status != doctor::Status::Ok) {
            println!("      \u{2192} {fix}");
        }
    });
    match report.ok() {
        true => Ok(()),
        false => std::process::exit(1),
    }
}
