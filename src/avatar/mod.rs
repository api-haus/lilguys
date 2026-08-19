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

/// Passing facial weather. The mind picks from this list; the adapter decides how to show it.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub enum Emotion {
    #[default]
    Neutral,
    Curious,
    Pleased,
    Amused,
    Surprised,
    Concerned,
    Bored,
    Sleepy,
}

impl Emotion {
    pub const NAMES: [&'static str; 8] = [
        "neutral", "curious", "pleased", "amused", "surprised", "concerned", "bored", "sleepy",
    ];

    pub fn from_name(s: &str) -> Option<Self> {
        use Emotion::*;
        Some(match s {
            "neutral" => Neutral,
            "curious" => Curious,
            "pleased" => Pleased,
            "amused" => Amused,
            "surprised" => Surprised,
            "concerned" => Concerned,
            "bored" => Bored,
            "sleepy" => Sleepy,
            _ => return None,
        })
    }

    pub fn name(self) -> &'static str {
        Self::NAMES[self as usize]
    }

    /// Parameter targets at full intensity. Anything unlisted eases back to rest.
    pub fn targets(self) -> &'static [(Param, f32)] {
        use Emotion::*;
        use Param::*;
        match self {
            Neutral => &[],
            Curious => &[(BrowL, 0.7), (BrowR, 0.4), (HeadRoll, 0.16), (MouthForm, 0.2)],
            Pleased => &[(Joy, 0.8), (MouthForm, 0.8), (BrowL, 0.3), (BrowR, 0.3)],
            Amused => &[(Fun, 0.9), (MouthOpen, 0.4), (MouthForm, 0.9), (HeadPitch, -0.15)],
            Surprised => &[(Surprise, 1.0), (MouthOpen, 0.7), (BrowL, 1.0), (BrowR, 1.0)],
            Concerned => &[(Sorrow, 0.6), (BrowL, -0.6), (BrowR, -0.6), (MouthForm, -0.5)],
            Bored => &[(BrowL, -0.3), (BrowR, -0.3), (EyeOpenL, 0.55), (EyeOpenR, 0.55), (HeadPitch, 0.2)],
            Sleepy => &[(EyeOpenL, 0.2), (EyeOpenR, 0.2), (HeadPitch, 0.4), (HeadRoll, 0.22)],
        }
    }
}

/// A deliberate whole-body movement. Slower and far more visible than a reaction.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Gesture {
    Wave,
    Nod,
    Shake,
    Shrug,
    Point,
    Stretch,
    Bounce,
    Slump,
}

impl Gesture {
    pub const NAMES: [&'static str; 8] =
        ["wave", "nod", "shake", "shrug", "point", "stretch", "bounce", "slump"];

    pub fn from_name(s: &str) -> Option<Self> {
        use Gesture::*;
        Some(match s {
            "wave" => Wave,
            "nod" => Nod,
            "shake" => Shake,
            "shrug" => Shrug,
            "point" => Point,
            "stretch" => Stretch,
            "bounce" => Bounce,
            "slump" => Slump,
            _ => return None,
        })
    }

    pub fn name(self) -> &'static str {
        Self::NAMES[self as usize]
    }

    pub fn duration(self) -> f32 {
        use Gesture::*;
        match self {
            Wave | Point => 1.8,
            Nod | Shake => 1.2,
            Shrug | Bounce => 1.4,
            Stretch => 2.4,
            Slump => 2.0,
        }
    }
}

/// Body state that is not a facial parameter — a Live2D adapter turns it into motion selection, a
/// VRM adapter into a blend-tree weight, the graybox into limb offsets.
#[derive(Clone, Debug, Default)]
pub struct Drive {
    /// Speed through the air in pixels per second.
    pub speed: f32,
    /// Direction of travel, unit-ish.
    pub heading: [f32; 2],
    /// Float-cycle phase in radians, continuous.
    pub bob_phase: f32,
    /// Gesture in flight, with its progress from 0 to 1.
    pub gesture: Option<(Gesture, f32)>,
    pub speaking: bool,
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
