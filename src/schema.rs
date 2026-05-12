use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::paint::Paint;
use crate::path::AnimPath;
use crate::transform::Transform;

// ── Document ─────────────────────────────────────────────────────────────────

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Document {
    pub version: u32,
    pub artboards: Vec<Artboard>,
}

// ── Artboard ─────────────────────────────────────────────────────────────────

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Artboard {
    pub id: Uuid,
    pub name: String,
    pub width: f32,
    pub height: f32,
    pub background: crate::paint::Color,
    /// Nodes ordered so parents always precede children.
    pub nodes: Vec<Node>,
    pub animations: Vec<Animation>,
}

// ── Node ─────────────────────────────────────────────────────────────────────

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Node {
    pub id: Uuid,
    pub name: String,
    pub transform: Transform,
    pub parent_id: Option<Uuid>,
    pub opacity: f32,
    pub visible: bool,
    pub shape: Option<ShapeData>,
    pub clip_children: bool,
}

impl Node {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            id: Uuid::new_v4(),
            name: name.into(),
            transform: Transform::default(),
            parent_id: None,
            opacity: 1.0,
            visible: true,
            shape: None,
            clip_children: false,
        }
    }
}

// ── Shape data ───────────────────────────────────────────────────────────────

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ShapeData {
    pub geometry: Geometry,
    pub paint: Paint,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum Geometry {
    /// Axis-aligned rectangle (before node transform).
    Rect { width: f32, height: f32, corner_radius: f32 },
    /// Ellipse with independent x/y radii.
    Ellipse { radius_x: f32, radius_y: f32 },
    /// Arbitrary cubic-bezier path.
    Path(AnimPath),
}

impl Geometry {
    /// Convert any geometry variant to an AnimPath for rendering.
    pub fn to_path(&self) -> AnimPath {
        match self {
            Geometry::Rect { width, height, corner_radius } => {
                if *corner_radius <= 0.0 {
                    AnimPath::rect(0.0, 0.0, *width, *height)
                } else {
                    rounded_rect(0.0, 0.0, *width, *height, *corner_radius)
                }
            }
            Geometry::Ellipse { radius_x, radius_y } => {
                AnimPath::ellipse(0.0, 0.0, *radius_x, *radius_y)
            }
            Geometry::Path(p) => p.clone(),
        }
    }
}

fn rounded_rect(x: f32, y: f32, w: f32, h: f32, r: f32) -> AnimPath {
    let r = r.min(w / 2.0).min(h / 2.0);
    const K: f32 = 0.5522847498;
    let kr = r * K;
    let mut p = AnimPath::new();
    p.move_to(x + r, y);
    p.line_to(x + w - r, y);
    p.cubic_to(x + w - r + kr, y, x + w, y + r - kr, x + w, y + r);
    p.line_to(x + w, y + h - r);
    p.cubic_to(x + w, y + h - r + kr, x + w - r + kr, y + h, x + w - r, y + h);
    p.line_to(x + r, y + h);
    p.cubic_to(x + r - kr, y + h, x, y + h - r + kr, x, y + h - r);
    p.line_to(x, y + r);
    p.cubic_to(x, y + r - kr, x + r - kr, y, x + r, y);
    p.close();
    p
}

// ── Animation ────────────────────────────────────────────────────────────────

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Animation {
    pub id: Uuid,
    pub name: String,
    pub duration_secs: f32,
    pub fps: u32,
    pub loop_mode: LoopMode,
    pub tracks: Vec<Track>,
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq)]
pub enum LoopMode {
    Once,
    Loop,
    PingPong,
}

impl Default for LoopMode {
    fn default() -> Self {
        Self::Loop
    }
}

// ── Track / Keyframe ─────────────────────────────────────────────────────────

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Track {
    pub node_id: Uuid,
    pub property: Property,
    pub keyframes: Vec<Keyframe>,
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq)]
pub enum Property {
    X,
    Y,
    Rotation,
    ScaleX,
    ScaleY,
    SkewX,
    SkewY,
    Opacity,
    // Paint properties
    FillColorR,
    FillColorG,
    FillColorB,
    FillColorA,
    StrokeWidth,
    // Path morph (by index into points buffer)
    PathPointX(u32),
    PathPointY(u32),
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Keyframe {
    pub time_secs: f32,
    pub value: f32,
    pub easing: Easing,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum Easing {
    Linear,
    Hold,
    CubicBezier(f32, f32, f32, f32),
}
