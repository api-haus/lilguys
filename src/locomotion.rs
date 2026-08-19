//! Where a lilguy is and why. Nothing here knows about gravity — see docs/design-space.md §5.

use crate::avatar::{Emotion, Gesture, Param, Pose};
use crate::config::Motion;
use crate::sensors::Feeling;

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Drift {
    /// Floating in place, minding its own business.
    Idle,
    /// Watching the pointer without committing to anything.
    Noticing,
    /// Travelling to a chosen point.
    Seeking,
    /// Arrived, staying a while.
    Lingering,
    /// Heading off the edge of the screen on purpose.
    Leaving,
    Held,
}

impl Drift {
    pub fn name(self) -> &'static str {
        match self {
            Drift::Idle => "idle",
            Drift::Noticing => "noticing",
            Drift::Seeking => "seeking",
            Drift::Lingering => "lingering",
            Drift::Leaving => "leaving",
            Drift::Held => "held",
        }
    }
}

#[derive(Clone, Debug)]
pub struct Target {
    pub at: [f32; 2],
    /// Seconds to stay once arrived. Negative means leave and do not come back.
    pub linger: f32,
    pub label: String,
}

pub struct Body {
    pub pos: [f32; 2],
    pub vel: [f32; 2],
    pub facing: f32,
    pub drift: Drift,
    pub motion: Motion,
    pub size: f32,
    pub bounds: [f32; 2],
    pub cursor_dist: f32,
    pub emotion: Emotion,
    pub emotion_weight: f32,
    pub speaking: bool,
    target: Option<Target>,
    emotion_hold: f32,
    gesture: Option<(Gesture, f32)>,
    linger_left: f32,
    state_age: f32,
    turn_dwell: f32,
    wander_roll: f32,
    blink_timer: f32,
    blink_phase: f32,
    breath: f32,
    bob: f32,
    sway: f32,
    felt: Vec<Feeling>,
    was_offscreen: bool,
}

impl Body {
    pub fn new(motion: Motion, size: f32, bounds: [f32; 2]) -> Self {
        Self {
            pos: [bounds[0] * 0.5, bounds[1] * 0.55],
            vel: [0.0, 0.0],
            facing: 1.0,
            drift: Drift::Idle,
            motion,
            size,
            bounds,
            cursor_dist: f32::MAX,
            emotion: Emotion::Neutral,
            emotion_weight: 0.0,
            speaking: false,
            target: None,
            emotion_hold: 0.0,
            gesture: None,
            linger_left: 0.0,
            state_age: 0.0,
            turn_dwell: 0.0,
            wander_roll: 0.0,
            blink_timer: 2.0,
            blink_phase: 0.0,
            breath: 0.0,
            bob: 0.0,
            sway: 0.0,
            felt: Vec::new(),
            was_offscreen: false,
        }
    }

    /// Interoception. The body knows things about itself the mind would otherwise never learn.
    pub fn drain_feelings(&mut self) -> Vec<Feeling> {
        std::mem::take(&mut self.felt)
    }

    fn feel_that(&mut self, state: &str, detail: &str, tone: &str, intensity: f32, hold: f32) {
        self.felt.push(Feeling::own(state, detail, tone, intensity, hold));
    }

    /// One line of how it is in here, for the framing of every slice. Continuous, unlike an event.
    pub fn sensation(&self) -> String {
        let mood = if self.emotion_weight > 0.05 {
            format!("{} {:.0}%", self.emotion.name(), self.emotion_weight * 100.0)
        } else {
            "settled".into()
        };
        let doing = match self.drift {
            Drift::Held => "being held".into(),
            Drift::Leaving => "drifting off the screen".into(),
            Drift::Seeking => match self.target.as_ref() {
                Some(t) => format!("on your way to {}", t.label),
                None => "on your way somewhere".into(),
            },
            Drift::Lingering => match self.target.as_ref() {
                Some(t) => format!("settled by {}", t.label),
                None => "settled".into(),
            },
            Drift::Noticing => "watching the pointer".into(),
            Drift::Idle => "drifting".into(),
        };
        format!("{doing}, feeling {mood}")
    }

    fn enter(&mut self, drift: Drift) {
        if self.drift != drift {
            self.drift = drift;
            self.state_age = 0.0;
        }
    }

    // ---- commands from the mind ------------------------------------------------

    pub fn go_to(&mut self, target: Target) {
        self.linger_left = target.linger;
        let leaving = target.linger < 0.0;
        self.target = Some(target);
        self.enter(if leaving { Drift::Leaving } else { Drift::Seeking });
    }

    /// Deliberate. The mind asked for this, so it needs no telling that it happened.
    pub fn feel(&mut self, emotion: Emotion, intensity: f32, hold: f32) {
        self.emotion = emotion;
        self.emotion_weight = intensity.clamp(0.0, 1.0);
        self.emotion_hold = hold.max(0.5);
    }

    /// Reflexive. It happened without asking, so the record says so — self-awareness is not free,
    /// it is this line. The record is marked reflective and cannot provoke another reflex.
    pub fn reflex(&mut self, emotion: Emotion, intensity: f32, hold: f32, because: &str) {
        self.feel(emotion, intensity, hold);
        self.felt.push(Feeling::reflex_record(
            &format!("looking {}", emotion.name()),
            because,
        ));
    }

    pub fn perform(&mut self, gesture: Gesture) {
        self.gesture = Some((gesture, 0.0));
    }

    pub fn gesture(&self) -> Option<(Gesture, f32)> {
        self.gesture.map(|(g, t)| (g, (t / g.duration()).clamp(0.0, 1.0)))
    }

    pub fn target_label(&self) -> Option<&str> {
        self.target.as_ref().map(|t| t.label.as_str())
    }

    pub fn bob_phase(&self) -> f32 {
        self.bob
    }

    pub fn speed(&self) -> f32 {
        (self.vel[0] * self.vel[0] + self.vel[1] * self.vel[1]).sqrt()
    }

    pub fn heading(&self) -> [f32; 2] {
        let s = self.speed();
        if s < 1.0 {
            return [self.facing, 0.0];
        }
        [self.vel[0] / s, self.vel[1] / s]
    }

    pub fn moving(&self) -> bool {
        self.speed() > 4.0
    }

    /// True once he has drifted fully out of sight, so the surface can stop rendering.
    pub fn offscreen(&self) -> bool {
        let m = self.size;
        self.pos[0] < -m || self.pos[0] > self.bounds[0] + m || self.pos[1] < -m
            || self.pos[1] > self.bounds[1] + m
    }

    pub fn grab(&mut self) {
        self.enter(Drift::Held);
        self.vel = [0.0, 0.0];
        self.target = None;
        self.feel_that("picked up", "someone has hold of you", "surprised", 0.8, 6.0);
    }

    pub fn drag_to(&mut self, x: f32, y: f32) {
        if self.drift == Drift::Held {
            let next = [x, y + self.size * 0.35];
            self.vel = [(next[0] - self.pos[0]) * 12.0, (next[1] - self.pos[1]) * 12.0];
            self.pos = next;
        }
    }

    pub fn release(&mut self) {
        if self.drift == Drift::Held {
            self.enter(Drift::Idle);
            self.feel_that("set down", "back to your own devices", "pleased", 0.5, 8.0);
        }
    }

    // ---- the tick --------------------------------------------------------------

    pub fn update(&mut self, cursor: [f32; 2], dt: f32, pose: &mut Pose) {
        self.state_age += dt;
        let m = self.motion.clone();

        let eye = [self.pos[0], self.pos[1] - self.size * 0.28];
        let (dx, dy) = (cursor[0] - eye[0], cursor[1] - eye[1]);
        self.cursor_dist = (dx * dx + dy * dy).sqrt();

        self.decide(dt, &m);
        self.steer(dt, &m);
        let gone = self.offscreen();
        if gone != self.was_offscreen {
            self.was_offscreen = gone;
            match gone {
                true => self.feel_that("out of sight", "nobody can see you", "neutral", 0.4, 20.0),
                false => self.feel_that("back in view", "", "curious", 0.4, 6.0),
            }
        }
        self.face(dx, dt, &m);
        self.advance_gesture(dt);
        self.drive_face(dx, dy, dt, &m, pose);
        self.drive_idle(dt, pose);
        self.apply_emotion(dt, pose);
    }

    fn decide(&mut self, dt: f32, m: &Motion) {
        match self.drift {
            Drift::Held => {}
            Drift::Leaving => {
                if self.offscreen() && self.state_age > 4.0 {
                    // Nothing out here to do; come back rather than vanish forever.
                    self.target = None;
                    self.enter(Drift::Idle);
                }
            }
            Drift::Seeking => {
                if self.arrived() {
                    let where_ = self.target.as_ref().map(|t| t.label.clone()).unwrap_or_default();
                    self.feel_that("arrived", &where_, "curious", 0.4, 6.0);
                    self.enter(Drift::Lingering);
                } else if self.state_age > 12.0 {
                    self.target = None;
                    self.enter(Drift::Idle);
                }
            }
            Drift::Lingering => {
                self.linger_left -= dt;
                if self.linger_left <= 0.0 {
                    let where_ = self.target.take().map(|t| t.label).unwrap_or_default();
                    self.feel_that("done here", &where_, "bored", 0.3, 5.0);
                    self.enter(Drift::Idle);
                }
            }
            Drift::Idle | Drift::Noticing => {
                let near = self.cursor_dist < m.notice_radius;
                let far = self.cursor_dist > m.forget_radius;
                if near && self.drift == Drift::Idle {
                    self.enter(Drift::Noticing);
                } else if far && self.drift == Drift::Noticing {
                    self.enter(Drift::Idle);
                }
                self.maybe_wander(dt, m);
            }
        }
    }

    /// Occasionally picks somewhere to be for no reason. Without this he only ever moves when
    /// told to, and reads as a widget rather than a creature.
    fn maybe_wander(&mut self, dt: f32, m: &Motion) {
        if m.wander_per_minute <= 0.0 || self.state_age < 6.0 {
            return;
        }
        self.wander_roll += dt * m.wander_per_minute / 60.0;
        if self.wander_roll < 1.0 {
            return;
        }
        self.wander_roll = 0.0;
        self.felt.push(Feeling::reflex_record("restless", "nothing in particular"));
        // Deterministic scatter from the clocks already running; no RNG dependency for a wander.
        let a = (self.sway * 12.9898 + self.breath * 78.233).sin() * 43758.547;
        let b = (self.bob * 39.3468 + self.sway * 11.135).sin() * 24634.633;
        let at = [
            (a.fract().abs() * 0.8 + 0.1) * self.bounds[0],
            (b.fract().abs() * 0.7 + 0.12) * self.bounds[1],
        ];
        self.go_to(Target { at, linger: 8.0 + a.fract().abs() * 20.0, label: "wandering".into() });
    }

    fn arrived(&self) -> bool {
        let Some(t) = self.target.as_ref() else { return true };
        let (dx, dy) = (t.at[0] - self.pos[0], t.at[1] - self.pos[1]);
        (dx * dx + dy * dy).sqrt() < self.size * 0.18
    }

    /// Seek-with-arrival, plus a slow float. No ground, no fall, no jump.
    fn steer(&mut self, dt: f32, m: &Motion) {
        if self.drift == Drift::Held {
            return;
        }
        let want = match (&self.target, self.drift) {
            (Some(t), Drift::Seeking | Drift::Leaving) => {
                let d = [t.at[0] - self.pos[0], t.at[1] - self.pos[1]];
                let dist = (d[0] * d[0] + d[1] * d[1]).sqrt().max(0.001);
                let ease = (dist / (self.size * 1.2)).min(1.0);
                [d[0] / dist * m.speed * ease, d[1] / dist * m.speed * ease]
            }
            _ => [0.0, 0.0],
        };

        let k = 1.0 - (-m.agility * dt).exp();
        self.vel[0] += (want[0] - self.vel[0]) * k;
        self.vel[1] += (want[1] - self.vel[1]) * k;
        self.pos[0] += self.vel[0] * dt;
        self.pos[1] += self.vel[1] * dt;

        // A gentle figure-of-eight so he is never perfectly still.
        self.bob += dt * 1.15;
        self.pos[0] += (self.bob * 0.5).sin() * 6.0 * dt;
        self.pos[1] += self.bob.sin() * 9.0 * dt;

        self.contain(m);
    }

    /// He may leave the screen, but only when he meant to.
    fn contain(&mut self, m: &Motion) {
        if self.drift == Drift::Leaving || self.drift == Drift::Held {
            return;
        }
        let slack = [self.bounds[0] * m.offscreen_margin, self.bounds[1] * m.offscreen_margin];
        let lo = [-slack[0], -slack[1]];
        let hi = [self.bounds[0] + slack[0], self.bounds[1] + slack[1]];
        for i in 0..2 {
            if self.pos[i] < lo[i] {
                self.pos[i] = lo[i];
                self.vel[i] = self.vel[i].abs() * 0.4;
            } else if self.pos[i] > hi[i] {
                self.pos[i] = hi[i];
                self.vel[i] = -self.vel[i].abs() * 0.4;
            }
        }
    }

    fn face(&mut self, dx: f32, dt: f32, m: &Motion) {
        if self.vel[0].abs() > 22.0 {
            self.facing = self.vel[0].signum();
            self.turn_dwell = 0.0;
            return;
        }
        let behind = dx.abs() > m.personal_space * 0.5 && dx.signum() != self.facing;
        if behind && self.drift != Drift::Idle {
            self.turn_dwell += dt;
            if self.turn_dwell >= 0.7 {
                self.facing = dx.signum();
                self.turn_dwell = 0.0;
            }
        } else {
            self.turn_dwell = 0.0;
        }
    }

    fn advance_gesture(&mut self, dt: f32) {
        let Some((g, t)) = self.gesture.as_mut() else { return };
        *t += dt;
        if *t >= g.duration() {
            self.gesture = None;
        }
    }

    fn drive_face(&mut self, dx: f32, dy: f32, dt: f32, m: &Motion, pose: &mut Pose) {
        let watching = match self.drift {
            Drift::Idle | Drift::Leaving => false,
            _ => self.cursor_dist < m.forget_radius,
        };
        let (gx, gy) = if watching {
            ((dx / m.forget_radius).clamp(-1.0, 1.0), (dy / (m.forget_radius * 0.6)).clamp(-1.0, 1.0))
        } else if self.moving() {
            // Look where you are going.
            let h = self.heading();
            (h[0], h[1] * 0.5)
        } else {
            (0.0, 0.0)
        };

        // Eyes lead, head follows, body trails. The lag is what reads as deliberate.
        pose.ease_to(Param::GazeX, gx, 14.0, dt);
        pose.ease_to(Param::GazeY, gy, 14.0, dt);
        pose.ease_to(Param::HeadYaw, gx * 0.75, 6.0, dt);
        pose.ease_to(Param::HeadPitch, gy * 0.55, 6.0, dt);
        pose.ease_to(Param::BodyYaw, gx * 0.30, 2.5, dt);
    }

    fn drive_idle(&mut self, dt: f32, pose: &mut Pose) {
        self.breath = (self.breath + dt * 0.9) % std::f32::consts::TAU;
        pose.set(Param::Breath, self.breath.sin() * 0.5 + 0.5);

        self.sway += dt * 0.37;
        pose.ease_to(Param::BodyRoll, self.sway.sin() * 0.10, 2.0, dt);

        self.blink_timer -= dt;
        if self.blink_timer <= 0.0 {
            self.blink_phase = 1.0;
            // Irrational-ish interval so blinks never lock to the breath cycle.
            self.blink_timer = 2.6 + (self.sway * 7.31).sin().abs() * 3.4;
        }
        if self.blink_phase > 0.0 {
            self.blink_phase = (self.blink_phase - dt * 7.0).max(0.0);
        }
        let open = 1.0 - (self.blink_phase * std::f32::consts::PI).sin().abs();
        pose.set(Param::EyeOpenL, open);
        pose.set(Param::EyeOpenR, open);

        if self.speaking {
            // Mouth flap keyed to nothing in particular; real visemes need the synthesiser.
            let flap = ((self.bob * 9.0).sin() * 0.5 + 0.5).powf(1.5);
            pose.ease_to(Param::MouthOpen, 0.15 + flap * 0.5, 22.0, dt);
        }
    }

    fn apply_emotion(&mut self, dt: f32, pose: &mut Pose) {
        if self.emotion_hold > 0.0 {
            self.emotion_hold -= dt;
            if self.emotion_hold <= 0.0 {
                self.emotion_weight = 0.0;
            }
        }
        if self.emotion_weight <= 0.001 {
            return;
        }
        for (p, target) in self.emotion.targets() {
            // Blinks and speech own their parameters outright; an expression must not fight them.
            if matches!(p, Param::EyeOpenL | Param::EyeOpenR) && self.blink_phase > 0.0 {
                continue;
            }
            if matches!(p, Param::MouthOpen) && self.speaking {
                continue;
            }
            pose.ease_to(*p, target * self.emotion_weight, 5.0, dt);
        }
    }
}
