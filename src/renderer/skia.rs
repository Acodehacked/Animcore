use tiny_skia::{
    BlendMode as SkBlendMode, FillRule, GradientStop as SkStop, LineCap, LineJoin,
    LinearGradient, Paint as SkPaint, Path as SkPath, PathBuilder, Pixmap, Point,
    RadialGradient, Shader, SpreadMode, Stroke as SkStroke, Transform as SkTransform,
};

use crate::paint::{BlendMode, Fill, Gradient, GradientStop, Paint, StrokeCap, StrokeJoin};
use crate::path::AnimPath;
use crate::renderer::{DrawCmd, Renderer};
use nalgebra::Matrix3;

pub struct SkiaRenderer {
    pixmap: Option<Pixmap>,
}

impl SkiaRenderer {
    pub fn new() -> Self {
        Self { pixmap: None }
    }
}

impl Renderer for SkiaRenderer {
    fn begin_frame(&mut self, width: u32, height: u32, background: [u8; 4]) {
        let mut pm = Pixmap::new(width, height).expect("pixmap alloc failed");
        let [r, g, b, a] = background;
        pm.fill(tiny_skia::Color::from_rgba8(r, g, b, a));
        self.pixmap = Some(pm);
    }

    fn draw_path(&mut self, path: &AnimPath, paint: &Paint, transform: &Matrix3<f32>, opacity: f32) {
        let pm = match &mut self.pixmap {
            Some(p) => p,
            None => return,
        };

        let sk_path = match build_sk_path(path, transform) {
            Some(p) => p,
            None => return,
        };

        let combined_opacity = (paint.opacity * opacity).clamp(0.0, 1.0);
        let blend = to_sk_blend(paint.blend_mode);

        // Fill
        if let Some(shader) = fill_to_shader(&paint.fill, combined_opacity) {
            let mut sk_paint = SkPaint::default();
            sk_paint.shader = shader;
            sk_paint.blend_mode = blend;
            sk_paint.anti_alias = true;
            pm.fill_path(&sk_path, &sk_paint, FillRule::Winding, SkTransform::identity(), None);
        }

        // Stroke
        if let Some(stroke_cfg) = &paint.stroke {
            if let Some(shader) = fill_to_shader(&stroke_cfg.fill, combined_opacity) {
                let mut sk_paint = SkPaint::default();
                sk_paint.shader = shader;
                sk_paint.blend_mode = blend;
                sk_paint.anti_alias = true;

                let mut sk_stroke = SkStroke::default();
                sk_stroke.width = stroke_cfg.width;
                sk_stroke.line_cap = match stroke_cfg.cap {
                    StrokeCap::Butt   => LineCap::Butt,
                    StrokeCap::Round  => LineCap::Round,
                    StrokeCap::Square => LineCap::Square,
                };
                sk_stroke.line_join = match stroke_cfg.join {
                    StrokeJoin::Miter => LineJoin::Miter,
                    StrokeJoin::Round => LineJoin::Round,
                    StrokeJoin::Bevel => LineJoin::Bevel,
                };
                sk_stroke.miter_limit = stroke_cfg.miter_limit;

                if !stroke_cfg.dash.is_empty() {
                    sk_stroke.dash = tiny_skia::StrokeDash::new(
                        stroke_cfg.dash.clone(),
                        stroke_cfg.dash_offset,
                    );
                }

                pm.stroke_path(&sk_path, &sk_paint, &sk_stroke, SkTransform::identity(), None);
            }
        }
    }

    fn end_frame(&mut self) -> Vec<u8> {
        match &self.pixmap {
            Some(pm) => pm.data().to_vec(),
            None => vec![],
        }
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn build_sk_path(path: &AnimPath, transform: &Matrix3<f32>) -> Option<SkPath> {
    let cmds = super::transform_path(path, transform);
    let mut pb = PathBuilder::new();
    for cmd in cmds {
        match cmd {
            DrawCmd::MoveTo(x, y)                        => pb.move_to(x, y),
            DrawCmd::LineTo(x, y)                        => pb.line_to(x, y),
            DrawCmd::CubicTo(cx1, cy1, cx2, cy2, x, y) => pb.cubic_to(cx1, cy1, cx2, cy2, x, y),
            DrawCmd::QuadTo(cx, cy, x, y)               => pb.quad_to(cx, cy, x, y),
            DrawCmd::Close                               => pb.close(),
        }
    }
    pb.finish()
}

fn fill_to_shader(fill: &Fill, opacity: f32) -> Option<Shader<'static>> {
    match fill {
        Fill::None => None,
        Fill::Solid(c) => {
            let [r, g, b, a] = c.to_u8();
            let final_a = ((a as f32 / 255.0) * opacity * 255.0) as u8;
            Some(Shader::SolidColor(tiny_skia::Color::from_rgba8(r, g, b, final_a)))
        }
        Fill::Gradient(g) => gradient_to_shader(g, opacity),
    }
}

fn gradient_to_shader(g: &Gradient, opacity: f32) -> Option<Shader<'static>> {
    match g {
        Gradient::Linear { start, end, stops } => {
            let sk_stops: Vec<SkStop> = stops.iter().map(|s| to_sk_stop(s, opacity)).collect();
            LinearGradient::new(
                Point::from_xy(start[0], start[1]),
                Point::from_xy(end[0], end[1]),
                sk_stops,
                SpreadMode::Pad,
                SkTransform::identity(),
            )
        }
        Gradient::Radial { center, radius, stops } => {
            let sk_stops: Vec<SkStop> = stops.iter().map(|s| to_sk_stop(s, opacity)).collect();
            let cp = Point::from_xy(center[0], center[1]);
            RadialGradient::new(
                cp,
                cp, // focal == center for standard radial gradient
                *radius,
                sk_stops,
                SpreadMode::Pad,
                SkTransform::identity(),
            )
        }
    }
}

fn to_sk_stop(s: &GradientStop, opacity: f32) -> SkStop {
    let [r, g, b, a] = s.color.to_u8();
    let final_a = ((a as f32 / 255.0) * opacity * 255.0) as u8;
    SkStop::new(
        s.position,
        tiny_skia::Color::from_rgba8(r, g, b, final_a),
    )
}

fn to_sk_blend(bm: BlendMode) -> SkBlendMode {
    match bm {
        BlendMode::Normal     => SkBlendMode::SourceOver,
        BlendMode::Multiply   => SkBlendMode::Multiply,
        BlendMode::Screen     => SkBlendMode::Screen,
        BlendMode::Overlay    => SkBlendMode::Overlay,
        BlendMode::Darken     => SkBlendMode::Darken,
        BlendMode::Lighten    => SkBlendMode::Lighten,
        BlendMode::ColorDodge => SkBlendMode::ColorDodge,
        BlendMode::ColorBurn  => SkBlendMode::ColorBurn,
        BlendMode::HardLight  => SkBlendMode::HardLight,
        BlendMode::SoftLight  => SkBlendMode::SoftLight,
        BlendMode::Difference => SkBlendMode::Difference,
        BlendMode::Exclusion  => SkBlendMode::Exclusion,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::paint::{Color, Paint};
    use crate::path::AnimPath;
    use nalgebra::Matrix3;

    #[test]
    fn render_filled_rect_to_pixels() {
        let mut renderer = SkiaRenderer::new();
        renderer.begin_frame(64, 64, [255, 255, 255, 255]);

        let path = AnimPath::rect(10.0, 10.0, 44.0, 44.0);
        let paint = Paint::filled(Color::from_hex(0xFF0000));

        renderer.draw_path(&path, &paint, &Matrix3::identity(), 1.0);

        let pixels = renderer.end_frame();
        assert_eq!(pixels.len(), 64 * 64 * 4);

        // pixel at center (32,32) should be red
        let idx = (32 * 64 + 32) * 4;
        assert_eq!(pixels[idx], 255);     // R
        assert_eq!(pixels[idx + 1], 0);   // G
        assert_eq!(pixels[idx + 2], 0);   // B
    }

    #[test]
    fn render_stroked_ellipse() {
        let mut renderer = SkiaRenderer::new();
        renderer.begin_frame(128, 128, [0, 0, 0, 255]);

        let path = AnimPath::ellipse(64.0, 64.0, 40.0, 30.0);
        let paint = Paint::stroked(Color::WHITE, 2.0);

        renderer.draw_path(&path, &paint, &Matrix3::identity(), 1.0);
        let pixels = renderer.end_frame();
        assert_eq!(pixels.len(), 128 * 128 * 4);
    }

    #[test]
    fn render_linear_gradient() {
        use crate::paint::{Fill, Gradient, GradientStop};
        let mut renderer = SkiaRenderer::new();
        renderer.begin_frame(100, 100, [255, 255, 255, 255]);

        let path = AnimPath::rect(0.0, 0.0, 100.0, 100.0);
        let paint = Paint {
            fill: Fill::Gradient(Gradient::Linear {
                start: [0.0, 0.0],
                end: [100.0, 0.0],
                stops: vec![
                    GradientStop { position: 0.0, color: Color::from_hex(0xFF0000) },
                    GradientStop { position: 1.0, color: Color::from_hex(0x0000FF) },
                ],
            }),
            ..Default::default()
        };

        renderer.draw_path(&path, &paint, &Matrix3::identity(), 1.0);
        let pixels = renderer.end_frame();
        // leftmost pixel should be red-ish
        assert!(pixels[0] > 200);
        // rightmost pixel should be blue-ish
        let right_idx = (99 * 4) as usize;
        assert!(pixels[right_idx + 2] > 200);
    }
}
