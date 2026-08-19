//! The seam. Above it lives the buddy's mind, below it a puppet that knows nothing.

pub mod graybox;

use crate::gpu::painter::Painter;

/// The parameter vocabulary is the contract — not the file format.
///
/// Names follow Live2D's standard parameter set because that makes the whole VTuber model corpus
/// an asset library. Every one of these also lands on a glTF/VRM rig: head/body angles drive bone
/// rotations, the rest drive morph-target weights or VRM expression presets.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[repr(usize)]
pub enum Param {
    HeadYaw,    // Live2D ParamAngleX  | VRM head bone Y rotation
    HeadPitch,  // Live2D ParamAngleY  | VRM head bone X rotation
    HeadRoll,   // Live2D ParamAngleZ  | VRM head bone Z rotation
    BodyYaw,    // Live2D ParamBodyAngleX
    BodyPitch,  // Live2D ParamBodyAngleY
    BodyRoll,   // Live2D ParamBodyAngleZ
    EyeOpenL,   // Live2D ParamEyeLOpen | VRM Blink_L inverted
    EyeOpenR,   // Live2D ParamEyeROpen | VRM Blink_R inverted
    GazeX,      // Live2D ParamEyeBallX | VRM LookLeft/LookRight
    GazeY,      // Live2D ParamEyeBallY | VRM LookUp/LookDown
    BrowL,      // Live2D ParamBrowLY
    BrowR,      // Live2D ParamBrowRY
    MouthOpen,  // Live2D ParamMouthOpenY | VRM A
    MouthForm,  // Live2D ParamMouthForm  | VRM I..O blend
    Breath,     // Live2D ParamBreath | procedural on VRM
    Joy,        // VRM Joy      | Live2D custom expression
    Sorrow,     // VRM Sorrow
    Anger,      // VRM Angry
    Fun,        // VRM Fun
    Surprise,   // VRM (custom)
}

impl Param {
    pub const COUNT: usize = 20;
    pub const ALL: [Param; Self::COUNT] = {
        use Param::*;
        [
            HeadYaw, HeadPitch, HeadRoll, BodyYaw, BodyPitch, BodyRoll, EyeOpenL, EyeOpenR, GazeX,
            GazeY, BrowL, BrowR, MouthOpen, MouthForm, Breath, Joy, Sorrow, Anger, Fun, Surprise,
        ]
    };

    pub fn name(self) -> &'static str {
        use Param::*;
        match self {
            HeadYaw => "head_yaw", HeadPitch => "head_pitch", HeadRoll => "head_roll",
            BodyYaw => "body_yaw", BodyPitch => "body_pitch", BodyRoll => "body_roll",
            EyeOpenL => "eye_open_l", EyeOpenR => "eye_open_r",
            GazeX => "gaze_x", GazeY => "gaze_y", BrowL => "brow_l", BrowR => "brow_r",
            MouthOpen => "mouth_open", MouthForm => "mouth_form", Breath => "breath",
            Joy => "joy", Sorrow => "sorrow", Anger => "anger", Fun => "fun", Surprise => "surprise",
        }
    }

    /// Rest value. Eyes rest open; everything else rests at zero.
    pub fn rest(self) -> f32 {
        matches!(self, Param::EyeOpenL | Param::EyeOpenR) as u8 as f32
    }
}

/// A full parameter set. Ranges are -1..1 for bidirectional params, 0..1 for the rest.
#[derive(Clone, Debug)]
pub struct Pose {
    v: [f32; Param::COUNT],
}

impl Default for Pose {
    fn default() -> Self {
        let mut v = [0.0; Param::COUNT];
        for p in Param::ALL {
            v[p as usize] = p.rest();
        }
        Self { v }
    }
}

impl Pose {
    pub fn get(&self, p: Param) -> f32 {
        self.v[p as usize]
    }

    pub fn set(&mut self, p: Param, x: f32) {
        self.v[p as usize] = x;
    }

    pub fn add(&mut self, p: Param, x: f32) {
        self.v[p as usize] += x;
    }

    /// Exponential approach, framerate-independent. `rate` is the fraction closed per second.
    pub fn ease_to(&mut self, p: Param, target: f32, rate: f32, dt: f32) {
        let k = 1.0 - (-rate * dt).exp();
        self.v[p as usize] += (target - self.v[p as usize]) * k;
    }
}

/// Local-space bounding box, origin at the character's ground point between the feet.
#[derive(Copy, Clone, Debug)]
pub struct Bounds {
    pub left: f32,
    pub top: f32,
    pub right: f32,
    pub bottom: f32,
}

impl Bounds {
    pub fn width(&self) -> f32 {
        self.right - self.left
    }
    pub fn height(&self) -> f32 {
        self.bottom - self.top
    }
}

/// Locomotion state that is not a facial parameter — a Live2D adapter turns it into motion
/// selection, a VRM adapter into a blend-tree weight, the graybox into a leg swing.
#[derive(Clone, Debug, Default)]
pub struct Drive {
    /// Ground speed in pixels per second.
    pub speed: f32,
    /// Walk-cycle phase in radians, continuous across stops.
    pub step_phase: f32,
    pub airborne: bool,
}

/// One adapter per rendering technology. Live2D and VRM implement this over the same `Pose`.
pub trait Avatar {
    fn name(&self) -> &str;

    /// Advance internal animation (physics, motion playback) — never reads the world.
    fn advance(&mut self, pose: &Pose, drive: &Drive, dt: f32);

    /// Emit geometry. `origin` is the character's ground point in surface pixels, `facing` is
    /// -1.0 (left) to 1.0 (right), `scale` is pixels per rig unit.
    fn draw(&self, painter: &mut Painter, origin: [f32; 2], facing: f32, scale: f32);

    fn bounds(&self) -> Bounds;

    /// Silhouette test in local rig units. Drives both click handling and the input region, so
    /// a false here means the click passes through to whatever is underneath.
    fn hit(&self, local: [f32; 2]) -> bool;
}
