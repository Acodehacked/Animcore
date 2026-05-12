use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq)]
pub struct Color {
    pub r: f32,
    pub g: f32,
    pub b: f32,
    pub a: f32,
}

impl Color {
    pub const TRANSPARENT: Self = Self { r: 0.0, g: 0.0, b: 0.0, a: 0.0 };
    pub const BLACK: Self = Self { r: 0.0, g: 0.0, b: 0.0, a: 1.0 };
    pub const WHITE: Self = Self { r: 1.0, g: 1.0, b: 1.0, a: 1.0 };

    pub fn rgba(r: f32, g: f32, b: f32, a: f32) -> Self {
        Self { r, g, b, a }
    }

    pub fn from_hex(hex: u32) -> Self {
        Self {
            r: ((hex >> 16) & 0xFF) as f32 / 255.0,
            g: ((hex >> 8) & 0xFF) as f32 / 255.0,
            b: (hex & 0xFF) as f32 / 255.0,
            a: 1.0,
        }
    }

    pub fn with_alpha(mut self, a: f32) -> Self {
        self.a = a;
        self
    }

    pub fn to_u8(self) -> [u8; 4] {
        [
            (self.r.clamp(0.0, 1.0) * 255.0) as u8,
            (self.g.clamp(0.0, 1.0) * 255.0) as u8,
            (self.b.clamp(0.0, 1.0) * 255.0) as u8,
            (self.a.clamp(0.0, 1.0) * 255.0) as u8,
        ]
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct GradientStop {
    pub position: f32,
    pub color: Color,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum Gradient {
    Linear {
        start: [f32; 2],
        end: [f32; 2],
        stops: Vec<GradientStop>,
    },
    Radial {
        center: [f32; 2],
        radius: f32,
        stops: Vec<GradientStop>,
    },
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum Fill {
    Solid(Color),
    Gradient(Gradient),
    None,
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq)]
pub enum StrokeCap {
    Butt,
    Round,
    Square,
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq)]
pub enum StrokeJoin {
    Miter,
    Round,
    Bevel,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Stroke {
    pub fill: Fill,
    pub width: f32,
    pub cap: StrokeCap,
    pub join: StrokeJoin,
    pub miter_limit: f32,
    /// dash pattern: [on, off, on, off, …], empty = solid
    pub dash: Vec<f32>,
    pub dash_offset: f32,
}

impl Stroke {
    pub fn solid(color: Color, width: f32) -> Self {
        Self {
            fill: Fill::Solid(color),
            width,
            cap: StrokeCap::Butt,
            join: StrokeJoin::Miter,
            miter_limit: 4.0,
            dash: vec![],
            dash_offset: 0.0,
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq)]
pub enum BlendMode {
    Normal,
    Multiply,
    Screen,
    Overlay,
    Darken,
    Lighten,
    ColorDodge,
    ColorBurn,
    HardLight,
    SoftLight,
    Difference,
    Exclusion,
}

impl Default for BlendMode {
    fn default() -> Self {
        Self::Normal
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Paint {
    pub fill: Fill,
    pub stroke: Option<Stroke>,
    pub opacity: f32,
    pub blend_mode: BlendMode,
}

impl Default for Paint {
    fn default() -> Self {
        Self {
            fill: Fill::Solid(Color::BLACK),
            stroke: None,
            opacity: 1.0,
            blend_mode: BlendMode::Normal,
        }
    }
}

impl Paint {
    pub fn filled(color: Color) -> Self {
        Self { fill: Fill::Solid(color), ..Default::default() }
    }

    pub fn stroked(color: Color, width: f32) -> Self {
        Self {
            fill: Fill::None,
            stroke: Some(Stroke::solid(color, width)),
            ..Default::default()
        }
    }
}
