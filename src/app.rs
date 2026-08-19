//! The layer surface, the tick, and the input region that decides what passes through.

use crate::attention::Verdict;
use crate::avatar::{graybox::Graybox, Emotion};
use crate::config::Config;
use crate::gpu::painter::{rgba, Painter};
use crate::gpu::Gpu;
use crate::guy::Guy;
use crate::hypr::Hypr;
use crate::mind::capability::{FocusTarget, Intent};
use crate::mind::{Reaction, ToMind};
use crate::locomotion::Target;
use crate::sensors::wayland::Sensors;
use crate::sensors::Observation;
use smithay_client_toolkit::{
    compositor::{CompositorHandler, CompositorState, Region},
    delegate_dispatch2, delegate_registry,
    output::{OutputHandler, OutputState},
    registry::{ProvidesRegistryState, RegistryState},
    registry_handlers,
    seat::{
        keyboard::{KeyEvent, KeyboardHandler, Keysym, Modifiers, RawModifiers},
        pointer::{PointerEvent, PointerEventKind, PointerHandler},
        Capability, SeatHandler, SeatState,
    },
    shell::{
        wlr_layer::{
            KeyboardInteractivity, LayerShellHandler, LayerSurface, LayerSurfaceConfigure,
        },
        WaylandSurface,
    },
};
use std::time::Instant;
use wayland_client::{
    protocol::{wl_keyboard, wl_output, wl_pointer, wl_seat, wl_surface},
    Connection, QueueHandle,
};

/// Tick rate while something is moving. Idle drops to `IDLE_HZ` — that gap is the whole economy
/// story, so it is measured in the HUD rather than assumed.
const ACTIVE_HZ: f32 = 60.0;
const IDLE_HZ: f32 = 8.0;

/// A message box abandoned for this long closes itself and hands the keyboard back.
const TYPING_TIMEOUT: f32 = 45.0;
/// Longer than this is not a remark, and the box is not a text editor.
const TYPING_CAP: usize = 200;

/// A message being typed at one of them, above their head, in the bubble their thoughts use.
pub struct Typing {
    pub who: usize,
    pub text: String,
    last_key: Instant,
}

impl Typing {
    fn idle_for(&self) -> f32 {
        self.last_key.elapsed().as_secs_f32()
    }
}

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
    pub config: Config,
    /// The roster. Everything personal lives in here; everything above is shared.
    pub guys: Vec<Guy>,

    pub width: u32,
    pub height: u32,
    pub exit: bool,
    configured: bool,

    pointer: Option<wl_pointer::WlPointer>,
    keyboard: Option<wl_keyboard::WlKeyboard>,
    hover: bool,
    /// Which of them is being dragged, if any.
    held: Option<usize>,
    /// Where a press landed, so a click can be told from a drag on release.
    pressed_at: Option<[f32; 2]>,
    /// The message box, open above one of them. This is the only time the surface takes the
    /// keyboard, and it gives it back on every exit path.
    typing: Option<Typing>,
    last_tick: Instant,
    frame_ms: f32,
    frames: u32,
    fps: f32,
    fps_window: Instant,
    input_region: Option<Vec<[i32; 4]>>,
    /// When anybody last spoke aloud. One clock for the whole cast.
    last_spoke: Option<Instant>,
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
        guys: Vec<Guy>,
    ) -> Self {
        let show_hud = config.debug.hud;
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
            config,
            guys,
            width: 0,
            height: 0,
            exit: false,
            configured: false,
            pointer: None,
            keyboard: None,
            hover: false,
            held: None,
            pressed_at: None,
            typing: None,
            last_tick: Instant::now(),
            frame_ms: 0.0,
            frames: 0,
            fps: 0.0,
            fps_window: Instant::now(),
            input_region: None,
            last_spoke: None,
            show_hud,
        }
    }

    /// Seconds until the next tick. Falls to `IDLE_HZ` when nothing is animating.
    pub fn tick_interval(&self) -> f32 {
        let busy = self.hover
            || self.held.is_some()
            || self.typing.is_some()
            || self.guys.iter().any(Guy::busy);
        1.0 / if busy { ACTIVE_HZ } else { IDLE_HZ }
    }

    pub fn tick(&mut self) {
        if !self.configured || self.gpu.is_none() {
            return;
        }
        let now = Instant::now();
        let dt = (now - self.last_tick).as_secs_f32().min(0.1);
        self.last_tick = now;

        // A box left open holds the keyboard, and a keyboard held by a mistake is the worst bug
        // this feature can have. It closes itself if nobody is typing.
        if self.typing.as_ref().is_some_and(|t| t.idle_for() > TYPING_TIMEOUT) {
            self.close_box();
        }

        let cursor = self.hypr.cursor_pos().map(|(x, y)| [x, y]).unwrap_or([0.0, 0.0]);
        self.sense(now);

        if let Some(i) = self.held {
            self.guys[i].body.drag_to(cursor[0], cursor[1]);
        }
        for guy in &mut self.guys {
            guy.body.update(cursor, dt, &mut guy.pose);
            Self::express(guy, dt);
            let drive = guy.drive();
            let (pose, drive) = (guy.pose.clone(), drive);
            guy.avatar.advance(&pose, &drive, dt);
        }

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

    /// Drains the shared senses once, then lets each of them rule on the same world separately.
    fn sense(&mut self, now: Instant) {
        let shared = self.sensors.bus.drain();
        let present = self.sensors.present;
        let focused = self.sensors.focused_title();
        let workspace = self.sensors.workspace_name();

        for guy in &mut self.guys {
            for line in guy.sense(&shared, now) {
                println!("{line}");
            }
            guy.quantiser.present = present;
            guy.quantiser.focused = focused.clone();
            guy.quantiser.workspace = workspace.clone();
            guy.quantiser.sensation = guy.body.sensation();
            if let Some(tx) = guy.to_mind.as_ref() {
                if let Some(q) = guy.quantiser.take(&self.config.mind, now) {
                    let _ = tx.send(ToMind::Tick(q));
                }
            }
        }
    }

    /// Enacts what one of them decided, and lets the others see it happen.
    pub fn enact(&mut self, who: usize, reaction: Reaction) {
        if self.guys.get(who).is_none() {
            return;
        }
        self.guys[who].turns += 1;
        self.guys[who].mind_error = reaction.error.clone();

        let mut witnessed = Vec::new();
        for intent in &reaction.intents {
            let name = self.guys[who].name.clone();
            println!("[*] {name} · {}", intent.summary());
            match intent {
                Intent::React { emotion, intensity, hold } => {
                    self.guys[who].body.feel(*emotion, *intensity, *hold)
                }
                Intent::Think { text, hold } => {
                    self.guys[who].thought = Some((Instant::now(), *hold, text.clone()))
                }
                Intent::Gesture { gesture } => self.guys[who].body.perform(*gesture),
                Intent::Speak { text, .. } => {
                    if !self.floor_allows(&name) || !self.guys[who].voice.say(text) {
                        println!("    ({name} could not speak just then)");
                        continue;
                    }
                }
                Intent::Focus { target, linger } => {
                    if let Some(t) = self.resolve(who, target, *linger) {
                        self.guys[who].body.go_to(t);
                    }
                }
            }
            witnessed.push(intent.clone());
        }
        let guy = &mut self.guys[who];
        for why in &reaction.rejected {
            println!("[x] {} refused {why}", guy.name);
        }
        if let Some(note) = reaction.note.as_deref() {
            println!("[.] {} noted: {}", guy.name, crate::sensors::clip(note, 70));
        }
        guy.last_reaction = Some(reaction);

        // Nobody witnesses their own action; they already know they did it. An action that never
        // happened is witnessed by nobody either, which is why the floor is enforced above.
        for i in 0..self.guys.len() {
            if i == who {
                continue;
            }
            let listener = self.guys[i].name.clone();
            let seen: Vec<_> = witnessed
                .iter()
                .filter_map(|intent| self.guys[who].witnessed_by(intent, &listener))
                .collect();
            for o in seen {
                self.guys[i].bus.push(o);
            }
        }
    }

    /// The cast shares one floor on speech. Faces, thoughts and movement are free and are never
    /// held back — this exists so a room of six is not a room that will not shut up.
    fn floor_allows(&mut self, who: &str) -> bool {
        let floor = self.config.mind.speech_floor;
        if let Some(last) = self.last_spoke {
            if last.elapsed() < floor {
                let gap = last.elapsed().as_secs_f32();
                crate::log::note("floor", &format!("{who} held back — somebody spoke {gap:.0}s ago"));
                return false;
            }
        }
        self.last_spoke = Some(Instant::now());
        true
    }

    /// Takes the keyboard, which a layer surface otherwise never does — that is why he never
    /// steals your typing, and why every path out of here gives it back.
    ///
    /// Exclusive rather than on-demand: on-demand is granted by a click, and the click that opens
    /// this box was delivered before the request could reach the compositor, so it would take a
    /// second click to type. Every dmenu-style launcher on Wayland does the same thing.
    fn open_box(&mut self, who: usize) {
        if self.typing.is_some() {
            return;
        }
        self.layer.set_keyboard_interactivity(KeyboardInteractivity::Exclusive);
        self.layer.commit();
        self.guys[who].body.listened_to();
        self.typing = Some(Typing { who, text: String::new(), last_key: Instant::now() });
    }

    fn close_box(&mut self) {
        if self.typing.take().is_none() {
            return;
        }
        self.layer.set_keyboard_interactivity(KeyboardInteractivity::None);
        self.layer.commit();
    }

    /// The typed line enters the stream as an ordinary observation, addressed to whoever it was
    /// typed at. Everybody in the room still hears it.
    fn send_typed(&mut self) {
        let Some(typing) = self.typing.take() else { return };
        self.layer.set_keyboard_interactivity(KeyboardInteractivity::None);
        self.layer.commit();
        let text = typing.text.trim().to_string();
        if text.is_empty() {
            return;
        }
        let to = self.guys.get(typing.who).map(|g| g.name.clone());
        self.sensors.bus.push(Observation::Told { text, to });
    }

    /// A surviving observation reads on the face, never as speech.
    fn express(guy: &mut Guy, dt: f32) {
        let Some((at, hold, _)) = guy.thought.as_ref() else { return };
        if at.elapsed().as_secs_f32() > *hold {
            guy.thought = None;
        }
        let _ = dt;
    }

    /// Turns a named target into a point. Window geometry is the one thing no Wayland protocol
    /// exposes, so this is where `hyprctl clients` earns its place.
    fn resolve(&mut self, who: usize, target: &FocusTarget, linger: f32) -> Option<Target> {
        let (w, h) = (self.width as f32, self.height as f32);
        let size = self.guys[who].size;
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
                let side = if self.guys[who].body.pos[0] > w * 0.5 { 1.0 } else { -1.0 };
                Some(Target {
                    at: [w * 0.5 + side * w * 0.9, self.guys[who].body.pos[1]],
                    linger: -1.0,
                    label: "away".into(),
                })
            }
            FocusTarget::Window { r#match } => {
                let (rect, name) = self.hypr.window_rect(r#match)?;
                Some(Target {
                    at: [rect[0] + rect[2] * 0.5, rect[1] + size * 0.45],
                    linger,
                    label: name,
                })
            }
        }
    }

    fn paint(&mut self, cursor: [f32; 2]) {
        self.painter.clear();
        for i in 0..self.guys.len() {
            let (origin, facing, scale) =
                (self.guys[i].body.pos, self.guys[i].body.facing, self.guys[i].size);
            let mut avatar =
                std::mem::replace(&mut self.guys[i].avatar, Box::new(Graybox::annotated(false)));
            avatar.set_palette(self.guys[i].skin);
            avatar.draw(&mut self.painter, origin, facing, scale);
            self.guys[i].avatar = avatar;
            self.overhead(i);
        }
        if self.config.debug.radii {
            self.radii(cursor);
        }
        if self.show_hud {
            self.hud(cursor);
        }
    }

    /// A thought bubble and, optionally, a few recent gate rulings — both floating with him rather
    /// than covering the screen. Above his head, flipped below when there is no room up there.
    fn overhead(&mut self, who: usize) {
        let origin = self.guys[who].body.pos;
        let rig = self.guys[who].size;
        let mut y = origin[1] - rig * 1.06;
        let below = y < 90.0;
        if below {
            y = origin[1] + rig * 0.14;
        }
        let line_h = 13.0;

        if self.config.debug.overhead > 0 {
            let dim = rgba(150, 158, 176, 0.55);
            let hot = rgba(250, 214, 82, 0.8);
            let rows: Vec<(String, [f32; 4])> = self.guys[who]
                .attention
                .log
                .iter()
                .take(self.config.debug.overhead)
                .map(|e| {
                    let hue =
                        if matches!(e.verdict, Verdict::Think | Verdict::Emote) { hot } else { dim };
                    (format!("{} {}", e.verdict.tag(), crate::sensors::clip(&e.text, 44)), hue)
                })
                .collect();
            let block = rows.len() as f32 * line_h;
            let top = if below { y } else { y - block };
            for (i, (text, color)) in rows.iter().rev().enumerate() {
                let ly = top + i as f32 * line_h;
                let w = self.painter.text_width(10.0, text);
                self.painter.rrect(origin[0], ly - 3.0, w + 12.0, 13.0, 3.0, rgba(12, 14, 20, 0.78));
                self.painter.text(origin[0] - w * 0.5, ly, 10.0, *color, text);
            }
            y = if below { y + block + 6.0 } else { top - 8.0 };
        }

        // A message being typed uses the bubble his thoughts use. Typing where his thinking
        // appears reads as talking *to* him; a dialog box reads as configuring him.
        let box_open = self.typing.as_ref().filter(|t| t.who == who).map(|t| {
            let caret = if t.idle_for() % 1.0 < 0.5 { "|" } else { " " };
            format!("{}{caret}", t.text)
        });
        let (text, alpha, tint) = match box_open {
            Some(typed) => (typed, 1.0, rgba(30, 40, 62, 0.92)),
            None => {
                let Some((at, hold, text)) = self.guys[who].thought.clone() else { return };
                let age = at.elapsed().as_secs_f32();
                if age > hold {
                    return;
                }
                let alpha = ((hold - age) / 1.0).clamp(0.0, 1.0);
                (text, alpha, rgba(20, 22, 30, 0.82 * alpha))
            }
        };

        let lines = self.painter.wrap(13.0, 260.0, &text);
        let w = lines.iter().map(|l| self.painter.text_width(13.0, l)).fold(0.0, f32::max);
        let h = lines.len() as f32 * 17.0;
        let (bx, by) = (origin[0], if below { y + h * 0.5 } else { y - h * 0.5 });

        let ink = tint;
        self.painter.rrect(bx, by, w + 26.0, h + 20.0, 9.0, ink);

        // The tail is a run of shrinking bubbles from the thinker's head to the balloon, smallest
        // at the head, so it stays attached however far the balloon is pushed up.
        let bounds = self.guys[who].avatar.bounds();
        let facing = self.guys[who].body.facing;
        let crown = [origin[0] + facing * bounds.left * 0.35 * rig, origin[1] + bounds.top * rig];
        let mouth = by + if below { -(h * 0.5 + 10.0) } else { h * 0.5 + 10.0 };
        let span = mouth - crown[1];
        for (t, r) in [(0.30_f32, 2.5_f32), (0.58, 4.0), (0.84, 5.5)] {
            let bow = (t * std::f32::consts::PI).sin() * 7.0 * facing;
            self.painter.ellipse(
                crown[0] + (bx - crown[0]) * t + bow,
                crown[1] + span * t,
                r * 2.0,
                r * 2.0,
                ink,
            );
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

    /// The awareness numbers in `[motion]`, drawn where they act: what earns a look, what stops
    /// earning one, and how close he will come.
    fn radii(&mut self, cursor: [f32; 2]) {
        let m = self.config.motion.clone();
        for i in 0..self.guys.len() {
            let p = self.guys[i].body.pos;
            ring(&mut self.painter, p[0], p[1], m.notice_radius, rgba(250, 214, 82, 0.35));
            ring(&mut self.painter, p[0], p[1], m.forget_radius, rgba(150, 158, 176, 0.22));
            ring(&mut self.painter, p[0], p[1], m.personal_space, rgba(240, 120, 120, 0.30));
            self.painter.line(p, cursor, 1.0, rgba(250, 214, 82, 0.25));
        }
    }

    fn hud(&mut self, cursor: [f32; 2]) {
        let ink = rgba(255, 255, 255, 0.85);
        let dim = rgba(160, 170, 190, 0.75);
        let hot = rgba(250, 214, 82, 0.95);
        let (x, mut y) = (28.0, 40.0);

        self.painter.rrect(x + 200.0, y + 100.0, 450.0, 280.0, 8.0, rgba(12, 14, 20, 0.72));
        let head = format!("{} on screen · {:.0} fps · {:.2} ms", self.guys.len(), self.fps, self.frame_ms);
        self.painter.text(x, y, 14.0, hot, &head);
        y += 22.0;

        self.painter.text(x, y, 11.0, dim, &format!("present {}", self.sensors.present));
        y += 18.0;

        for i in 0..self.guys.len() {
            let g = &self.guys[i];
            let [ignored, pending, emote, think] = g.attention.counts;
            let row = format!(
                "{:<10} {:<10} {} turns · gate {ignored}/{pending}/{emote}/{think} · q{} · {} tok",
                g.name.as_str(),
                g.body.drift.name(),
                g.turns,
                g.quantiser.pending(),
                g.last_reaction.as_ref().map(|r| r.tokens).unwrap_or(0),
            );
            self.painter.text(x, y, 11.0, ink, &row);
            y += 15.0;
            let err = self.guys[i].mind_error.clone();
            if let Some(e) = err {
                self.painter.text(x + 12.0, y, 10.0, hot, &crate::sensors::clip(&e, 64));
                y += 14.0;
            }
        }
        let _ = cursor;
    }

    /// The click-through contract: everything outside these rectangles belongs to whatever is below.
    fn sync_input_region(&mut self) {
        let rects: Vec<[i32; 4]> = self
            .guys
            .iter()
            .map(|g| {
                let b = g.avatar.bounds();
                let s = g.size;
                let l = if g.body.facing < 0.0 { -b.right } else { b.left };
                [
                    (g.body.pos[0] + l * s).floor() as i32,
                    (g.body.pos[1] + b.top * s).floor() as i32,
                    (b.width() * s).ceil() as i32,
                    (b.height() * s).ceil() as i32,
                ]
            })
            .collect();
        if self.input_region.as_deref() == Some(rects.as_slice()) {
            return;
        }
        if std::env::var_os("LILGUYS_DEBUG_INPUT").is_some() {
            println!("input regions -> {rects:?}");
        }
        self.input_region = Some(rects.clone());
        if let Ok(region) = Region::new(&self.compositor) {
            for r in &rects {
                region.add(r[0], r[1], r[2], r[3]);
            }
            self.layer.wl_surface().set_input_region(Some(region.wl_region()));
        }
    }

    /// Whoever's silhouette is under this point, nearest first.
    fn at_point(&self, x: f32, y: f32) -> Option<usize> {
        self.guys.iter().enumerate().find_map(|(i, g)| {
            let local = [(x - g.body.pos[0]) / g.size * g.body.facing, (y - g.body.pos[1]) / g.size];
            g.avatar.hit(local).then_some(i)
        })
    }
}

/// this layer can do while the model is unreachable is pull a face.
pub fn reflex(what: &Observation) -> (Emotion, f32, f32) {
    match what {
        // A feeling names its own tone, because only its author knows what it means.
        Observation::Feeling(f) => (
            f.tone.as_deref().and_then(Emotion::from_name).unwrap_or(Emotion::Curious),
            f.intensity.clamp(0.0, 1.0),
            f.hold.clamp(0.5, 120.0),
        ),
        // Being spoken to shows on the face before anybody works out what to say back.
        Observation::Told { .. } => (Emotion::Curious, 0.6, 6.0),
        Observation::Witnessed { .. } => (Emotion::Curious, 0.35, 5.0),
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
                for guy in &mut self.guys {
                    guy.body.bounds = [w as f32, h as f32];
                }
            }
            None => match Gpu::new(conn, self.layer.wl_surface(), w, h) {
                Ok(gpu) => {
                    self.gpu = Some(gpu);
                    // Spread them out rather than stacking everyone on the same pixel.
                    let n = self.guys.len().max(1) as f32;
                    for (i, guy) in self.guys.iter_mut().enumerate() {
                        guy.body.bounds = [w as f32, h as f32];
                        guy.body.pos =
                            [w as f32 * (i as f32 + 1.0) / (n + 1.0), h as f32 * 0.55];
                    }
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
        // Bound now, used only while a box is open — the surface asks for focus rather than
        // holding it, so it never steals anybody's typing.
        if capability == Capability::Keyboard && self.keyboard.is_none() {
            match self.seat_state.get_keyboard(qh, &seat, None) {
                Ok(k) => self.keyboard = Some(k),
                Err(e) => eprintln!("keyboard: {e}"),
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
        if capability == Capability::Keyboard {
            self.close_box();
            if let Some(k) = self.keyboard.take() {
                k.release();
            }
        }
    }

    fn remove_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat) {}
}

impl KeyboardHandler for App {
    fn enter(
        &mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_keyboard::WlKeyboard,
        _: &wl_surface::WlSurface, _: u32, _: &[u32], _: &[Keysym],
    ) {
    }

    /// Focus moved elsewhere — the person clicked another window, or the compositor took it away.
    /// That closes the box, which is how clicking away works without owning the whole screen.
    fn leave(
        &mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_keyboard::WlKeyboard,
        _: &wl_surface::WlSurface, _: u32,
    ) {
        self.close_box();
    }

    fn press_key(
        &mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_keyboard::WlKeyboard, _: u32,
        event: KeyEvent,
    ) {
        if self.typing.is_none() {
            return;
        }
        match event.keysym {
            Keysym::Escape => return self.close_box(),
            Keysym::Return | Keysym::KP_Enter => return self.send_typed(),
            _ => {}
        }
        let Some(typing) = self.typing.as_mut() else { return };
        typing.last_key = Instant::now();
        match event.keysym {
            Keysym::BackSpace => {
                typing.text.pop();
            }
            _ => {
                // The compositor already applied the layout and the modifiers; anything that is
                // not printable is not a character somebody meant to type.
                let typed = event.utf8.unwrap_or_default();
                if typed.chars().any(char::is_control) || typing.text.chars().count() >= TYPING_CAP
                {
                    return;
                }
                typing.text.push_str(&typed);
            }
        }
    }

    fn release_key(
        &mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_keyboard::WlKeyboard, _: u32,
        _: KeyEvent,
    ) {
    }

    fn repeat_key(
        &mut self, conn: &Connection, qh: &QueueHandle<Self>, kb: &wl_keyboard::WlKeyboard,
        serial: u32, event: KeyEvent,
    ) {
        self.press_key(conn, qh, kb, serial, event);
    }

    #[allow(clippy::too_many_arguments)]
    fn update_modifiers(
        &mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_keyboard::WlKeyboard, _: u32,
        _: Modifiers, _: RawModifiers, _: u32,
    ) {
    }
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
            let (px, py) = (event.position.0 as f32, event.position.1 as f32);
            match event.kind {
                PointerEventKind::Enter { .. } => self.hover = true,
                PointerEventKind::Leave { .. } => self.hover = false,
                PointerEventKind::Press { .. } => {
                    // The region is a union of rectangles, so a click inside it may still miss
                    // everyone. Asking each avatar keeps the silhouette authoritative.
                    if let Some(i) = self.at_point(px, py) {
                        self.held = Some(i);
                        self.pressed_at = Some([px, py]);
                        self.guys[i].body.grab();
                    }
                }
                PointerEventKind::Release { .. } => {
                    let from = self.pressed_at.take();
                    if let Some(i) = self.held.take() {
                        self.guys[i].body.release();
                        // A click opens the box; a drag was somebody moving him about.
                        let moved = from
                            .map(|a| (a[0] - px).hypot(a[1] - py))
                            .unwrap_or(f32::INFINITY);
                        if moved < 6.0 {
                            self.open_box(i);
                        }
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
