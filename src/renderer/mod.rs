use crate::paint::{BlendMode, Paint};
use crate::path::{AnimPath, PathVerb};
use nalgebra::Matrix3;

pub mod skia;

/// Minimal interface every renderer must implement.
pub trait Renderer {
    fn begin_frame(&mut self, width: u32, height: u32, background: [u8; 4]);
    fn draw_path(&mut self, path: &AnimPath, paint: &Paint, transform: &Matrix3<f32>, opacity: f32);
    /// Returns raw RGBA8 pixels, row-major.
    fn end_frame(&mut self) -> Vec<u8>;
}

// ── Shared helpers ─────────────────────────────────────────────────────────

/// Flatten transform + path into a concrete list of draw calls for backends
/// that need a simple command stream.
pub enum DrawCmd {
    MoveTo(f32, f32),
    LineTo(f32, f32),
    CubicTo(f32, f32, f32, f32, f32, f32),
    QuadTo(f32, f32, f32, f32),
    Close,
}

/// Transform all points in a path by a matrix and return draw commands.
pub fn transform_path(path: &AnimPath, m: &Matrix3<f32>) -> Vec<DrawCmd> {
    let mut cmds = Vec::with_capacity(path.verbs.len());
    let mut pi = 0usize;

    let tp = |pt: [f32; 2]| -> (f32, f32) {
        let v = m * nalgebra::Vector3::new(pt[0], pt[1], 1.0);
        (v.x, v.y)
    };

    for verb in &path.verbs {
        match verb {
            PathVerb::MoveTo => {
                let (x, y) = tp(path.points[pi]);
                cmds.push(DrawCmd::MoveTo(x, y));
                pi += 1;
            }
            PathVerb::LineTo => {
                let (x, y) = tp(path.points[pi]);
                cmds.push(DrawCmd::LineTo(x, y));
                pi += 1;
            }
            PathVerb::CubicTo => {
                let (cx1, cy1) = tp(path.points[pi]);
                let (cx2, cy2) = tp(path.points[pi + 1]);
                let (x, y)     = tp(path.points[pi + 2]);
                cmds.push(DrawCmd::CubicTo(cx1, cy1, cx2, cy2, x, y));
                pi += 3;
            }
            PathVerb::QuadTo => {
                let (cx, cy) = tp(path.points[pi]);
                let (x, y)   = tp(path.points[pi + 1]);
                cmds.push(DrawCmd::QuadTo(cx, cy, x, y));
                pi += 2;
            }
            PathVerb::Close => {
                cmds.push(DrawCmd::Close);
            }
        }
    }
    cmds
}

pub fn blend_mode_to_u8(bm: BlendMode) -> u8 {
    match bm {
        BlendMode::Normal     => 0,
        BlendMode::Multiply   => 1,
        BlendMode::Screen     => 2,
        BlendMode::Overlay    => 3,
        BlendMode::Darken     => 4,
        BlendMode::Lighten    => 5,
        BlendMode::ColorDodge => 6,
        BlendMode::ColorBurn  => 7,
        BlendMode::HardLight  => 8,
        BlendMode::SoftLight  => 9,
        BlendMode::Difference => 10,
        BlendMode::Exclusion  => 11,
    }
}
