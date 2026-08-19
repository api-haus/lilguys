//! lilguysd — an always-on desktop buddy on a wlr-layer-shell overlay.

mod app;
mod attention;
mod avatar;
mod gpu;
mod hypr;
mod locomotion;
mod sensors;

use anyhow::{Context, Result};
use app::App;
use calloop::{timer::{TimeoutAction, Timer}, EventLoop};
use calloop_wayland_source::WaylandSource;
use gpu::painter::Painter;
use hypr::Hypr;
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

const FONT: &str = "/usr/share/fonts/noto/NotoSansMono-Regular.ttf";

fn main() -> Result<()> {
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

    let font = std::fs::read(FONT).with_context(|| format!("read font {FONT}"))?;
    let mut app = App::new(
        RegistryState::new(&globals),
        SeatState::new(&globals, &qh),
        OutputState::new(&globals, &qh),
        compositor,
        layer,
        Painter::new(&font)?,
        Hypr::from_env()?,
        conn.clone(),
        sensors::wayland::Sensors::bind(&globals, &qh),
    );

    let mut event_loop: EventLoop<App> = EventLoop::try_new()?;
    let handle = event_loop.handle();
    WaylandSource::new(conn, event_queue).insert(handle.clone())?;

    let (tx, rx) = calloop::channel::channel();
    sensors::mpris::spawn(tx);
    handle
        .insert_source(rx, |event, _, app: &mut App| {
            if let calloop::channel::Event::Msg(o) = event {
                app.sensors.bus.push(o);
            }
        })
        .map_err(|e| anyhow::anyhow!("mpris channel: {e}"))?;

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
