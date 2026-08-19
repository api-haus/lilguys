//! Labelled shapes standing in for a character, so composition and locomotion can be judged before any art exists.

use super::{Avatar, Bounds, Drive, Param, Pose};
use crate::gpu::painter::{rgba, Color, Painter};

const SKIN: Color = rgba(232, 196, 160, 1.0);
const TORSO: Color = rgba(96, 124, 176, 1.0);
const ARM: Color = rgba(104, 168, 128, 1.0);
const LEG: Color = rgba(200, 136, 84, 1.0);
const JOINT: Color = rgba(255, 255, 255, 0.85);
const INK: Color = rgba(24, 26, 32, 1.0);
const LABEL: Color = rgba(255, 255, 255, 0.55);
const BOUNDS: Color = rgba(226, 86, 200, 0.55);
const GAZE: Color = rgba(250, 214, 82, 0.75);

pub struct Graybox {
    pose: Pose,
    drive: Drive,
    /// Draw joint dots, part labels and the bounds frame.
    pub annotate: bool,
}

impl Default for Graybox {
    fn default() -> Self {
        Self { pose: Pose::default(), drive: Drive::default(), annotate: true }
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
        let m = if facing < 0.0 { -1.0 } else { 1.0 };
        // X-ish params are world space; `to_px` mirrors, so rig-local uses must pre-multiply by m
        // and screen-space uses must not. Getting this backwards inverts the gaze when facing left.
        let yaw = p.get(Param::HeadYaw) * m;
        let body_yaw_local = p.get(Param::BodyYaw) * m;
        let roll = p.get(Param::HeadRoll) * m;
        let gaze = [p.get(Param::GazeX), p.get(Param::GazeY)];
        let breath = p.get(Param::Breath);
        let step = self.drive.step_phase;
        let stride = (self.drive.speed / 210.0).clamp(0.0, 1.0);

        // Rig-local to surface pixels.
        let to_px = |q: P| -> P { [origin[0] + q[0] * m * scale, origin[1] + q[1] * scale] };
        let px = scale;

        let body_yaw = body_yaw_local * 0.10;
        let hip: P = [body_yaw * 0.4, -0.46 + breath * 0.004];
        let chest: P = [body_yaw, -0.70 + breath * 0.010];
        let neck: P = [body_yaw * 1.1, -0.775 + breath * 0.012];

        // ---- legs ----
        let swing = step.sin() * 0.42 * stride;
        let lift = (step.cos() * 0.5 + 0.5) * 0.10 * stride;
        for (side, phase) in [(-1.0f32, 0.0f32), (1.0, std::f32::consts::PI)] {
            let s = (step + phase).sin() * 0.42 * stride;
            let l = ((step + phase).cos() * 0.5 + 0.5) * 0.10 * stride;
            let h = add(hip, [side * 0.075, 0.0]);
            let knee = add(h, [s * 0.16, 0.24 - l * 0.4]);
            let foot = add(knee, [s * 0.10, 0.22 - l]);
            painter.line(to_px(h), to_px(knee), 0.075 * px, LEG);
            painter.line(to_px(knee), to_px(foot), 0.065 * px, LEG);
            painter.rrect(
                to_px(add(foot, [0.02, 0.012]))[0],
                to_px(add(foot, [0.02, 0.012]))[1],
                0.13 * px,
                0.045 * px,
                0.02 * px,
                LEG,
            );
            if self.annotate {
                painter.ellipse(to_px(knee)[0], to_px(knee)[1], 0.028 * px, 0.028 * px, JOINT);
                painter.ellipse(to_px(h)[0], to_px(h)[1], 0.030 * px, 0.030 * px, JOINT);
            }
        }
        let _ = (swing, lift);

        // ---- torso ----
        let torso_mid = [(hip[0] + chest[0]) * 0.5, (hip[1] + chest[1]) * 0.5];
        painter.rrect_rot(
            to_px(torso_mid)[0],
            to_px(torso_mid)[1],
            0.26 * px,
            (hip[1] - chest[1]).abs() * px + 0.06 * px,
            0.09 * px,
            (chest[0] - hip[0]) * m * 0.6,
            TORSO,
        );

        // ---- arms ----
        for (side, phase) in [(-1.0f32, std::f32::consts::PI), (1.0, 0.0)] {
            let s = (step + phase).sin() * 0.34 * stride;
            let shoulder = add(chest, [side * 0.115, -0.015]);
            let elbow = add(shoulder, [s * 0.13 - side * 0.02, 0.155]);
            let hand = add(elbow, [s * 0.10, 0.145]);
            painter.line(to_px(shoulder), to_px(elbow), 0.062 * px, ARM);
            painter.line(to_px(elbow), to_px(hand), 0.054 * px, ARM);
            painter.ellipse(to_px(hand)[0], to_px(hand)[1], 0.055 * px, 0.055 * px, ARM);
            if self.annotate {
                painter.ellipse(to_px(shoulder)[0], to_px(shoulder)[1], 0.028 * px, 0.028 * px, JOINT);
                painter.ellipse(to_px(elbow)[0], to_px(elbow)[1], 0.024 * px, 0.024 * px, JOINT);
            }
        }

        // ---- head ----
        let head_c = add(neck, [yaw * 0.035, -0.105 + p.get(Param::HeadPitch) * 0.012]);
        let head_c = rot_about(head_c, neck, roll * 0.5);
        painter.line(to_px(neck), to_px(head_c), 0.07 * px, SKIN);
        painter.rrect_rot(
            to_px(head_c)[0],
            to_px(head_c)[1],
            0.235 * px,
            0.245 * px,
            0.10 * px,
            roll * m,
            SKIN,
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
        let open = p.get(Param::MouthOpen).clamp(0.0, 1.0);
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
    }
}
