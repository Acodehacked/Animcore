use std::fmt::Write;

use nalgebra::Matrix3;
use uuid::Uuid;

use crate::paint::{Fill, Gradient, Paint};
use crate::path::{AnimPath, PathVerb};
use crate::schema::{Artboard, Geometry, Node, ShapeData};
use crate::transform::compute_world_transforms;

/// Export an artboard as an SVG string (static frame, no animation applied).
pub fn to_svg_str(artboard: &Artboard) -> String {
    let world = compute_world_transforms(&artboard.nodes);
    let mut defs = String::new();
    let mut body = String::new();
    let mut grad_counter = 0usize;

    for node in &artboard.nodes {
        if !node.visible {
            continue;
        }
        let shape = match &node.shape {
            Some(s) => s,
            None => continue,
        };
        let world_mat = world.get(&node.id).copied().unwrap_or(Matrix3::identity());
        write_node(node, shape, &world_mat, &mut body, &mut defs, &mut grad_counter);
    }

    let mut out = String::new();
    let _ = write!(
        out,
        r#"<svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink" width="{}" height="{}">"#,
        artboard.width, artboard.height
    );

    // Background rect — color written manually to avoid "# confusion in format strings
    let bg = artboard.background.to_u8();
    out.push_str(&bg_rect(artboard.width, artboard.height, bg));

    if !defs.is_empty() {
        out.push_str("<defs>");
        out.push_str(&defs);
        out.push_str("</defs>");
    }
    out.push_str(&body);
    out.push_str("</svg>");
    out
}

fn bg_rect(w: f32, h: f32, [r, g, b, a]: [u8; 4]) -> String {
    let color = hex_color(r, g, b);
    let alpha = a as f32 / 255.0;
    format!(
        r#"<rect width="{}" height="{}" fill="{}" fill-opacity="{:.4}"/>"#,
        w, h, color, alpha
    )
}

// ── Node writer ───────────────────────────────────────────────────────────────

fn write_node(
    node: &Node,
    shape: &ShapeData,
    world: &Matrix3<f32>,
    body: &mut String,
    defs: &mut String,
    counter: &mut usize,
) {
    let t = matrix_to_svg_transform(world);
    let fill = paint_fill_attr(&shape.paint, defs, counter);
    let stroke = paint_stroke_attrs(&shape.paint);
    let opacity = if (node.opacity - 1.0).abs() > 0.001 {
        format!(r#" opacity="{:.4}""#, node.opacity)
    } else {
        String::new()
    };
    let id = format!(r#" id="{}""#, node.name);

    match &shape.geometry {
        Geometry::Rect { width, height, corner_radius } => {
            let rx = if *corner_radius > 0.0 {
                format!(r#" rx="{}" ry="{}""#, corner_radius, corner_radius)
            } else {
                String::new()
            };
            let _ = write!(
                body,
                r#"<rect{id} width="{w}" height="{h}"{rx} transform="{t}"{fill}{stroke}{opacity}/>"#,
                id = id, w = width, h = height, rx = rx,
                t = t, fill = fill, stroke = stroke, opacity = opacity
            );
        }
        Geometry::Ellipse { radius_x, radius_y } => {
            let _ = write!(
                body,
                r#"<ellipse{id} rx="{rx}" ry="{ry}" transform="{t}"{fill}{stroke}{opacity}/>"#,
                id = id, rx = radius_x, ry = radius_y,
                t = t, fill = fill, stroke = stroke, opacity = opacity
            );
        }
        Geometry::Path(path) => {
            let d = path_to_svg_d(path);
            let _ = write!(
                body,
                r#"<path{id} d="{d}" transform="{t}"{fill}{stroke}{opacity}/>"#,
                id = id, d = d, t = t, fill = fill, stroke = stroke, opacity = opacity
            );
        }
    }
}

// ── Fill / stroke attribute builders ─────────────────────────────────────────

fn paint_fill_attr(paint: &Paint, defs: &mut String, counter: &mut usize) -> String {
    match &paint.fill {
        Fill::None => r#" fill="none""#.to_string(),
        Fill::Solid(c) => {
            let [r, g, b, a] = c.to_u8();
            let alpha = a as f32 / 255.0;
            let color = hex_color(r, g, b);
            if (alpha - 1.0).abs() < 0.004 {
                format!(r#" fill="{}""#, color)
            } else {
                format!(r#" fill="{}" fill-opacity="{:.4}""#, color, alpha)
            }
        }
        Fill::Gradient(grad) => {
            let id = format!("grad{}", *counter);
            *counter += 1;
            write_gradient_def(grad, &id, defs);
            format!(r#" fill="url(#{})""#, id)
        }
    }
}

fn write_gradient_def(grad: &Gradient, id: &str, defs: &mut String) {
    match grad {
        Gradient::Linear { start, end, stops } => {
            let _ = write!(
                defs,
                r#"<linearGradient id="{id}" x1="{}" y1="{}" x2="{}" y2="{}" gradientUnits="userSpaceOnUse">"#,
                start[0], start[1], end[0], end[1]
            );
            for s in stops {
                let [r, g, b, a] = s.color.to_u8();
                let _ = write!(
                    defs,
                    r#"<stop offset="{:.4}" stop-color="{}" stop-opacity="{:.4}"/>"#,
                    s.position, hex_color(r, g, b), a as f32 / 255.0
                );
            }
            defs.push_str("</linearGradient>");
        }
        Gradient::Radial { center, radius, stops } => {
            let _ = write!(
                defs,
                r#"<radialGradient id="{id}" cx="{}" cy="{}" r="{}" gradientUnits="userSpaceOnUse">"#,
                center[0], center[1], radius
            );
            for s in stops {
                let [r, g, b, a] = s.color.to_u8();
                let _ = write!(
                    defs,
                    r#"<stop offset="{:.4}" stop-color="{}" stop-opacity="{:.4}"/>"#,
                    s.position, hex_color(r, g, b), a as f32 / 255.0
                );
            }
            defs.push_str("</radialGradient>");
        }
    }
}

fn paint_stroke_attrs(paint: &Paint) -> String {
    match &paint.stroke {
        None => r#" stroke="none""#.to_string(),
        Some(s) => {
            let color_attr = match &s.fill {
                Fill::None => return r#" stroke="none""#.to_string(),
                Fill::Solid(c) => {
                    let [r, g, b, a] = c.to_u8();
                    let alpha = a as f32 / 255.0;
                    let color = hex_color(r, g, b);
                    if (alpha - 1.0).abs() < 0.004 {
                        format!(r#" stroke="{}""#, color)
                    } else {
                        format!(r#" stroke="{}" stroke-opacity="{:.4}""#, color, alpha)
                    }
                }
                Fill::Gradient(_) => return r#" stroke="none""#.to_string(),
            };
            let cap = match s.cap {
                crate::paint::StrokeCap::Butt   => "butt",
                crate::paint::StrokeCap::Round  => "round",
                crate::paint::StrokeCap::Square => "square",
            };
            let join = match s.join {
                crate::paint::StrokeJoin::Miter => "miter",
                crate::paint::StrokeJoin::Round => "round",
                crate::paint::StrokeJoin::Bevel => "bevel",
            };
            format!(
                r#"{} stroke-width="{}" stroke-linecap="{}" stroke-linejoin="{}""#,
                color_attr, s.width, cap, join
            )
        }
    }
}

// ── Path → SVG d attribute ────────────────────────────────────────────────────

fn path_to_svg_d(path: &AnimPath) -> String {
    let mut out = String::new();
    let mut pi = 0usize;

    for verb in &path.verbs {
        match verb {
            PathVerb::MoveTo => {
                let [x, y] = path.points[pi];
                let _ = write!(out, "M {:.4} {:.4} ", x, y);
                pi += 1;
            }
            PathVerb::LineTo => {
                let [x, y] = path.points[pi];
                let _ = write!(out, "L {:.4} {:.4} ", x, y);
                pi += 1;
            }
            PathVerb::CubicTo => {
                let [cx1, cy1] = path.points[pi];
                let [cx2, cy2] = path.points[pi + 1];
                let [x, y]     = path.points[pi + 2];
                let _ = write!(out, "C {:.4} {:.4} {:.4} {:.4} {:.4} {:.4} ",
                    cx1, cy1, cx2, cy2, x, y);
                pi += 3;
            }
            PathVerb::QuadTo => {
                let [cx, cy] = path.points[pi];
                let [x, y]   = path.points[pi + 1];
                let _ = write!(out, "Q {:.4} {:.4} {:.4} {:.4} ", cx, cy, x, y);
                pi += 2;
            }
            PathVerb::Close => out.push_str("Z "),
        }
    }
    out.trim_end().to_string()
}

// ── Utilities ─────────────────────────────────────────────────────────────────

fn hex_color(r: u8, g: u8, b: u8) -> String {
    format!("#{:02X}{:02X}{:02X}", r, g, b)
}

fn matrix_to_svg_transform(m: &Matrix3<f32>) -> String {
    // SVG matrix(a,b,c,d,e,f): a=m[0,0] b=m[1,0] c=m[0,1] d=m[1,1] e=m[0,2] f=m[1,2]
    format!(
        "matrix({:.6},{:.6},{:.6},{:.6},{:.6},{:.6})",
        m[(0, 0)], m[(1, 0)], m[(0, 1)], m[(1, 1)], m[(0, 2)], m[(1, 2)]
    )
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::paint::{Color, Paint};
    use crate::schema::{Artboard, Geometry, Node, ShapeData};
    use crate::transform::Transform;

    fn simple_artboard() -> Artboard {
        let mut node = Node::new("rect1");
        node.transform = Transform::translation(10.0, 10.0);
        node.shape = Some(ShapeData {
            geometry: Geometry::Rect { width: 80.0, height: 40.0, corner_radius: 0.0 },
            paint: Paint::filled(Color::from_hex(0xFF0000)),
        });
        Artboard {
            id: Uuid::new_v4(),
            name: "Test".into(),
            width: 200.0,
            height: 100.0,
            background: Color::WHITE,
            nodes: vec![node],
            animations: vec![],
        }
    }

    #[test]
    fn export_contains_rect() {
        let svg = to_svg_str(&simple_artboard());
        assert!(svg.contains("<rect"));
        assert!(svg.contains("</svg>"));
        assert!(svg.contains("#FF0000") || svg.contains("#ff0000"));
    }

    #[test]
    fn export_roundtrip_dimensions() {
        let svg = to_svg_str(&simple_artboard());
        assert!(svg.contains("width=\"200\""));
        assert!(svg.contains("height=\"100\""));
    }

    #[test]
    fn export_gradient() {
        use crate::paint::{Fill, Gradient, GradientStop};
        let mut node = Node::new("g1");
        node.shape = Some(ShapeData {
            geometry: Geometry::Rect { width: 100.0, height: 100.0, corner_radius: 0.0 },
            paint: Paint {
                fill: Fill::Gradient(Gradient::Linear {
                    start: [0.0, 0.0],
                    end: [100.0, 0.0],
                    stops: vec![
                        GradientStop { position: 0.0, color: Color::from_hex(0xFF0000) },
                        GradientStop { position: 1.0, color: Color::from_hex(0x0000FF) },
                    ],
                }),
                ..Default::default()
            },
        });
        let mut ab = simple_artboard();
        ab.nodes = vec![node];
        let svg = to_svg_str(&ab);
        assert!(svg.contains("linearGradient"));
        assert!(svg.contains("url(#"));
    }
}
