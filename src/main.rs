/// animcore — Rive-equivalent animation engine in Rust
/// built by Abin Antony Kattady

mod effects;
mod paint;
mod path;
mod playback;
mod renderer;
mod scene;
mod schema;
mod svg;
mod transform;

use effects::Effect;
use paint::{Color, Paint};
use renderer::{skia::SkiaRenderer, Renderer};
use scene::Scene;
use schema::*;
use transform::Transform;
use uuid::Uuid;

fn main() {
    // ── Build artboard ────────────────────────────────────────────────────
    let rect_id = Uuid::new_v4();
    let mut rect_node = Node::new("RedRect");
    rect_node.id = rect_id;
    rect_node.transform = Transform::translation(20.0, 80.0);
    rect_node.shape = Some(ShapeData {
        geometry: Geometry::Rect { width: 120.0, height: 80.0, corner_radius: 12.0 },
        paint: Paint::filled(Color::from_hex(0xFF4444)),
    });
    // Drop shadow on the rect
    rect_node.effects = vec![Effect::DropShadow {
        offset_x: 6.0,
        offset_y: 8.0,
        blur_radius: 16.0,
        color: Color::rgba(0.0, 0.0, 0.0, 0.45),
    }];

    let circle_id = Uuid::new_v4();
    let mut circle_node = Node::new("BlueCircle");
    circle_node.id = circle_id;
    circle_node.transform = Transform::translation(300.0, 120.0);
    circle_node.shape = Some(ShapeData {
        geometry: Geometry::Ellipse { radius_x: 60.0, radius_y: 60.0 },
        paint: Paint::filled(Color::from_hex(0x4488FF)),
    });
    circle_node.effects = vec![Effect::OuterGlow {
        blur_radius: 20.0,
        color: Color::rgba(0.27, 0.53, 1.0, 0.6),
        opacity: 0.8,
    }];

    let slide_anim = Animation {
        id: Uuid::new_v4(),
        name: "slide".into(),
        duration_secs: 2.0,
        fps: 60,
        loop_mode: LoopMode::Loop,
        tracks: vec![
            Track {
                node_id: rect_id,
                property: Property::X,
                keyframes: vec![
                    Keyframe { time_secs: 0.0, value: 20.0,  easing: Easing::CubicBezier(0.25, 0.1, 0.25, 1.0) },
                    Keyframe { time_secs: 1.0, value: 220.0, easing: Easing::CubicBezier(0.25, 0.1, 0.25, 1.0) },
                    Keyframe { time_secs: 2.0, value: 20.0,  easing: Easing::Linear },
                ],
            },
            Track {
                node_id: circle_id,
                property: Property::ScaleX,
                keyframes: vec![
                    Keyframe { time_secs: 0.0, value: 1.0, easing: Easing::CubicBezier(0.4, 0.0, 0.2, 1.0) },
                    Keyframe { time_secs: 1.0, value: 1.4, easing: Easing::CubicBezier(0.4, 0.0, 0.2, 1.0) },
                    Keyframe { time_secs: 2.0, value: 1.0, easing: Easing::Linear },
                ],
            },
            Track {
                node_id: circle_id,
                property: Property::ScaleY,
                keyframes: vec![
                    Keyframe { time_secs: 0.0, value: 1.0, easing: Easing::CubicBezier(0.4, 0.0, 0.2, 1.0) },
                    Keyframe { time_secs: 1.0, value: 1.4, easing: Easing::CubicBezier(0.4, 0.0, 0.2, 1.0) },
                    Keyframe { time_secs: 2.0, value: 1.0, easing: Easing::Linear },
                ],
            },
        ],
    };

    let artboard = Artboard {
        id: Uuid::new_v4(),
        name: "Main".into(),
        width: 480.0,
        height: 270.0,
        background: Color::rgba(0.12, 0.12, 0.16, 1.0),
        nodes: vec![rect_node, circle_node],
        animations: vec![slide_anim],
    };

    // ── SVG round-trip demo ───────────────────────────────────────────────
    let exported = svg::export::to_svg_str(&artboard);
    std::fs::write("demo_export.svg", &exported).unwrap();
    println!("SVG export: {} bytes → demo_export.svg", exported.len());

    let reimported = svg::import::from_svg_str(&exported).expect("re-import failed");
    println!("SVG re-import: {} nodes", reimported.nodes.len());

    // ── Render sample frames ──────────────────────────────────────────────
    let mut scene = Scene::new(artboard);
    scene.play("slide");

    let mut rend = SkiaRenderer::new();

    for t in &[0.0f32, 0.5, 1.0] {
        if let Some(p) = &mut scene.player { p.time = *t; }
        scene.render(&mut rend);
        let pixels = rend.end_frame();
        let fname = format!("frame_{:.1}s.raw", t);
        std::fs::write(&fname, &pixels).unwrap();
        println!("wrote {} ({} bytes, {}×{}px)", fname, pixels.len(), 480, 270);
    }

    println!("\nPhase 2 complete — effects, clip masks, SVG import/export all live.");
}
