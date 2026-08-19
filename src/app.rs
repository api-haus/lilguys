//! The layer surface, the tick, and the input region that decides what passes through.

use crate::attention::{Attention, Verdict};
use crate::avatar::{graybox::Graybox, Avatar, Drive, Emotion, Param, Pose};
use crate::config::Config;
use crate::mind::capability::{FocusTarget, Intent};
use crate::mind::{Quantiser, Reaction, ToMind};
use crate::gpu::painter::{rgba, Painter};
use crate::gpu::Gpu;
use crate::hypr::Hypr;
use crate::locomotion::{Body, Drift, Target};
use crate::sensors::wayland::Sensors;
use crate::sensors::Observation;
use crate::voice::Voice;
use smithay_client_toolkit::{
    compositor::{CompositorHandler, CompositorState, Region},
    delegate_dispatch2, delegate_registry,
    output::{OutputHandler, OutputState},
    registry::{ProvidesRegistryState, RegistryState},
    registry_handlers,
    seat::{
        pointer::{PointerEvent, PointerEventKind, PointerHandler},
        Capability, SeatHandler, SeatState,
    },
    shell::{
        wlr_layer::{LayerShellHandler, LayerSurface, LayerSurfaceConfigure},
        WaylandSurface,
    },
};
use std::time::Instant;
use wayland_client::{
    protocol::{wl_output, wl_pointer, wl_seat, wl_surface},
    Connection, QueueHandle,
};

/// Tick rate while something is moving. Idle drops to `IDLE_HZ` — that gap is the whole economy
/// story, so it is measured in the HUD rather than assumed.
const ACTIVE_HZ: f32 = 60.0;
const IDLE_HZ: f32 = 8.0;

pub struct App {
    pub registry_state: RegistryState,
    pub seat_state: SeatState,
    pub output_state: OutputState,
    pub compositor: CompositorState,
    pub layer: LayerSurface,
    pub gpu: Option<Gpu>,
    pub painter: Painter,
    pub hypr: Hypr,
    pub sensors: Sensors,
    pub attention: Attention,
    pub config: Config,
    pub quantiser: Quantiser,
    pub voice: Voice,
    to_mind: Option<std::sync::mpsc::Sender<ToMind>>,
    last_reaction: Option<Reaction>,
    thought: Option<(Instant, f32, String)>,
    mind_error: Option<String>,
    turns: u32,

    pub width: u32,
    pub height: u32,
    pub exit: bool,
    configured: bool,

    pointer: Option<wl_pointer::WlPointer>,
    hover: bool,
    press_origin: Option<[f32; 2]>,

    avatar: Box<dyn Avatar>,
    pose: Pose,
    pub body: Body,
    last_tick: Instant,
    frame_ms: f32,
    frames: u32,
    fps: f32,
    fps_window: Instant,
    input_region: Option<[i32; 4]>,
    pub show_hud: bool,
}

impl App {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        registry_state: RegistryState,
        seat_state: SeatState,
        output_state: OutputState,
        compositor: CompositorState,
        layer: LayerSurface,
        painter: Painter,
        hypr: Hypr,
        sensors: Sensors,
        config: Config,
        voice: Voice,
        to_mind: Option<std::sync::mpsc::Sender<ToMind>>,
    ) -> Self {
        let motion = config.motion.clone();
        let size = config.buddy.size;
        let debug = config.debug.clone();
        Self {
            registry_state,
            seat_state,
            output_state,
            compositor,
            layer,
            gpu: None,
            painter,
            hypr,
            sensors,
            attention: Attention::new((&config).into()),
            quantiser: Quantiser::default(),
            voice,
            to_mind,
            last_reaction: None,
            thought: None,
            mind_error: None,
            turns: 0,
            config,
            width: 0,
            height: 0,
            exit: false,
            configured: false,
            pointer: None,
            hover: false,
            press_origin: None,
            avatar: Box::new(Graybox::annotated(debug.rig)),
            pose: Pose::default(),
            body: Body::new(motion, size, [1.0, 1.0]),
            last_tick: Instant::now(),
            frame_ms: 0.0,
            frames: 0,
            fps: 0.0,
            fps_window: Instant::now(),
            input_region: None,
            show_hud: debug.hud,
        }
    }

    /// Seconds until the next tick. Falls to `IDLE_HZ` when nothing is animating.
    pub fn tick_interval(&self) -> f32 {
        let busy = self.thought.is_some()
            || self.body.moving()
            || self.body.drift != Drift::Idle
            || self.body.gesture().is_some()
            || self.body.speaking
            || self.hover
            || self.press_origin.is_some();
        1.0 / if busy { ACTIVE_HZ } else { IDLE_HZ }
    }

    pub fn tick(&mut self) {
        if !self.configured || self.gpu.is_none() {
            return;
        }
        let now = Instant::now();
        let dt = (now - self.last_tick).as_secs_f32().min(0.1);
        self.last_tick = now;

        self.sense(now);

        let cursor = self.hypr.cursor_pos().map(|(x, y)| [x, y]).unwrap_or([0.0, 0.0]);
        if self.press_origin.is_some() {
            self.body.drag_to(cursor[0], cursor[1]);
        }
        self.body.update(cursor, dt, &mut self.pose);

        let drive = Drive {
            speed: self.body.speed(),
            heading: self.body.heading(),
            bob_phase: self.body.bob_phase(),
            gesture: self.body.gesture(),
            speaking: self.body.speaking,
        };
        self.avatar.advance(&self.pose, &drive, dt);

        self.paint(cursor);
        self.sync_input_region();

        let started = Instant::now();
        if let Some(gpu) = self.gpu.as_mut() {
            if let Err(e) = gpu.render(&mut self.painter) {
                eprintln!("render: {e}");
            }
        }
        self.frame_ms = started.elapsed().as_secs_f32() * 1000.0;

        self.frames += 1;
        let win = self.fps_window.elapsed().as_secs_f32();
        if win >= 0.5 {
            self.fps = self.frames as f32 / win;
            self.frames = 0;
            self.fps_window = Instant::now();
        }
    }

    /// Drains the sensor bus through the gate, then buckets what survived for the model.
    fn sense(&mut self, now: Instant) {
        for feeling in self.body.drain_feelings() {
            self.sensors.bus.push(Observation::Feeling(feeling));
        }
        let sensed = self.sensors.bus.drain();
        if !sensed.is_empty() || self.attention.waiting() > 0 {
            for (verdict, what) in self.attention.consider(sensed, now) {
                // Model turns are minutes apart and capped, so the gate answers locally in the gap.
                // A record of a reflex is exempt: it reaches the mind, but reacting to it would
                // only produce another record of reacting to it.
                let already_reflexive =
                    matches!(&what, Observation::Feeling(f) if f.reflective);
                if verdict == Verdict::Emote && !already_reflexive {
                    let (emotion, intensity, hold) = reflex(&what);
                    self.body.reflex(emotion, intensity, hold, &what.summary());
                }
                self.quantiser.observe(what.summary());
            }
            for e in self.attention.log.iter().take(self.attention.fresh()) {
                println!("[{}] {}", e.verdict.tag(), e.text);
                crate::log::event(e.verdict.tag(), &e.text);
            }
        }

        self.quantiser.present = self.sensors.present;
        self.quantiser.focused = self.sensors.focused_title();
        self.quantiser.workspace = self.sensors.workspace_name();
        self.quantiser.sensation = self.body.sensation();
        if let Some(tx) = self.to_mind.as_ref() {
            if let Some(q) = self.quantiser.take(&self.config.mind, now) {
                let _ = tx.send(ToMind::Tick(q));
            }
        }
    }

    /// Enacts what the model decided. Nothing here talks to the model; it only obeys.
    pub fn enact(&mut self, reaction: Reaction) {
        self.turns += 1;
        self.mind_error = reaction.error.clone();
        for intent in &reaction.intents {
            println!("[*] {}", intent.summary());
            match intent {
                Intent::React { emotion, intensity, hold } => {
                    self.body.feel(*emotion, *intensity, *hold)
                }
                Intent::Think { text, hold } => {
                    self.thought = Some((Instant::now(), *hold, text.clone()))
                }
                Intent::Gesture { gesture } => self.body.perform(*gesture),
                Intent::Speak { text } => {
                    if !self.voice.say(text) {
                        println!("    (voice unavailable or busy)");
                    }
                }
                Intent::Focus { target, linger } => {
                    if let Some(t) = self.resolve(target, *linger) {
                        self.body.go_to(t);
                    }
                }
            }
        }
        for why in &reaction.rejected {
            println!("[x] refused {why}");
        }
        if let Some(note) = reaction.note.as_deref() {
            println!("[.] note: {}", crate::sensors::clip(note, 70));
        }
        self.last_reaction = Some(reaction);
    }

    /// Turns a named target into a point. Window geometry is the one thing no Wayland protocol
    /// exposes, so this is where `hyprctl clients` earns its place.
    fn resolve(&mut self, target: &FocusTarget, linger: f32) -> Option<Target> {
        let (w, h) = (self.width as f32, self.height as f32);
        match target {
            FocusTarget::Pointer => {
                let c = self.hypr.cursor_pos().ok()?;
                let side = if c.0 > w * 0.5 { -1.0 } else { 1.0 };
                Some(Target {
                    at: [c.0 + side * self.config.motion.personal_space, c.1],
                    linger,
                    label: "the pointer".into(),
                })
            }
            FocusTarget::Place { where_ } => {
                let f = where_.fraction();
                Some(Target { at: [f[0] * w, f[1] * h], linger, label: format!("{where_:?}") })
            }
            FocusTarget::Away => {
                let side = if self.body.pos[0] > w * 0.5 { 1.0 } else { -1.0 };
                Some(Target {
                    at: [w * 0.5 + side * w * 0.9, self.body.pos[1]],
                    linger: -1.0,
                    label: "away".into(),
                })
            }
            FocusTarget::Window { r#match } => {
                let (rect, name) = self.hypr.window_rect(r#match)?;
                Some(Target {
                    // Beside the window's top edge, not on top of what the user is reading.
                    at: [rect[0] + rect[2] * 0.5, rect[1] + self.config.buddy.size * 0.45],
                    linger,
                    label: name,
                })
            }
        }
    }

    fn scale(&self) -> f32 {
        self.config.buddy.size
    }

    fn paint(&mut self, cursor: [f32; 2]) {
        self.painter.clear();
        let origin = self.body.pos;
        let (facing, scale) = (self.body.facing, self.scale());
        let avatar =
            std::mem::replace(&mut self.avatar, Box::new(Graybox::annotated(self.config.debug.rig)));
        avatar.draw(&mut self.painter, origin, facing, scale);
        self.avatar = avatar;
        self.overhead(origin);
        if self.show_hud {
            self.hud(cursor, origin);
            self.sense_hud();
        }
    }

    fn hud(&mut self, cursor: [f32; 2], origin: [f32; 2]) {
        let t = self.config.motion.clone();
        let ink = rgba(255, 255, 255, 0.85);
        let dim = rgba(160, 170, 190, 0.75);
        let hot = rgba(250, 214, 82, 0.95);

        // Awareness radii, so a tuning change is visible instead of felt.
        let eye_y = origin[1] - self.config.buddy.size * 0.28;
        for (r, c) in if self.config.debug.radii { [
            (t.personal_space, rgba(226, 86, 90, 0.30)),
            (t.notice_radius, rgba(104, 168, 128, 0.22)),
            (t.forget_radius, rgba(96, 124, 176, 0.18)),
        ] } else { [(0.0, [0.0; 4]); 3] } {
            if r > 0.0 {
                ring(&mut self.painter, origin[0], eye_y, r, c);
            }
        }
        if self.config.debug.radii {
            self.painter.line([origin[0], eye_y], cursor, 1.0, rgba(250, 214, 82, 0.35));
        }

        let (x, mut y) = (origin[0] + 150.0, origin[1] - self.config.buddy.size * 0.95);
        let line_h = 15.0;
        let head = format!("{}  ·  {}", self.avatar.name(), self.body.drift.name());
        self.painter.rrect(x + 96.0, y + 96.0, 210.0, 250.0, 8.0, rgba(12, 14, 20, 0.72));
        self.painter.text(x, y, 14.0, hot, &head);
        y += line_h + 4.0;

        for row in [
            format!("fps      {:>6.1}", self.fps),
            format!("frame    {:>6.2} ms", self.frame_ms),
            format!("tick     {:>6.0} hz", 1.0 / self.tick_interval()),
            format!("cursor   {:>6.0} px", self.body.cursor_dist),
            format!("vel      {:>6.0} px/s", self.body.speed()),
            format!("quads    {:>6}", self.painter.quads.len()),
        ] {
            self.painter.text(x, y, 12.0, dim, &row);
            y += line_h;
        }
        y += 6.0;

        for p in [
            Param::HeadYaw,
            Param::HeadPitch,
            Param::HeadRoll,
            Param::GazeX,
            Param::GazeY,
            Param::EyeOpenL,
            Param::BrowL,
            Param::Breath,
            Param::BodyYaw,
        ] {
            let v = self.pose.get(p);
            self.painter.text(x, y, 11.0, dim, p.name());
            let bx = x + 78.0;
            self.painter.rect(bx + 45.0, y - 3.5, 90.0, 3.0, rgba(255, 255, 255, 0.12));
            let w = (v.abs() * 45.0).min(45.0);
            let cx = if v >= 0.0 { bx + 45.0 + w * 0.5 } else { bx + 45.0 - w * 0.5 };
            self.painter.rect(cx, y - 3.5, w.max(1.0), 3.0, if v >= 0.0 { ink } else { hot });
            y += 13.0;
        }
    }

    /// A thought bubble and, optionally, a few recent gate rulings — both floating with him rather
    /// than covering the screen. Above his head, flipped below when there is no room up there.
    fn overhead(&mut self, origin: [f32; 2]) {
        let size = self.config.buddy.size;
        let mut y = origin[1] - size * 1.06;
        let below = y < 90.0;
        if below {
            y = origin[1] + size * 0.14;
        }
        let line_h = 13.0;

        if self.config.debug.overhead > 0 {
            let dim = rgba(150, 158, 176, 0.55);
            let hot = rgba(250, 214, 82, 0.8);
            let rows: Vec<(String, [f32; 4])> = self
                .attention
                .log
                .iter()
                .take(self.config.debug.overhead)
                .map(|e| {
                    let hue = if matches!(e.verdict, Verdict::Think | Verdict::Emote) { hot } else { dim };
                    (format!("{} {}", e.verdict.tag(), crate::sensors::clip(&e.text, 44)), hue)
                })
                .collect();
            // Oldest at the top, so new lines push upward away from him.
            let block = rows.len() as f32 * line_h;
            let top = if below { y } else { y - block };
            for (i, (text, color)) in rows.iter().rev().enumerate() {
                let ly = top + i as f32 * line_h;
                let w = self.painter.text_width(10.0, text);
                // Centred on him and opaque enough to read over a bright window underneath.
                self.painter.rrect(origin[0], ly - 3.0, w + 12.0, 13.0, 3.0, rgba(12, 14, 20, 0.78));
                self.painter.text(origin[0] - w * 0.5, ly, 10.0, *color, text);
            }
            y = if below { y + block + 6.0 } else { top - 8.0 };
        }

        let Some((at, hold, text)) = self.thought.clone() else { return };
        let age = at.elapsed().as_secs_f32();
        if age > hold {
            self.thought = None;
            return;
        }
        // Fades over the last second so it does not simply vanish mid-read.
        let alpha = ((hold - age) / 1.0).clamp(0.0, 1.0);

        let max_w = 260.0;
        let lines = self.painter.wrap(13.0, max_w, &text);
        let w = lines.iter().map(|l| self.painter.text_width(13.0, l)).fold(0.0, f32::max);
        let h = lines.len() as f32 * 17.0;
        let (bx, by) = (origin[0], if below { y + h * 0.5 } else { y - h * 0.5 });

        self.painter.rrect(bx, by, w + 26.0, h + 20.0, 9.0, rgba(20, 22, 30, 0.82 * alpha));
        self.painter.rrect(bx, by, w + 26.0, h + 20.0, 9.0, rgba(0, 0, 0, 0.0));
        // Two shrinking dots for the tail, the way a thought bubble is drawn.
        let dir = if below { -1.0 } else { 1.0 };
        for (i, r) in [(1.0, 5.0), (2.0, 3.0)] {
            let dy = by + dir * (h * 0.5 + 12.0 + i * 9.0);
            self.painter.ellipse(bx + i * 5.0, dy, r * 2.0, r * 2.0, rgba(20, 22, 30, 0.82 * alpha));
        }
        for (i, line) in lines.iter().enumerate() {
            self.painter.text(
                bx - w * 0.5,
                by - h * 0.5 + 13.0 + i as f32 * 17.0,
                13.0,
                rgba(232, 236, 245, alpha),
                line,
            );
        }
    }

    /// The sensing panel. Grayboxing the senses matters as much as grayboxing the rig: without it
    /// an economy claim is a belief.
    fn sense_hud(&mut self) {
        let ink = rgba(255, 255, 255, 0.85);
        let dim = rgba(160, 170, 190, 0.75);
        let hot = rgba(250, 214, 82, 0.95);
        let (x, mut y) = (28.0, 40.0);

        self.painter.rrect(x + 186.0, y + 118.0, 420.0, 300.0, 8.0, rgba(12, 14, 20, 0.72));
        self.painter.text(x, y, 14.0, hot, "senses");
        y += 20.0;

        let [ignored, pending, emote, think] = self.attention.counts;
        for row in [
            format!("present  {}", if self.sensors.present { "yes" } else { "away" }),
            format!("gate     {ignored} ignored  {pending} pending"),
            format!("acted    {emote} emote  {think} think"),
            format!("waiting  {}", self.attention.waiting()),
            format!("credit   {:.2} thoughts", self.attention.thought_credit()),
            format!("quantum  {} queued  {:.0}s", self.quantiser.pending(), self.quantiser.since_last().as_secs_f32()),
            format!("mind     {} turns  {} tok", self.turns, self.last_reaction.as_ref().map(|r| r.tokens).unwrap_or(0)),
            match self.mind_error.as_deref() {
                Some(e) => format!("error    {}", crate::sensors::clip(e, 30)),
                None => format!("voice    {}", if self.voice.available() { "ready" } else { "off" }),
            },
        ] {
            self.painter.text(x, y, 12.0, dim, &row);
            y += 15.0;
        }
        y += 8.0;

        let entries: Vec<(String, [f32; 4])> = self
            .attention
            .log
            .iter()
            .map(|e| {
                let color = match e.verdict {
                    Verdict::Think => hot,
                    Verdict::Emote => ink,
                    _ => rgba(150, 158, 176, 0.5),
                };
                (format!("{} {:>4.0}s  {}", e.verdict.tag(), e.at.elapsed().as_secs_f32(), e.text), color)
            })
            .collect();
        for (text, color) in entries {
            self.painter.text(x, y, 11.0, color, &text);
            y += 14.0;
        }
    }

    /// The click-through contract: everything outside this rectangle belongs to whatever is below.
    fn sync_input_region(&mut self) {
        let b = self.avatar.bounds();
        let s = self.scale();
        let (o, f) = (self.body.pos, self.body.facing);
        let l = if f < 0.0 { -b.right } else { b.left };
        let rect = [
            (o[0] + l * s).floor() as i32,
            (o[1] + b.top * s).floor() as i32,
            (b.width() * s).ceil() as i32,
            (b.height() * s).ceil() as i32,
        ];
        if self.input_region == Some(rect) {
            return;
        }
        self.input_region = Some(rect);
        if std::env::var_os("LILGUYS_DEBUG_INPUT").is_some() {
            println!("input region -> x{} y{} w{} h{}", rect[0], rect[1], rect[2], rect[3]);
        }
        if let Ok(region) = Region::new(&self.compositor) {
            region.add(rect[0], rect[1], rect[2], rect[3]);
            self.layer.wl_surface().set_input_region(Some(region.wl_region()));
        }
    }
}

/// The reflex arc: an observation the gate let through, answered without a model.
///
/// Facial only. A gesture is a deliberate intention and stays the mind's to decide, so the worst
/// this layer can do while the model is unreachable is pull a face.
fn reflex(what: &Observation) -> (Emotion, f32, f32) {
    match what {
        // A feeling names its own tone, because only its author knows what it means.
        Observation::Feeling(f) => (
            f.tone.as_deref().and_then(Emotion::from_name).unwrap_or(Emotion::Curious),
            f.intensity.clamp(0.0, 1.0),
            f.hold.clamp(0.5, 120.0),
        ),
        Observation::Presence { present: true } => (Emotion::Pleased, 0.55, 6.0),
        Observation::Presence { present: false } => (Emotion::Sleepy, 0.7, 30.0),
        Observation::Workspace { .. } => (Emotion::Curious, 0.30, 3.0),
        Observation::Focus { .. } => (Emotion::Curious, 0.45, 5.0),
        // A retitle is the same window saying something changed — a glance, not a look.
        Observation::Title { .. } => (Emotion::Curious, 0.22, 2.5),
        Observation::Media { playing: true, .. } => (Emotion::Amused, 0.45, 8.0),
        Observation::Media { playing: false, .. } => (Emotion::Neutral, 0.2, 3.0),
    }
}

fn ring(p: &mut Painter, cx: f32, cy: f32, r: f32, color: [f32; 4]) {
    const SEGMENTS: usize = 48;
    let mut prev = [cx + r, cy];
    for i in 1..=SEGMENTS {
        let a = i as f32 / SEGMENTS as f32 * std::f32::consts::TAU;
        let next = [cx + r * a.cos(), cy + r * a.sin()];
        p.line(prev, next, 1.0, color);
        prev = next;
    }
}

impl CompositorHandler for App {
    fn scale_factor_changed(
        &mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: i32,
    ) {
    }
    fn transform_changed(
        &mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface,
        _: wl_output::Transform,
    ) {
    }
    fn frame(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: u32) {}
    fn surface_enter(
        &mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface,
        _: &wl_output::WlOutput,
    ) {
    }
    fn surface_leave(
        &mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface,
        _: &wl_output::WlOutput,
    ) {
    }
}

impl OutputHandler for App {
    fn output_state(&mut self) -> &mut OutputState {
        &mut self.output_state
    }
    fn new_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
    fn update_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
    fn output_destroyed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
}

impl LayerShellHandler for App {
    fn closed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &LayerSurface) {
        self.exit = true;
    }

    fn configure(
        &mut self, conn: &Connection, _: &QueueHandle<Self>, _: &LayerSurface,
        configure: LayerSurfaceConfigure, _serial: u32,
    ) {
        let (w, h) = configure.new_size;
        if w == 0 || h == 0 {
            return;
        }
        self.width = w;
        self.height = h;

        match self.gpu.as_mut() {
            Some(gpu) => {
                gpu.resize(w, h);
                self.body.bounds = [w as f32, h as f32];
            }
            None => match Gpu::new(conn, self.layer.wl_surface(), w, h) {
                Ok(gpu) => {
                    self.gpu = Some(gpu);
                    self.body = Body::new(
                        self.config.motion.clone(),
                        self.config.buddy.size,
                        [w as f32, h as f32],
                    );
                }
                Err(e) => {
                    eprintln!("gpu init failed: {e:#}");
                    self.exit = true;
                }
            },
        }
        self.configured = true;
    }
}

impl SeatHandler for App {
    fn seat_state(&mut self) -> &mut SeatState {
        &mut self.seat_state
    }
    fn new_seat(&mut self, _: &Connection, qh: &QueueHandle<Self>, seat: wl_seat::WlSeat) {
        self.sensors.arm_idle(&seat, qh);
    }

    fn new_capability(
        &mut self, _: &Connection, qh: &QueueHandle<Self>, seat: wl_seat::WlSeat,
        capability: Capability,
    ) {
        self.sensors.arm_idle(&seat, qh);
        if capability == Capability::Pointer && self.pointer.is_none() {
            match self.seat_state.get_pointer(qh, &seat) {
                Ok(p) => self.pointer = Some(p),
                Err(e) => eprintln!("pointer: {e}"),
            }
        }
    }

    fn remove_capability(
        &mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat,
        capability: Capability,
    ) {
        if capability == Capability::Pointer {
            if let Some(p) = self.pointer.take() {
                p.release();
            }
        }
    }

    fn remove_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat) {}
}

impl PointerHandler for App {
    fn pointer_frame(
        &mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_pointer::WlPointer,
        events: &[PointerEvent],
    ) {
        for event in events {
            if &event.surface != self.layer.wl_surface() {
                continue;
            }
            // The input region is a rectangle, so a click inside it may still miss him. Asking
            // the avatar keeps the silhouette authoritative even when the region cannot be.
            let local = [
                (event.position.0 as f32 - self.body.pos[0]) / self.scale() * self.body.facing,
                (event.position.1 as f32 - self.body.pos[1]) / self.scale(),
            ];
            if matches!(event.kind, PointerEventKind::Press { .. }) && !self.avatar.hit(local) {
                continue;
            }
            match event.kind {
                PointerEventKind::Enter { .. } => self.hover = true,
                PointerEventKind::Leave { .. } => self.hover = false,
                PointerEventKind::Press { .. } => {
                    if std::env::var_os("LILGUYS_DEBUG_INPUT").is_some() {
                        println!("press at {:.0},{:.0}", event.position.0, event.position.1);
                    }
                    self.press_origin = Some([event.position.0 as f32, event.position.1 as f32]);
                    self.body.grab();
                }
                PointerEventKind::Release { .. } => {
                    // A release with no matching press is a leftover from before we mapped.
                    if self.press_origin.take().is_some() {
                        self.body.release();
                        println!("clicked — this is where the hermes channel opens");
                    }
                }
                _ => {}
            }
        }
    }
}

delegate_registry!(App);
delegate_dispatch2!(App);

impl ProvidesRegistryState for App {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.registry_state
    }
    registry_handlers![OutputState, SeatState];
}
