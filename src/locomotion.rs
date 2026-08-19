//! Where the buddy is and why he moved there. Hysteresis and dwell are the personality — see docs/design-space.md §5.

use crate::avatar::{Param, Pose};

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Gait {
    Idle,
    Noticing,
    Approaching,
    Retreating,
    Settling,
    Dragged,
}

impl Gait {
    pub fn name(self) -> &'static str {
        match self {
            Gait::Idle => "idle",
            Gait::Noticing => "noticing",
            Gait::Approaching => "approaching",
            Gait::Retreating => "retreating",
            Gait::Settling => "settling",
            Gait::Dragged => "dragged",
        }
    }
}

#[derive(Clone, Debug)]
pub struct Tuning {
    /// Cursor closer than this earns a look.
    pub notice_radius: f32,
    /// Cursor further than this stops earning one. Must exceed `notice_radius`.
    pub forget_radius: f32,
    /// He will not stand closer than this.
    pub personal_space: f32,
    /// Beyond this he may consider walking over.
    pub approach_radius: f32,
    /// Seconds the cursor must hold still inside `notice_radius` before he looks.
    pub notice_dwell: f32,
    /// Seconds before he commits to actually walking. Deliberately long.
    pub approach_dwell: f32,
    pub walk_speed: f32,
    pub accel: f32,
    /// Seconds of stillness after arriving, before Idle resumes.
    pub settle_time: f32,
    /// Seconds the cursor must sit behind him before he turns to face it.
    pub turn_dwell: f32,
    /// Character height in pixels.
    pub height: f32,
}

impl Default for Tuning {
    fn default() -> Self {
        Self {
            notice_radius: 420.0,
            forget_radius: 620.0,
            personal_space: 140.0,
            approach_radius: 700.0,
            notice_dwell: 0.35,
            approach_dwell: 2.5,
            walk_speed: 210.0,
            accel: 900.0,
            settle_time: 1.2,
            turn_dwell: 0.7,
            height: 240.0,
        }
    }
}

pub struct Body {
    pub pos: [f32; 2],
    pub vel_x: f32,
    pub facing: f32,
    pub gait: Gait,
    pub tuning: Tuning,
    pub ground_y: f32,
    pub cursor_dist: f32,
    target_x: f32,
    dwell: f32,
    state_age: f32,
    blink_timer: f32,
    blink_phase: f32,
    breath: f32,
    step_phase: f32,
    sway: f32,
    turn_dwell: f32,
}

impl Body {
    pub fn new(tuning: Tuning, ground_y: f32, x: f32) -> Self {
        Self {
            pos: [x, ground_y],
            vel_x: 0.0,
            facing: 1.0,
            gait: Gait::Idle,
            tuning,
            ground_y,
            cursor_dist: f32::MAX,
            target_x: x,
            dwell: 0.0,
            state_age: 0.0,
            blink_timer: 2.0,
            blink_phase: 0.0,
            breath: 0.0,
            step_phase: 0.0,
            sway: 0.0,
            turn_dwell: 0.0,
        }
    }

    fn enter(&mut self, gait: Gait) {
        if self.gait != gait {
            self.gait = gait;
            self.state_age = 0.0;
            self.dwell = 0.0;
        }
    }

    pub fn grab(&mut self) {
        self.enter(Gait::Dragged);
        self.vel_x = 0.0;
    }

    pub fn drag_to(&mut self, x: f32, y: f32) {
        if self.gait == Gait::Dragged {
            self.pos = [x, y];
        }
    }

    pub fn release(&mut self) {
        if self.gait == Gait::Dragged {
            self.pos[1] = self.ground_y;
            self.enter(Gait::Settling);
        }
    }

    /// One tick. `cursor` is in the same surface pixel space as `pos`.
    pub fn update(&mut self, cursor: [f32; 2], dt: f32, pose: &mut Pose) {
        self.state_age += dt;
        let t = self.tuning.clone();

        let dx = cursor[0] - self.pos[0];
        let dy = cursor[1] - (self.pos[1] - t.height * 0.78); // measure from the eyes, not the feet
        let dist = (dx * dx + dy * dy).sqrt();
        self.cursor_dist = dist;

        self.decide(dist, dx, dt, &t);
        self.locomote(dt, &t);
        self.face(dx, dt, &t);
        self.drive_face(dx, dy, dist, dt, &t, pose);
        self.drive_idle(dt, pose);
    }

    fn decide(&mut self, dist: f32, dx: f32, dt: f32, t: &Tuning) {
        match self.gait {
            Gait::Dragged => {}
            Gait::Idle => {
                self.dwell = if dist < t.notice_radius { self.dwell + dt } else { 0.0 };
                if self.dwell >= t.notice_dwell {
                    self.enter(Gait::Noticing);
                }
            }
            Gait::Noticing => {
                if dist > t.forget_radius {
                    self.enter(Gait::Idle);
                    return;
                }
                // A cursor already within reach is worth a look and nothing more.
                let far = dist > t.approach_radius * 0.5 && dist > t.personal_space * 1.6;
                self.dwell = if far { self.dwell + dt } else { 0.0 };
                if self.dwell >= t.approach_dwell {
                    self.target_x = self.pos[0] + dx - t.personal_space * dx.signum();
                    self.enter(Gait::Approaching);
                }
                if dist < t.personal_space * 0.6 {
                    self.target_x = self.pos[0] - dx.signum() * t.personal_space;
                    self.enter(Gait::Retreating);
                }
            }
            Gait::Approaching | Gait::Retreating => {
                if (self.target_x - self.pos[0]).abs() < 6.0 {
                    self.enter(Gait::Settling);
                }
                if self.state_age > 8.0 {
                    self.enter(Gait::Settling); // never walk forever
                }
            }
            Gait::Settling => {
                if self.state_age >= t.settle_time {
                    self.enter(if dist < t.notice_radius { Gait::Noticing } else { Gait::Idle });
                }
            }
        }
    }

    fn locomote(&mut self, dt: f32, t: &Tuning) {
        let walking = matches!(self.gait, Gait::Approaching | Gait::Retreating);
        let want = if walking {
            let d = self.target_x - self.pos[0];
            d.signum() * t.walk_speed * (d.abs() / 80.0).min(1.0)
        } else {
            0.0
        };
        let k = 1.0 - (-t.accel / t.walk_speed.max(1.0) * dt).exp();
        self.vel_x += (want - self.vel_x) * k;
        self.pos[0] += self.vel_x * dt;

        if self.vel_x.abs() > 12.0 {
            self.step_phase += self.vel_x.abs() / 46.0 * dt * std::f32::consts::TAU;
        } else {
            // Ease the swing back to neutral so he does not freeze mid-stride.
            self.step_phase = unwind(self.step_phase, dt * 6.0);
        }
    }

    /// Facing follows the walk. Standing still, he turns only for a cursor that stays behind him —
    /// without the dwell he pivots at every stray flick of the mouse.
    fn face(&mut self, dx: f32, dt: f32, t: &Tuning) {
        if self.vel_x.abs() > 12.0 {
            self.facing = self.vel_x.signum();
            self.turn_dwell = 0.0;
            return;
        }
        let behind = dx.abs() > t.personal_space * 0.5 && dx.signum() != self.facing;
        if behind && self.gait != Gait::Idle {
            self.turn_dwell += dt;
            if self.turn_dwell >= t.turn_dwell {
                self.facing = dx.signum();
                self.turn_dwell = 0.0;
            }
        } else {
            self.turn_dwell = 0.0;
        }
    }

    fn drive_face(&mut self, dx: f32, dy: f32, dist: f32, dt: f32, t: &Tuning, pose: &mut Pose) {
        let attentive = self.gait != Gait::Idle && dist < t.forget_radius;
        let (gx, gy) = if attentive {
            ((dx / t.forget_radius).clamp(-1.0, 1.0), (dy / (t.forget_radius * 0.6)).clamp(-1.0, 1.0))
        } else {
            (0.0, 0.0)
        };

        // Eyes lead, head follows, body trails. The lag is what reads as deliberate.
        pose.ease_to(Param::GazeX, gx, 14.0, dt);
        pose.ease_to(Param::GazeY, gy, 14.0, dt);
        pose.ease_to(Param::HeadYaw, gx * 0.75, 6.0, dt);
        pose.ease_to(Param::HeadPitch, gy * 0.55, 6.0, dt);
        pose.ease_to(Param::BodyYaw, gx * 0.30, 2.5, dt);

        let interest = match self.gait {
            Gait::Noticing => 0.35,
            Gait::Approaching => 0.55,
            Gait::Retreating => 0.0,
            Gait::Dragged => 0.9,
            _ => 0.1,
        };
        pose.ease_to(Param::BrowL, interest, 4.0, dt);
        pose.ease_to(Param::BrowR, interest, 4.0, dt);
        pose.ease_to(Param::Surprise, (self.gait == Gait::Dragged) as u8 as f32, 8.0, dt);
    }

    fn drive_idle(&mut self, dt: f32, pose: &mut Pose) {
        self.breath = (self.breath + dt * 0.9) % std::f32::consts::TAU;
        pose.set(Param::Breath, self.breath.sin() * 0.5 + 0.5);

        self.sway += dt * 0.37;
        pose.ease_to(Param::HeadRoll, self.sway.sin() * 0.08, 3.0, dt);

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
    }

    pub fn step_phase(&self) -> f32 {
        self.step_phase
    }

    pub fn moving(&self) -> bool {
        self.vel_x.abs() > 1.0
    }
}

fn unwind(a: f32, k: f32) -> f32 {
    use std::f32::consts::{PI, TAU};
    let wrapped = (a + PI).rem_euclid(TAU) - PI;
    wrapped * (1.0 - k.clamp(0.0, 1.0))
}
