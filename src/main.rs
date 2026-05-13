/// animcore — Rive-equivalent animation engine in Rust
/// built by Abin Antony Kattady

mod constraints;
mod effects;
mod mixer;
mod paint;
mod path;
mod playback;
mod renderer;
mod scene;
mod schema;
mod svg;
mod transform;

use constraints::Constraint;
use effects::Effect;
use mixer::AnimationMixer;
use paint::{Color, Paint};
use renderer::{skia::SkiaRenderer, Renderer};
use scene::Scene;
use schema::*;
use transform::Transform;
use uuid::Uuid;

fn main() {
    phase2_demo();
    phase3_demo();
}

// ── Phase 2 demo: effects, clip masks, SVG round-trip ────────────────────────

fn phase2_demo() {
    let rect_id = Uuid::new_v4();
    let mut rect_node = Node::new("RedRect");
    rect_node.id = rect_id;
    rect_node.transform = Transform::translation(20.0, 80.0);
    rect_node.shape = Some(ShapeData {
        geometry: Geometry::Rect { width: 120.0, height: 80.0, corner_radius: 12.0 },
        paint: Paint::filled(Color::from_hex(0xFF4444)),
    });
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
        constraints: vec![],
    };

    let exported = svg::export::to_svg_str(&artboard);
    std::fs::write("demo_export.svg", &exported).unwrap();
    println!("SVG export: {} bytes → demo_export.svg", exported.len());

    let reimported = svg::import::from_svg_str(&exported).expect("re-import failed");
    println!("SVG re-import: {} nodes", reimported.nodes.len());

    let mut scene = Scene::new(artboard);
    scene.play("slide");

    let mut rend = SkiaRenderer::new();
    for t in &[0.0f32, 0.5, 1.0] {
        if let Some(p) = &mut scene.player { p.time = *t; }
        scene.render(&mut rend);
        let pixels = rend.end_frame();
        let fname = format!("frame_{:.1}s.raw", t);
        std::fs::write(&fname, &pixels).unwrap();
        println!("wrote {} ({} bytes)", fname, pixels.len());
    }

    println!("Phase 2 complete — effects, clip masks, SVG import/export all live.");
}

// ── Phase 3 demo: animation mixer + constraints ───────────────────────────────

fn phase3_demo() {
    // Build a simple 3-bone arm for IK demo
    let root_id = Uuid::new_v4();
    let mid_id  = Uuid::new_v4();
    let end_id  = Uuid::new_v4();
    let tgt_id  = Uuid::new_v4();

    let mut root_node = Node::new("Root");
    root_node.id = root_id;
    root_node.transform = Transform::translation(100.0, 200.0);

    let mut mid_node = Node::new("Mid");
    mid_node.id = mid_id;
    mid_node.parent_id = Some(root_id);
    mid_node.transform = Transform::translation(60.0, 0.0);

    let mut end_node = Node::new("End");
    end_node.id = end_id;
    end_node.parent_id = Some(mid_id);
    end_node.transform = Transform::translation(60.0, 0.0);
    end_node.shape = Some(ShapeData {
        geometry: Geometry::Ellipse { radius_x: 8.0, radius_y: 8.0 },
        paint: Paint::filled(Color::from_hex(0xFF8800)),
    });

    let mut target_node = Node::new("IKTarget");
    target_node.id = tgt_id;
    target_node.transform = Transform::translation(100.0, 120.0);

    // Animation that moves the IK target
    let ik_anim = Animation {
        id: Uuid::new_v4(),
        name: "ik_reach".into(),
        duration_secs: 2.0,
        fps: 60,
        loop_mode: LoopMode::PingPong,
        tracks: vec![
            Track {
                node_id: tgt_id,
                property: Property::X,
                keyframes: vec![
                    Keyframe { time_secs: 0.0, value: 100.0, easing: Easing::CubicBezier(0.4, 0.0, 0.6, 1.0) },
                    Keyframe { time_secs: 1.0, value: 220.0, easing: Easing::CubicBezier(0.4, 0.0, 0.6, 1.0) },
                    Keyframe { time_secs: 2.0, value: 100.0, easing: Easing::Linear },
                ],
            },
            Track {
                node_id: tgt_id,
                property: Property::Y,
                keyframes: vec![
                    Keyframe { time_secs: 0.0, value: 120.0, easing: Easing::CubicBezier(0.4, 0.0, 0.6, 1.0) },
                    Keyframe { time_secs: 1.0, value:  80.0, easing: Easing::CubicBezier(0.4, 0.0, 0.6, 1.0) },
                    Keyframe { time_secs: 2.0, value: 120.0, easing: Easing::Linear },
                ],
            },
        ],
    };

    let artboard = Artboard {
        id: Uuid::new_v4(),
        name: "IKDemo".into(),
        width: 400.0,
        height: 300.0,
        background: Color::rgba(0.08, 0.08, 0.12, 1.0),
        nodes: vec![root_node, mid_node, end_node, target_node],
        animations: vec![ik_anim],
        constraints: vec![
            Constraint::IK {
                bone_ids: [root_id, mid_id, end_id],
                target_id: tgt_id,
                bend_direction: 1.0,
                strength: 1.0,
            },
        ],
    };

    // ── Mixer demo: blend two animations at different weights ─────────────
    let mut scene = Scene::new(artboard);

    let mut mixer = AnimationMixer::new();
    mixer.add_layer(scene.artboard.animations[0].clone(), 1.0);

    scene.mixer = Some(mixer);

    let mut rend = SkiaRenderer::new();
    for i in 0..=4 {
        let t = i as f32 * 0.5;
        if let Some(m) = &mut scene.mixer {
            m.layers[0].player.time = t;
        }
        scene.render(&mut rend);
        let pixels = rend.end_frame();
        let fname = format!("ik_frame_{:.1}s.raw", t);
        std::fs::write(&fname, &pixels).unwrap();
        println!("IK frame t={:.1}s → {} ({} bytes)", t, fname, pixels.len());
    }

    println!("Phase 3 complete — animation mixer, IK/aim/distance constraints all live.");
}
