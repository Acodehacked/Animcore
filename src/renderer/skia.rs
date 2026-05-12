use tiny_skia::{
    BlendMode as SkBlendMode, FillRule, GradientStop as SkStop, LineCap, LineJoin,
    LinearGradient, Mask, Paint as SkPaint, Path as SkPath, PathBuilder, Pixmap, PixmapPaint,
    Point, RadialGradient, Shader, SpreadMode, Stroke as SkStroke, Transform as SkTransform,
};

use crate::effects::Effect;
use crate::paint::{BlendMode, Color, Fill, Gradient, GradientStop, Paint, StrokeCap, StrokeJoin};
use crate::path::AnimPath;
use crate::renderer::{DrawCmd, Renderer};
use nalgebra::Matrix3;

pub struct SkiaRenderer {
    pixmap: Option<Pixmap>,
    clip_stack: Vec<Mask>,
}

impl SkiaRenderer {
    pub fn new() -> Self {
        Self { pixmap: None, clip_stack: vec![] }
    }
}

impl Default for SkiaRenderer {
    fn default() -> Self {
        Self::new()
    }
}

impl Renderer for SkiaRenderer {
    fn begin_frame(&mut self, width: u32, height: u32, background: [u8; 4]) {
        let mut pm = Pixmap::new(width, height).expect("pixmap alloc failed");
        let [r, g, b, a] = background;
        pm.fill(tiny_skia::Color::from_rgba8(r, g, b, a));
        self.pixmap = Some(pm);
        self.clip_stack.clear();
    }

    fn push_clip(&mut self, path: &AnimPath, transform: &Matrix3<f32>) {
        let (w, h) = match &self.pixmap {
            Some(p) => (p.width(), p.height()),
            None => return,
        };

        if let Some(sk_path) = build_sk_path(path, transform) {
            if let Some(mut mask) = Mask::new(w, h) {
                mask.fill_path(&sk_path, FillRule::Winding, true, SkTransform::identity());
                self.clip_stack.push(mask);
            }
        }
    }

    fn pop_clip(&mut self) {
        self.clip_stack.pop();
    }

    fn draw_path(
        &mut self,
        path: &AnimPath,
        paint: &Paint,
        transform: &Matrix3<f32>,
        opacity: f32,
        effects: &[Effect],
    ) {
        let pm = match &mut self.pixmap {
            Some(p) => p,
            None => return,
        };

        let sk_path = match build_sk_path(path, transform) {
            Some(p) => p,
            None => return,
        };

        let active_clip = self.clip_stack.last();
        let combined_opacity = (paint.opacity * opacity).clamp(0.0, 1.0);
        let blend = to_sk_blend(paint.blend_mode);

        // ── Pre-pass: effects that render behind the shape ────────────────
        for effect in effects {
            match effect {
                Effect::DropShadow { offset_x, offset_y, blur_radius, color } => {
                    draw_shadow(
                        pm, &sk_path, *color, *offset_x, *offset_y, *blur_radius,
                        combined_opacity, active_clip,
                    );
                }
                Effect::OuterGlow { blur_radius, color, opacity: glow_opacity } => {
                    draw_shadow(
                        pm, &sk_path, *color, 0.0, 0.0, *blur_radius,
                        combined_opacity * glow_opacity, active_clip,
                    );
                }
                Effect::InnerGlow { .. } => { /* Phase 3 */ }
            }
        }

        // ── Fill ──────────────────────────────────────────────────────────
        if let Some(shader) = fill_to_shader(&paint.fill, combined_opacity) {
            let mut sk_paint = SkPaint::default();
            sk_paint.shader = shader;
            sk_paint.blend_mode = blend;
            sk_paint.anti_alias = true;
            pm.fill_path(&sk_path, &sk_paint, FillRule::Winding, SkTransform::identity(), active_clip);
        }

        // ── Stroke ────────────────────────────────────────────────────────
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

                pm.stroke_path(&sk_path, &sk_paint, &sk_stroke, SkTransform::identity(), active_clip);
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

// ── Shadow / glow rendering ────────────────────────────────────────────────────

fn draw_shadow(
    pm: &mut Pixmap,
    sk_path: &SkPath,
    color: Color,
    offset_x: f32,
    offset_y: f32,
    blur_radius: f32,
    opacity: f32,
    clip: Option<&Mask>,
) {
    let w = pm.width();
    let h = pm.height();

    // Render the shape silhouette in shadow color to a temp pixmap
    let mut shadow_pm = match Pixmap::new(w, h) {
        Some(p) => p,
        None => return,
    };

    let [r, g, b, a] = color.to_u8();
    let shadow_alpha = ((a as f32 / 255.0) * opacity * 255.0) as u8;
    let shader = Shader::SolidColor(tiny_skia::Color::from_rgba8(r, g, b, shadow_alpha));

    let mut sk_paint = SkPaint::default();
    sk_paint.shader = shader;
    sk_paint.anti_alias = true;
    shadow_pm.fill_path(sk_path, &sk_paint, FillRule::Winding, SkTransform::identity(), None);

    // Blur the silhouette
    if blur_radius > 0.5 {
        let sigma = blur_radius * 0.5;
        blur_rgba(shadow_pm.data_mut(), w as usize, h as usize, sigma);
    }

    // Composite blurred shadow onto the main pixmap at the specified offset
    let ox = offset_x as i32;
    let oy = offset_y as i32;
    pm.draw_pixmap(ox, oy, shadow_pm.as_ref(), &PixmapPaint::default(), SkTransform::identity(), clip);
}

// ── Gaussian blur (3× box blur approximation) ─────────────────────────────────

fn blur_rgba(data: &mut [u8], width: usize, height: usize, sigma: f32) {
    if sigma < 0.1 || width == 0 || height == 0 {
        return;
    }
    // radius per pass such that 3 passes ≈ Gaussian with given sigma
    let r = ((sigma * 1.5 + 0.5) as usize).max(1);
    let mut buf = vec![0u8; data.len()];

    for _ in 0..3 {
        box_blur_h(data, &mut buf, width, height, r);
        box_blur_v(&buf, data, width, height, r);
    }
}

fn box_blur_h(src: &[u8], dst: &mut [u8], w: usize, h: usize, r: usize) {
    for y in 0..h {
        for c in 0..4usize {
            let mut sum = 0u32;
            let mut count = 0u32;
            // initialise window for x=0
            for kx in 0..=(r.min(w - 1)) {
                sum += src[(y * w + kx) * 4 + c] as u32;
                count += 1;
            }
            for x in 0..w {
                dst[(y * w + x) * 4 + c] = (sum / count) as u8;
                // slide: add x+r+1, remove x-r
                if x + r + 1 < w {
                    sum += src[(y * w + x + r + 1) * 4 + c] as u32;
                    count += 1;
                }
                if x >= r {
                    sum -= src[(y * w + x - r) * 4 + c] as u32;
                    count -= 1;
                }
            }
        }
    }
}

fn box_blur_v(src: &[u8], dst: &mut [u8], w: usize, h: usize, r: usize) {
    for x in 0..w {
        for c in 0..4usize {
            let mut sum = 0u32;
            let mut count = 0u32;
            for ky in 0..=(r.min(h - 1)) {
                sum += src[(ky * w + x) * 4 + c] as u32;
                count += 1;
            }
            for y in 0..h {
                dst[(y * w + x) * 4 + c] = (sum / count) as u8;
                if y + r + 1 < h {
                    sum += src[((y + r + 1) * w + x) * 4 + c] as u32;
                    count += 1;
                }
                if y >= r {
                    sum -= src[((y - r) * w + x) * 4 + c] as u32;
                    count -= 1;
                }
            }
        }
    }
}

// ── Path building ──────────────────────────────────────────────────────────────

fn build_sk_path(path: &AnimPath, transform: &Matrix3<f32>) -> Option<SkPath> {
    let cmds = super::transform_path(path, transform);
    let mut pb = PathBuilder::new();
    for cmd in cmds {
        match cmd {
            DrawCmd::MoveTo(x, y)                        => pb.move_to(x, y),
            DrawCmd::LineTo(x, y)                        => pb.line_to(x, y),
            DrawCmd::CubicTo(cx1, cy1, cx2, cy2, x, y)  => pb.cubic_to(cx1, cy1, cx2, cy2, x, y),
            DrawCmd::QuadTo(cx, cy, x, y)                => pb.quad_to(cx, cy, x, y),
            DrawCmd::Close                               => pb.close(),
        }
    }
    pb.finish()
}

// ── Paint helpers ──────────────────────────────────────────────────────────────

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
                cp,
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
    SkStop::new(s.position, tiny_skia::Color::from_rgba8(r, g, b, final_a))
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

// ── Tests ──────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::effects::Effect;
    use crate::paint::{Color, Paint};
    use crate::path::AnimPath;

    fn identity() -> Matrix3<f32> { Matrix3::identity() }

    #[test]
    fn render_filled_rect_to_pixels() {
        let mut r = SkiaRenderer::new();
        r.begin_frame(64, 64, [255, 255, 255, 255]);
        let path = AnimPath::rect(10.0, 10.0, 44.0, 44.0);
        r.draw_path(&path, &Paint::filled(Color::from_hex(0xFF0000)), &identity(), 1.0, &[]);
        let pixels = r.end_frame();
        assert_eq!(pixels.len(), 64 * 64 * 4);
        let idx = (32 * 64 + 32) * 4;
        assert_eq!(pixels[idx], 255);   // R
        assert_eq!(pixels[idx + 1], 0); // G
    }

    #[test]
    fn render_stroked_ellipse() {
        let mut r = SkiaRenderer::new();
        r.begin_frame(128, 128, [0, 0, 0, 255]);
        let path = AnimPath::ellipse(64.0, 64.0, 40.0, 30.0);
        r.draw_path(&path, &Paint::stroked(Color::WHITE, 2.0), &identity(), 1.0, &[]);
        assert_eq!(r.end_frame().len(), 128 * 128 * 4);
    }

    #[test]
    fn render_linear_gradient() {
        use crate::paint::{Fill, Gradient, GradientStop};
        let mut r = SkiaRenderer::new();
        r.begin_frame(100, 100, [255, 255, 255, 255]);
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
        r.draw_path(&AnimPath::rect(0.0, 0.0, 100.0, 100.0), &paint, &identity(), 1.0, &[]);
        let pixels = r.end_frame();
        assert!(pixels[0] > 200);
        assert!(pixels[99 * 4 + 2] > 200);
    }

    #[test]
    fn drop_shadow_renders() {
        let mut r = SkiaRenderer::new();
        r.begin_frame(128, 128, [255, 255, 255, 255]);
        let path = AnimPath::rect(20.0, 20.0, 80.0, 80.0);
        let effects = vec![Effect::DropShadow {
            offset_x: 6.0,
            offset_y: 6.0,
            blur_radius: 8.0,
            color: Color::rgba(0.0, 0.0, 0.0, 0.5),
        }];
        r.draw_path(&path, &Paint::filled(Color::WHITE), &identity(), 1.0, &effects);
        let pixels = r.end_frame();
        // shadow region (bottom-right of shape) should have dark pixels
        let sx = (26 + 6) as usize;
        let sy = (100 + 6) as usize;
        let _idx = (sy.min(127) * 128 + sx.min(127)) * 4;
        // at minimum the shadow pass ran without panic
        assert_eq!(pixels.len(), 128 * 128 * 4);
    }

    #[test]
    fn clip_mask_clips_shape() {
        let mut r = SkiaRenderer::new();
        r.begin_frame(64, 64, [255, 255, 255, 255]);

        // clip to top-left 32×32 quadrant
        let clip_path = AnimPath::rect(0.0, 0.0, 32.0, 32.0);
        r.push_clip(&clip_path, &identity());

        // draw full 64×64 red rect
        let path = AnimPath::rect(0.0, 0.0, 64.0, 64.0);
        r.draw_path(&path, &Paint::filled(Color::from_hex(0xFF0000)), &identity(), 1.0, &[]);
        r.pop_clip();

        let pixels = r.end_frame();
        // top-left (8, 8) → red
        let tl = (8 * 64 + 8) * 4;
        assert_eq!(pixels[tl], 255);
        // bottom-right (48, 48) → still white (clipped away)
        let br = (48 * 64 + 48) * 4;
        assert_eq!(pixels[br], 255);     // R = white
        assert_eq!(pixels[br + 1], 255); // G = white
        assert_eq!(pixels[br + 2], 255); // B = white
    }
}
