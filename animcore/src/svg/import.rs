use std::collections::HashMap;

use roxmltree::{Document, Node as XmlNode};
use uuid::Uuid;

use crate::paint::{Color, Fill, Paint, Stroke, StrokeCap, StrokeJoin};
use crate::path::AnimPath;
use crate::schema::{Artboard, Geometry, Node, ShapeData};
use crate::transform::Transform;

#[derive(Debug)]
pub struct SvgImportError(pub String);

impl std::fmt::Display for SvgImportError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "SVG import error: {}", self.0)
    }
}

impl From<roxmltree::Error> for SvgImportError {
    fn from(e: roxmltree::Error) -> Self {
        Self(e.to_string())
    }
}

/// Parse an SVG string into an AnimCore Artboard.
pub fn from_svg_str(svg: &str) -> Result<Artboard, SvgImportError> {
    let doc = Document::parse(svg)?;
    let root = doc.root_element();

    let width = root
        .attribute("width")
        .and_then(|v| parse_length(v))
        .unwrap_or(100.0);
    let height = root
        .attribute("height")
        .and_then(|v| parse_length(v))
        .unwrap_or(100.0);

    let mut nodes: Vec<Node> = Vec::new();
    let inherited = InheritedStyle::default();

    for child in root.children().filter(|n| n.is_element()) {
        process_element(&child, None, &mut nodes, &inherited);
    }

    Ok(Artboard {
        id: Uuid::new_v4(),
        name: root.attribute("id").unwrap_or("Imported").to_string(),
        width,
        height,
        background: Color::WHITE,
        nodes,
        animations: vec![],
        constraints: vec![],
    })
}

// ── Element processing ────────────────────────────────────────────────────────

#[derive(Clone)]
struct InheritedStyle {
    fill: Fill,
    stroke: Option<Stroke>,
    opacity: f32,
}

impl Default for InheritedStyle {
    fn default() -> Self {
        Self {
            fill: Fill::Solid(Color::BLACK),
            stroke: None,
            opacity: 1.0,
        }
    }
}

fn process_element(
    xml: &XmlNode,
    parent_id: Option<Uuid>,
    nodes: &mut Vec<Node>,
    inherited: &InheritedStyle,
) {
    let tag = xml.tag_name().name();
    let style = parse_style_attrs(xml, inherited);

    match tag {
        "g" => {
            let id = Uuid::new_v4();
            let name = xml.attribute("id").unwrap_or("group").to_string();
            let transform = parse_transform_attr(xml);

            let mut group = Node::new(name);
            group.id = id;
            group.transform = transform;
            group.parent_id = parent_id;
            group.opacity = style.opacity;
            nodes.push(group);

            for child in xml.children().filter(|n| n.is_element()) {
                process_element(&child, Some(id), nodes, &style);
            }
        }
        "rect" => {
            if let Some(node) = parse_rect(xml, parent_id, &style) {
                nodes.push(node);
            }
        }
        "circle" => {
            if let Some(node) = parse_circle(xml, parent_id, &style) {
                nodes.push(node);
            }
        }
        "ellipse" => {
            if let Some(node) = parse_ellipse(xml, parent_id, &style) {
                nodes.push(node);
            }
        }
        "path" => {
            if let Some(node) = parse_path_elem(xml, parent_id, &style) {
                nodes.push(node);
            }
        }
        "polygon" | "polyline" => {
            if let Some(node) = parse_poly(xml, parent_id, &style, tag == "polygon") {
                nodes.push(node);
            }
        }
        "line" => {
            if let Some(node) = parse_line(xml, parent_id, &style) {
                nodes.push(node);
            }
        }
        _ => {}
    }
}

// ── Shape parsers ─────────────────────────────────────────────────────────────

fn parse_rect(xml: &XmlNode, parent_id: Option<Uuid>, style: &InheritedStyle) -> Option<Node> {
    let x = attr_f32(xml, "x").unwrap_or(0.0);
    let y = attr_f32(xml, "y").unwrap_or(0.0);
    let w = attr_f32(xml, "width")?;
    let h = attr_f32(xml, "height")?;
    let rx = attr_f32(xml, "rx").or_else(|| attr_f32(xml, "ry")).unwrap_or(0.0);

    let mut node = Node::new(xml.attribute("id").unwrap_or("rect"));
    node.parent_id = parent_id;
    node.transform = compose_transform(parse_transform_attr(xml), x, y);
    node.opacity = style.opacity;
    node.shape = Some(ShapeData {
        geometry: Geometry::Rect { width: w, height: h, corner_radius: rx },
        paint: make_paint(style),
    });
    Some(node)
}

fn parse_circle(xml: &XmlNode, parent_id: Option<Uuid>, style: &InheritedStyle) -> Option<Node> {
    let cx = attr_f32(xml, "cx").unwrap_or(0.0);
    let cy = attr_f32(xml, "cy").unwrap_or(0.0);
    let r = attr_f32(xml, "r")?;

    let mut node = Node::new(xml.attribute("id").unwrap_or("circle"));
    node.parent_id = parent_id;
    node.transform = compose_transform(parse_transform_attr(xml), cx, cy);
    node.opacity = style.opacity;
    node.shape = Some(ShapeData {
        geometry: Geometry::Ellipse { radius_x: r, radius_y: r },
        paint: make_paint(style),
    });
    Some(node)
}

fn parse_ellipse(xml: &XmlNode, parent_id: Option<Uuid>, style: &InheritedStyle) -> Option<Node> {
    let cx = attr_f32(xml, "cx").unwrap_or(0.0);
    let cy = attr_f32(xml, "cy").unwrap_or(0.0);
    let rx = attr_f32(xml, "rx")?;
    let ry = attr_f32(xml, "ry")?;

    let mut node = Node::new(xml.attribute("id").unwrap_or("ellipse"));
    node.parent_id = parent_id;
    node.transform = compose_transform(parse_transform_attr(xml), cx, cy);
    node.opacity = style.opacity;
    node.shape = Some(ShapeData {
        geometry: Geometry::Ellipse { radius_x: rx, radius_y: ry },
        paint: make_paint(style),
    });
    Some(node)
}

fn parse_path_elem(xml: &XmlNode, parent_id: Option<Uuid>, style: &InheritedStyle) -> Option<Node> {
    let d = xml.attribute("d")?;
    let path = AnimPath::from_svg_d(d).ok()?;

    let mut node = Node::new(xml.attribute("id").unwrap_or("path"));
    node.parent_id = parent_id;
    node.transform = parse_transform_attr(xml);
    node.opacity = style.opacity;
    node.shape = Some(ShapeData {
        geometry: Geometry::Path(path),
        paint: make_paint(style),
    });
    Some(node)
}

fn parse_poly(
    xml: &XmlNode,
    parent_id: Option<Uuid>,
    style: &InheritedStyle,
    closed: bool,
) -> Option<Node> {
    let points_str = xml.attribute("points")?;
    let coords: Vec<f32> = points_str
        .split_whitespace()
        .flat_map(|s| s.split(','))
        .filter_map(|s| s.parse::<f32>().ok())
        .collect();

    if coords.len() < 4 || coords.len() % 2 != 0 {
        return None;
    }

    let mut path = AnimPath::new();
    path.move_to(coords[0], coords[1]);
    for pair in coords[2..].chunks(2) {
        path.line_to(pair[0], pair[1]);
    }
    if closed {
        path.close();
    }

    let mut node = Node::new(xml.attribute("id").unwrap_or(if closed { "polygon" } else { "polyline" }));
    node.parent_id = parent_id;
    node.transform = parse_transform_attr(xml);
    node.opacity = style.opacity;
    node.shape = Some(ShapeData {
        geometry: Geometry::Path(path),
        paint: make_paint(style),
    });
    Some(node)
}

fn parse_line(xml: &XmlNode, parent_id: Option<Uuid>, style: &InheritedStyle) -> Option<Node> {
    let x1 = attr_f32(xml, "x1").unwrap_or(0.0);
    let y1 = attr_f32(xml, "y1").unwrap_or(0.0);
    let x2 = attr_f32(xml, "x2").unwrap_or(0.0);
    let y2 = attr_f32(xml, "y2").unwrap_or(0.0);

    let mut path = AnimPath::new();
    path.move_to(x1, y1).line_to(x2, y2);

    let mut node = Node::new(xml.attribute("id").unwrap_or("line"));
    node.parent_id = parent_id;
    node.transform = parse_transform_attr(xml);
    node.opacity = style.opacity;
    node.shape = Some(ShapeData {
        geometry: Geometry::Path(path),
        paint: make_paint(style),
    });
    Some(node)
}

// ── Style resolution ──────────────────────────────────────────────────────────

fn parse_style_attrs(xml: &XmlNode, inherited: &InheritedStyle) -> InheritedStyle {
    // Collect all attribute overrides, merging inline `style=""` as well
    let mut attrs: HashMap<&str, String> = HashMap::new();

    // Presentation attributes (lower priority)
    for attr in xml.attributes() {
        attrs.insert(attr.name(), attr.value().to_string());
    }
    // Inline style overrides presentation attributes
    if let Some(style_str) = xml.attribute("style") {
        for decl in style_str.split(';') {
            if let Some((k, v)) = decl.split_once(':') {
                attrs.insert(k.trim(), v.trim().to_string());
            }
        }
    }

    let fill = attrs
        .get("fill")
        .map(|v| parse_fill_value(v))
        .unwrap_or_else(|| inherited.fill.clone());

    let stroke_color = attrs.get("stroke").and_then(|v| parse_color(v));
    let stroke_width = attrs.get("stroke-width").and_then(|v| v.parse::<f32>().ok()).unwrap_or(1.0);
    let stroke = stroke_color.map(|c| {
        let mut s = Stroke::solid(c, stroke_width);
        s.cap = attrs.get("stroke-linecap").map(|v| match v.as_str() {
            "round"  => StrokeCap::Round,
            "square" => StrokeCap::Square,
            _        => StrokeCap::Butt,
        }).unwrap_or(StrokeCap::Butt);
        s.join = attrs.get("stroke-linejoin").map(|v| match v.as_str() {
            "round" => StrokeJoin::Round,
            "bevel" => StrokeJoin::Bevel,
            _       => StrokeJoin::Miter,
        }).unwrap_or(StrokeJoin::Miter);
        s
    }).or_else(|| inherited.stroke.clone());

    let opacity = attrs
        .get("opacity")
        .and_then(|v| v.parse::<f32>().ok())
        .unwrap_or(inherited.opacity);

    InheritedStyle { fill, stroke, opacity }
}

fn parse_fill_value(v: &str) -> Fill {
    match v.trim() {
        "none" | "transparent" => Fill::None,
        s => parse_color(s).map(Fill::Solid).unwrap_or(Fill::None),
    }
}

fn make_paint(style: &InheritedStyle) -> Paint {
    Paint {
        fill: style.fill.clone(),
        stroke: style.stroke.clone(),
        opacity: 1.0,
        blend_mode: crate::paint::BlendMode::Normal,
    }
}

// ── Transform parsing ─────────────────────────────────────────────────────────

fn parse_transform_attr(xml: &XmlNode) -> Transform {
    xml.attribute("transform")
        .map(|s| parse_transform_str(s))
        .unwrap_or_default()
}

pub fn parse_transform_str(s: &str) -> Transform {
    let s = s.trim();
    // multiple transforms are applied right-to-left; compose into one matrix
    let mut mat = nalgebra::Matrix3::identity();

    let mut i = 0;
    while i < s.len() {
        // find next function name
        let rest = &s[i..];
        if let Some(paren) = rest.find('(') {
            let end = rest[paren..].find(')').map(|e| paren + e).unwrap_or(rest.len());
            let name = rest[..paren].trim();
            let args_str = &rest[paren + 1..end];
            let args: Vec<f32> = args_str
                .split(|c: char| c == ',' || c.is_whitespace())
                .filter(|s| !s.is_empty())
                .filter_map(|s| s.parse::<f32>().ok())
                .collect();

            let local = match name {
                "translate" => {
                    let tx = args.first().copied().unwrap_or(0.0);
                    let ty = args.get(1).copied().unwrap_or(0.0);
                    Transform::translation(tx, ty).to_matrix()
                }
                "scale" => {
                    let sx = args.first().copied().unwrap_or(1.0);
                    let sy = args.get(1).copied().unwrap_or(sx);
                    Transform { scale_x: sx, scale_y: sy, ..Default::default() }.to_matrix()
                }
                "rotate" => {
                    let angle = args.first().copied().unwrap_or(0.0).to_radians();
                    if args.len() >= 3 {
                        // rotate(angle, cx, cy) = translate(cx,cy) rotate(angle) translate(-cx,-cy)
                        let cx = args[1];
                        let cy = args[2];
                        let t1 = Transform::translation(cx, cy).to_matrix();
                        let r  = Transform { rotation: angle, ..Default::default() }.to_matrix();
                        let t2 = Transform::translation(-cx, -cy).to_matrix();
                        t1 * r * t2
                    } else {
                        Transform { rotation: angle, ..Default::default() }.to_matrix()
                    }
                }
                "skewX" => {
                    let angle = args.first().copied().unwrap_or(0.0).to_radians();
                    Transform { skew_x: angle, ..Default::default() }.to_matrix()
                }
                "skewY" => {
                    let angle = args.first().copied().unwrap_or(0.0).to_radians();
                    Transform { skew_y: angle, ..Default::default() }.to_matrix()
                }
                "matrix" if args.len() == 6 => {
                    // SVG matrix(a,b,c,d,e,f):
                    //  [a c e]
                    //  [b d f]
                    //  [0 0 1]
                    nalgebra::Matrix3::new(
                        args[0], args[2], args[4],
                        args[1], args[3], args[5],
                        0.0,     0.0,     1.0,
                    )
                }
                _ => nalgebra::Matrix3::identity(),
            };

            mat = mat * local;
            i += end + 1;
        } else {
            break;
        }
    }

    Transform::from_matrix(&mat)
}

fn compose_transform(base: Transform, shift_x: f32, shift_y: f32) -> Transform {
    // For shapes that use cx/cy or x/y as a positional offset, bake it in.
    let mut t = base;
    t.x += shift_x;
    t.y += shift_y;
    t
}

// ── Color parsing ─────────────────────────────────────────────────────────────

pub fn parse_color(s: &str) -> Option<Color> {
    let s = s.trim();
    if s == "none" || s == "transparent" {
        return None;
    }
    if s.starts_with('#') {
        return parse_hex_color(s);
    }
    if s.starts_with("rgb(") || s.starts_with("rgba(") {
        return parse_rgb_color(s);
    }
    named_color(s)
}

fn parse_hex_color(s: &str) -> Option<Color> {
    let hex = s.trim_start_matches('#');
    match hex.len() {
        3 => {
            let r = u8::from_str_radix(&hex[0..1].repeat(2), 16).ok()?;
            let g = u8::from_str_radix(&hex[1..2].repeat(2), 16).ok()?;
            let b = u8::from_str_radix(&hex[2..3].repeat(2), 16).ok()?;
            Some(Color::rgba(r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0, 1.0))
        }
        6 => {
            let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
            let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
            let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
            Some(Color::rgba(r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0, 1.0))
        }
        8 => {
            let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
            let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
            let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
            let a = u8::from_str_radix(&hex[6..8], 16).ok()?;
            Some(Color::rgba(r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0, a as f32 / 255.0))
        }
        _ => None,
    }
}

fn parse_rgb_color(s: &str) -> Option<Color> {
    let inner = s
        .trim_start_matches("rgba(")
        .trim_start_matches("rgb(")
        .trim_end_matches(')');
    let parts: Vec<f32> = inner
        .split(',')
        .filter_map(|p| {
            let p = p.trim();
            if p.ends_with('%') {
                p.trim_end_matches('%').parse::<f32>().ok().map(|v| v / 100.0)
            } else {
                p.parse::<f32>().ok().map(|v| v / 255.0)
            }
        })
        .collect();

    match parts.len() {
        3 => Some(Color::rgba(parts[0], parts[1], parts[2], 1.0)),
        4 => Some(Color::rgba(parts[0], parts[1], parts[2], parts[3])),
        _ => None,
    }
}

fn named_color(name: &str) -> Option<Color> {
    Some(match name.to_lowercase().as_str() {
        "black"   => Color::BLACK,
        "white"   => Color::WHITE,
        "red"     => Color::from_hex(0xFF0000),
        "green"   => Color::from_hex(0x008000),
        "lime"    => Color::from_hex(0x00FF00),
        "blue"    => Color::from_hex(0x0000FF),
        "yellow"  => Color::from_hex(0xFFFF00),
        "cyan"    => Color::from_hex(0x00FFFF),
        "magenta" | "fuchsia" => Color::from_hex(0xFF00FF),
        "orange"  => Color::from_hex(0xFF8000),
        "purple"  => Color::from_hex(0x800080),
        "gray" | "grey" => Color::from_hex(0x808080),
        "silver"  => Color::from_hex(0xC0C0C0),
        "maroon"  => Color::from_hex(0x800000),
        "navy"    => Color::from_hex(0x000080),
        "teal"    => Color::from_hex(0x008080),
        "olive"   => Color::from_hex(0x808000),
        "pink"    => Color::from_hex(0xFFC0CB),
        "brown"   => Color::from_hex(0xA52A2A),
        _ => return None,
    })
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn attr_f32(xml: &XmlNode, name: &str) -> Option<f32> {
    xml.attribute(name)?.parse::<f32>().ok()
}

fn parse_length(s: &str) -> Option<f32> {
    s.trim_end_matches(|c: char| c.is_alphabetic()).parse::<f32>().ok()
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn import_simple_rect() {
        let svg = r##"<svg xmlns="http://www.w3.org/2000/svg" width="200" height="100">
            <rect x="10" y="10" width="80" height="60" fill="#FF0000"/>
        </svg>"##;
        let ab = from_svg_str(svg).unwrap();
        assert_eq!(ab.width, 200.0);
        assert_eq!(ab.nodes.len(), 1);
        let shape = ab.nodes[0].shape.as_ref().unwrap();
        assert!(matches!(shape.geometry, Geometry::Rect { .. }));
    }

    #[test]
    fn import_group_with_children() {
        let svg = r#"<svg xmlns="http://www.w3.org/2000/svg" width="200" height="200">
            <g id="group1">
                <circle cx="50" cy="50" r="30" fill="blue"/>
                <rect width="40" height="20" fill="red"/>
            </g>
        </svg>"#;
        let ab = from_svg_str(svg).unwrap();
        // group + 2 children = 3 nodes
        assert_eq!(ab.nodes.len(), 3);
        assert!(ab.nodes[1].parent_id == Some(ab.nodes[0].id));
    }

    #[test]
    fn import_path_elem() {
        let svg = r##"<svg xmlns="http://www.w3.org/2000/svg" width="100" height="100">
            <path d="M 10 10 L 90 10 L 90 90 Z" fill="#00FF00"/>
        </svg>"##;
        let ab = from_svg_str(svg).unwrap();
        assert_eq!(ab.nodes.len(), 1);
        assert!(matches!(ab.nodes[0].shape.as_ref().unwrap().geometry, Geometry::Path(_)));
    }

    #[test]
    fn parse_transform_translate() {
        let t = parse_transform_str("translate(10, 20)");
        assert!((t.x - 10.0).abs() < 0.01);
        assert!((t.y - 20.0).abs() < 0.01);
    }

    #[test]
    fn parse_hex_colors() {
        assert!(parse_color("#FF0000").is_some());
        assert!(parse_color("#F00").is_some());
        assert!(parse_color("none").is_none());
        assert!(parse_color("red").is_some());
    }
}
