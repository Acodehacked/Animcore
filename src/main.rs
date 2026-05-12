/// animcore is the animation engine 
/// it is not a UI-framework, it does not know anything about pixels, windows, 
/// input, svg or html
/// the core concepts are nodes, shapes, transforms, paths and animations
/// built by Abin Antony Kattady

mod paint;
mod path;
mod playback;
mod renderer;
mod scene;
mod schema;
mod transform;

use paint::{Color, Paint};
use renderer::{skia::SkiaRenderer, Renderer};
use scene::Scene;
use schema::*;
use transform::Transform;
use uuid::Uuid;

fn main() {
    // Build a simple artboard: a red rect that slides right, then fades out
    let rect_id = Uuid::new_v4();

    let mut rect_node = Node::new("RedRect");
    rect_node.id = rect_id;
    rect_node.transform = Transform::translation(20.0, 80.0);
    rect_node.shape = Some(ShapeData {
        geometry: Geometry::Rect { width: 120.0, height: 80.0, corner_radius: 12.0 },
        paint: Paint::filled(Color::from_hex(0xFF4444)),
    });

    let circle_id = Uuid::new_v4();
    let mut circle_node = Node::new("BlueCircle");
    circle_node.id = circle_id;
    circle_node.transform = Transform::translation(300.0, 120.0);
    circle_node.shape = Some(ShapeData {
        geometry: Geometry::Ellipse { radius_x: 60.0, radius_y: 60.0 },
        paint: Paint::filled(Color::from_hex(0x4488FF)),
    });

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

    let mut scene = Scene::new(artboard);
    scene.play("slide");

    // Render 3 sample frames and save as raw RGBA PNG-like blobs
    let mut renderer = SkiaRenderer::new();
    let frames = [0.0f32, 0.5, 1.0];

    for t in &frames {
        // rewind
        if let Some(p) = &mut scene.player { p.time = *t; }
        scene.render(&mut renderer);
        let pixels = renderer.end_frame();
        let filename = format!("frame_{:.1}s.raw", t);
        std::fs::write(&filename, &pixels).unwrap();
        println!("wrote {} ({} bytes)", filename, pixels.len());
    }

    println!("Phase 1 complete — paint, paths, transforms, CPU renderer all wired up.");
}
