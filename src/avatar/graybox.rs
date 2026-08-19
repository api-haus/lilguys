//! Labelled shapes standing in for a character, so composition and locomotion can be judged before any art exists.

use super::{Avatar, Bounds, Drive, Gesture, Param, Pose};
use crate::gpu::painter::{rgba, Color, Painter};

const JOINT: Color = rgba(255, 255, 255, 0.85);
const INK: Color = rgba(24, 26, 32, 1.0);
const LABEL: Color = rgba(255, 255, 255, 0.55);
const BOUNDS: Color = rgba(226, 86, 200, 0.55);
const GAZE: Color = rgba(250, 214, 82, 0.75);

pub struct Graybox {
    pose: Pose,
    drive: Drive,
    palette: crate::config::Skin,
    /// Draw joint dots, part labels and the bounds frame.
    pub annotate: bool,
}

impl Default for Graybox {
    fn default() -> Self {
        Self {
            pose: Pose::default(),
            drive: Drive::default(),
            palette: crate::config::Skin::default(),
            annotate: false,
        }
    }
}

impl Graybox {
    pub fn annotated(annotate: bool) -> Self {
        Self { annotate, ..Default::default() }
    }
}

/// Rig-local point, y negative is up, origin between the feet, 1.0 is character height.
type P = [f32; 2];

fn add(a: P, b: P) -> P {
    [a[0] + b[0], a[1] + b[1]]
}

fn rot_about(p: P, pivot: P, a: f32) -> P {
    let (s, c) = a.sin_cos();
    let (dx, dy) = (p[0] - pivot[0], p[1] - pivot[1]);
    [pivot[0] + dx * c - dy * s, pivot[1] + dx * s + dy * c]
}

impl Avatar for Graybox {
    fn name(&self) -> &str {
        "graybox"
    }

    fn advance(&mut self, pose: &Pose, drive: &Drive, _dt: f32) {
        self.pose = pose.clone();
        self.drive = drive.clone();
    }

    fn set_palette(&mut self, skin: crate::config::Skin) {
        self.palette = skin;
    }

    fn bounds(&self) -> Bounds {
        Bounds { left: -0.26, top: -1.0, right: 0.26, bottom: 0.03 }
    }

    fn hit(&self, local: P) -> bool {
        let b = self.bounds();
        // Generous box rather than a true silhouette; a real adapter tests its own alpha.
        local[0] >= b.left && local[0] <= b.right && local[1] >= b.top && local[1] <= b.bottom
    }

    fn draw(&self, painter: &mut Painter, origin: P, facing: f32, scale: f32) {
        let p = &self.pose;
        let opaque = |c: [u8; 3]| rgba(c[0], c[1], c[2], 1.0);
        let (skin, torso_c, arm_c, leg_c) = (
            opaque(self.palette.head),
            opaque(self.palette.torso),
            opaque(self.palette.arm),
            opaque(self.palette.leg),
        );
        let m = if facing < 0.0 { -1.0 } else { 1.0 };
        // X-ish params are world space; `to_px` mirrors, so rig-local uses must pre-multiply by m
        // and screen-space uses must not. Getting this backwards inverts the gaze when facing left.
        let yaw = p.get(Param::HeadYaw) * m;
        let body_yaw_local = p.get(Param::BodyYaw) * m;
        let roll = p.get(Param::HeadRoll) * m;
        let gaze = [p.get(Param::GazeX), p.get(Param::GazeY)];
        let breath = p.get(Param::Breath);
        let bob = self.drive.bob_phase;
        // Limbs trail the direction of travel, so drifting reads as swimming rather than sliding.
        let drag = (self.drive.speed / 190.0).clamp(0.0, 1.4);
        let lean = [-self.drive.heading[0] * drag * m, -self.drive.heading[1] * drag];
        let (gk, gt) = match self.drive.gesture {
            Some((g, t)) => (Some(g), t),
            None => (None, 0.0),
        };
        // One smooth in-and-out envelope drives every gesture; each maps it to different limbs.
        let ge = (gt * std::f32::consts::PI).sin();

        // Rig-local to surface pixels.
        let to_px = |q: P| -> P { [origin[0] + q[0] * m * scale, origin[1] + q[1] * scale] };
        let px = scale;

        let body_yaw = body_yaw_local * 0.10;
        let squash = match gk {
            Some(Gesture::Bounce) => -ge * 0.05,
            Some(Gesture::Slump) => ge * 0.07,
            Some(Gesture::Stretch) => -ge * 0.05,
            _ => 0.0,
        };
        let hip: P = [body_yaw * 0.4 + lean[0] * 0.05, -0.46 + breath * 0.004 + squash];
        let chest: P = [body_yaw + lean[0] * 0.09, -0.70 + breath * 0.010 + squash * 0.6];
        let neck: P = [body_yaw * 1.1 + lean[0] * 0.11, -0.775 + breath * 0.012 + squash * 0.4];

        // ---- legs ----
        let kick = if gk == Some(Gesture::Bounce) { ge * 0.5 } else { 0.0 };
        let tuck = if gk == Some(Gesture::Slump) { ge * 0.16 } else { 0.0 };
        for (side, phase) in [(-1.0f32, 0.0f32), (1.0, std::f32::consts::PI)] {
            let s = (bob * 0.8 + phase).sin() * 0.14 + lean[0] * 0.5 + kick * side;
            let l = (bob * 0.8 + phase).cos() * 0.05 + lean[1] * 0.4;
            let h = add(hip, [side * 0.075, 0.0]);
            let knee = add(h, [s * 0.16, 0.24 - l * 0.4 - tuck]);
            let foot = add(knee, [s * 0.10, 0.22 - l - tuck]);
            painter.line(to_px(h), to_px(knee), 0.075 * px, leg_c);
            painter.line(to_px(knee), to_px(foot), 0.065 * px, leg_c);
            painter.rrect(
                to_px(add(foot, [0.02, 0.012]))[0],
                to_px(add(foot, [0.02, 0.012]))[1],
                0.13 * px,
                0.045 * px,
                0.02 * px,
                leg_c,
            );
            if self.annotate {
                painter.ellipse(to_px(knee)[0], to_px(knee)[1], 0.028 * px, 0.028 * px, JOINT);
                painter.ellipse(to_px(h)[0], to_px(h)[1], 0.030 * px, 0.030 * px, JOINT);
            }
        }

        // ---- torso ----
        let torso_mid = [(hip[0] + chest[0]) * 0.5, (hip[1] + chest[1]) * 0.5];
        painter.rrect_rot(
            to_px(torso_mid)[0],
            to_px(torso_mid)[1],
            0.26 * px,
            (hip[1] - chest[1]).abs() * px + 0.06 * px,
            0.09 * px,
            (chest[0] - hip[0]) * m * 0.6,
            torso_c,
        );

        // ---- arms ----
        for (side, phase) in [(-1.0f32, std::f32::consts::PI), (1.0, 0.0)] {
            let lead = side > 0.0;
            let s = (bob * 0.8 + phase).sin() * 0.16 + lean[0] * 0.7;
            let shoulder = add(chest, [side * 0.115, -0.015]);
            // A gesture overrides the drift pose on whichever arm it uses.
            let mut elbow = add(shoulder, [s * 0.13 - side * 0.02, 0.155 + lean[1] * 0.5]);
            let mut hand = add(elbow, [s * 0.10, 0.145 + lean[1] * 0.5]);
            match gk {
                Some(Gesture::Wave) if lead => {
                    elbow = add(shoulder, [side * 0.10, 0.02]);
                    hand = add(elbow, [side * (0.06 + (gt * 22.0).sin() * 0.09), -0.13]);
                }
                Some(Gesture::Point) if lead => {
                    elbow = add(shoulder, [side * (0.10 + ge * 0.06), 0.06]);
                    hand = add(elbow, [side * (0.10 + ge * 0.12), 0.02 - ge * 0.04]);
                }
                Some(Gesture::Shrug) => {
                    elbow = add(shoulder, [side * (0.13 + ge * 0.05), 0.10 - ge * 0.06]);
                    hand = add(elbow, [side * 0.05, 0.10 - ge * 0.05]);
                }
                Some(Gesture::Stretch) => {
                    elbow = add(shoulder, [side * 0.09, 0.04 - ge * 0.14]);
                    hand = add(elbow, [side * 0.05, 0.04 - ge * 0.16]);
                }
                _ => {}
            }
            painter.line(to_px(shoulder), to_px(elbow), 0.062 * px, arm_c);
            painter.line(to_px(elbow), to_px(hand), 0.054 * px, arm_c);
            painter.ellipse(to_px(hand)[0], to_px(hand)[1], 0.055 * px, 0.055 * px, arm_c);
            if self.annotate {
                painter.ellipse(to_px(shoulder)[0], to_px(shoulder)[1], 0.028 * px, 0.028 * px, JOINT);
                painter.ellipse(to_px(elbow)[0], to_px(elbow)[1], 0.024 * px, 0.024 * px, JOINT);
            }
        }

        // ---- head ----
        let nod = match gk {
            Some(Gesture::Nod) => (gt * 14.0).sin() * 0.030,
            Some(Gesture::Slump) => ge * 0.045,
            _ => 0.0,
        };
        let shake = match gk {
            Some(Gesture::Shake) => (gt * 15.0).sin() * 0.035,
            _ => 0.0,
        };
        let head_c = add(
            neck,
            [yaw * 0.035 + shake * m, -0.105 + p.get(Param::HeadPitch) * 0.012 + nod],
        );
        let head_c = rot_about(head_c, neck, roll * 0.5);
        painter.line(to_px(neck), to_px(head_c), 0.07 * px, skin);
        painter.rrect_rot(
            to_px(head_c)[0],
            to_px(head_c)[1],
            0.235 * px,
            0.245 * px,
            0.10 * px,
            roll * m,
            skin,
        );

        // ---- face ----
        for (i, side) in [(0usize, -1.0f32), (1, 1.0)] {
            let open = p.get(if i == 0 { Param::EyeOpenL } else { Param::EyeOpenR }).clamp(0.0, 1.0);
            let eye = rot_about(add(head_c, [side * 0.058 + yaw * 0.022, -0.012]), head_c, roll);
            let e = to_px(eye);
            painter.ellipse(e[0], e[1], 0.070 * px, 0.070 * px * open.max(0.06), rgba(250, 250, 252, 1.0));
            if open > 0.15 {
                painter.ellipse(
                    e[0] + gaze[0] * 0.020 * px,
                    e[1] + gaze[1] * 0.016 * px,
                    0.032 * px,
                    0.032 * px * open,
                    INK,
                );
            }
            let brow = p.get(if i == 0 { Param::BrowL } else { Param::BrowR });
            painter.rrect_rot(
                e[0],
                e[1] - (0.062 + brow * 0.018) * px,
                0.075 * px,
                0.016 * px,
                0.008 * px,
                roll * m + side * m * brow * 0.18,
                INK,
            );
        }
        let mouth = to_px(rot_about(add(head_c, [yaw * 0.020, 0.085]), head_c, roll));
        // Speech owns the mouth outright, so a held expression cannot clamp it shut mid-word.
        let open = match self.drive.speaking {
            true => p.get(Param::MouthOpen).clamp(0.12, 1.0),
            false => p.get(Param::MouthOpen).clamp(0.0, 1.0),
        };
        painter.rrect(mouth[0], mouth[1], (0.075 + p.get(Param::MouthForm) * 0.03) * px, (0.012 + open * 0.075) * px, 0.02 * px, INK);

        if !self.annotate {
            return;
        }

        // ---- annotation ----
        let b = self.bounds();
        painter.frame(
            origin[0] + (b.left + b.right) * 0.5 * m * scale,
            origin[1] + (b.top + b.bottom) * 0.5 * scale,
            b.width() * scale,
            b.height() * scale,
            1.0,
            BOUNDS,
        );
        painter.ellipse(origin[0], origin[1], 7.0, 7.0, BOUNDS);

        let gz = to_px(rot_about(add(head_c, [yaw * 0.020, -0.012]), head_c, roll));
        painter.line(gz, [gz[0] + gaze[0] * 90.0, gz[1] + gaze[1] * 70.0], 2.0, GAZE);

        for (label, at) in [("head", head_c), ("chest", chest), ("hip", hip)] {
            let q = to_px(at);
            painter.text(q[0] + 0.16 * px, q[1], 12.0, LABEL, label);
        }
        if let Some(g) = gk {
            let q = to_px(add(head_c, [0.0, -0.30]));
            painter.text(q[0] - 0.10 * px, q[1], 12.0, GAZE, g.name());
        }
    }
}
