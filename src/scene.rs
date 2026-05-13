use std::collections::HashMap;
use uuid::Uuid;

use nalgebra::Matrix3;

use crate::constraints;
use crate::mixer::AnimationMixer;
use crate::paint::Paint;
use crate::playback::{AnimationPlayer, NodePose};
use crate::renderer::Renderer;
use crate::schema::{Artboard, Geometry, Node};

/// High-level handle for driving a single artboard.
/// Supports a single player, a multi-layer mixer, or neither (static).
pub struct Scene {
    pub artboard: Artboard,
    /// Simple single-animation player (used when `mixer` is None).
    pub player: Option<AnimationPlayer>,
    /// Multi-layer blended player (takes priority over `player` when set).
    pub mixer: Option<AnimationMixer>,
}

impl Scene {
    pub fn new(artboard: Artboard) -> Self {
        Self { artboard, player: None, mixer: None }
    }

    pub fn play(&mut self, animation_name: &str) {
        if let Some(anim) = self
            .artboard
            .animations
            .iter()
            .find(|a| a.name == animation_name)
            .cloned()
        {
            self.player = Some(AnimationPlayer::new(anim));
        }
    }

    pub fn advance(&mut self, delta_secs: f32) {
        if let Some(m) = &mut self.mixer {
            m.advance(delta_secs);
        } else if let Some(p) = &mut self.player {
            p.advance(delta_secs);
        }
    }

    /// Render the current frame to a PNG byte vector (CPU path via SkiaRenderer).
    #[cfg(feature = "skia-renderer")]
    pub fn render_to_png(&self) -> Vec<u8> {
        use crate::renderer::skia::SkiaRenderer;
        let mut r = SkiaRenderer::new();
        self.render(&mut r);
        r.encode_png()
    }

    /// Render the current frame.
    pub fn render(&self, renderer: &mut dyn Renderer) {
        let bg = self.artboard.background.to_u8();
        renderer.begin_frame(
            self.artboard.width as u32,
            self.artboard.height as u32,
            bg,
        );

        // 1. Evaluate poses
        let poses: HashMap<Uuid, NodePose> = if let Some(m) = &self.mixer {
            m.evaluate()
        } else if let Some(p) = &self.player {
            p.evaluate()
        } else {
            HashMap::new()
        };

        // 2. Build a working copy of nodes with poses applied to transforms
        let mut working = apply_poses(&self.artboard.nodes, &poses);

        // 3. Compute world matrices
        let mut world = compute_world_transforms(&working);

        // 4. Solve constraints
        if !self.artboard.constraints.is_empty() {
            constraints::solve_constraints(&self.artboard.constraints, &mut working, &mut world);
        }

        // 5. Render
        // Pre-build clip node map: id → (clip_path, world_matrix)
        let clip_map: HashMap<Uuid, AnimClip> = working
            .iter()
            .filter(|n| n.clip_children)
            .filter_map(|n| {
                let shape = n.shape.as_ref()?;
                Some((n.id, AnimClip {
                    path: shape.geometry.to_path(),
                    mat: world.get(&n.id).copied().unwrap_or(Matrix3::identity()),
                }))
            })
            .collect();

        for node in &working {
            if !node.visible {
                continue;
            }

            let world_mat = world.get(&node.id).copied().unwrap_or(Matrix3::identity());
            let opacity = node.opacity;

            // Push parent clip if applicable
            let need_clip = node
                .parent_id
                .and_then(|pid| clip_map.get(&pid))
                .is_some();

            if need_clip {
                let clip = node.parent_id.and_then(|pid| clip_map.get(&pid)).unwrap();
                renderer.push_clip(&clip.path, &clip.mat);
            }

            // Draw shape or nested artboard
            if let Some(shape) = &node.shape {
                match &shape.geometry {
                    Geometry::NestedArtboard(nested_ab) => {
                        render_nested(renderer, nested_ab, &world_mat, opacity);
                    }
                    _ => {
                        let path = shape.geometry.to_path();
                        let paint = apply_paint_pose(&shape.paint, poses.get(&node.id));
                        renderer.draw_path(&path, &paint, &world_mat, opacity, &node.effects);
                    }
                }
            }

            if need_clip {
                renderer.pop_clip();
            }
        }
    }
}

struct AnimClip {
    path: crate::path::AnimPath,
    mat: Matrix3<f32>,
}

// ── Nested artboard rendering ─────────────────────────────────────────────────

fn render_nested(
    renderer: &mut dyn Renderer,
    artboard: &Artboard,
    transform: &Matrix3<f32>,
    opacity: f32,
) {
    #[cfg(feature = "skia-renderer")]
    {
        use crate::renderer::skia::SkiaRenderer;
        let nested = Scene::new(artboard.clone());
        let mut r = SkiaRenderer::new();
        nested.render(&mut r);
        let pixels = r.end_frame();
        renderer.draw_pixels(&pixels, artboard.width as u32, artboard.height as u32, transform, opacity);
    }
    #[cfg(not(feature = "skia-renderer"))]
    let _ = (renderer, artboard, transform, opacity);
}

// ── Pose application ──────────────────────────────────────────────────────────

fn apply_poses(nodes: &[Node], poses: &HashMap<Uuid, NodePose>) -> Vec<Node> {
    nodes.iter().map(|n| {
        let mut n = n.clone();
        if let Some(pose) = poses.get(&n.id) {
            n.transform = pose.apply_to_transform(&n.transform);
            if let Some(v) = pose.opacity { n.opacity = v; }
            if let Some(shape) = &mut n.shape {
                shape.paint = apply_paint_pose(&shape.paint, Some(pose));
            }
        }
        n
    }).collect()
}

fn apply_paint_pose(base: &Paint, pose: Option<&NodePose>) -> Paint {
    let pose = match pose {
        Some(p) => p,
        None => return base.clone(),
    };
    let mut paint = base.clone();
    if let crate::paint::Fill::Solid(ref mut c) = paint.fill {
        if let Some(r) = pose.fill_color[0] { c.r = r; }
        if let Some(g) = pose.fill_color[1] { c.g = g; }
        if let Some(b) = pose.fill_color[2] { c.b = b; }
        if let Some(a) = pose.fill_color[3] { c.a = a; }
    }
    if let Some(sw) = pose.stroke_width {
        if let Some(s) = &mut paint.stroke { s.width = sw; }
    }
    paint
}

// ── World transform computation ────────────────────────────────────────────────

pub fn compute_world_transforms(nodes: &[Node]) -> HashMap<Uuid, Matrix3<f32>> {
    let mut world = HashMap::new();
    let identity = Matrix3::identity();
    for n in nodes {
        let local  = n.transform.to_matrix();
        let parent = n.parent_id.and_then(|p| world.get(&p)).copied().unwrap_or(identity);
        world.insert(n.id, parent * local);
    }
    world
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(all(test, feature = "skia-renderer"))]
mod tests {
    use super::*;
    use crate::paint::Color;
    use crate::schema::Artboard;

    fn blank_scene() -> Scene {
        Scene::new(Artboard {
            id: uuid::Uuid::new_v4(),
            name: "T".into(),
            width: 32.0, height: 32.0,
            background: Color::WHITE,
            nodes: vec![], animations: vec![], constraints: vec![],
        })
    }

    #[test]
    fn render_to_png_produces_valid_header() {
        let png = blank_scene().render_to_png();
        assert!(png.len() > 8, "PNG too short ({} bytes)", png.len());
        assert_eq!(&png[0..4], b"\x89PNG", "missing PNG magic bytes");
    }

    #[test]
    fn render_to_png_correct_dimensions() {
        let mut s = blank_scene();
        let png = s.render_to_png();
        // PNG IHDR is at offset 16..20 (width) and 20..24 (height)
        let w = u32::from_be_bytes([png[16], png[17], png[18], png[19]]);
        let h = u32::from_be_bytes([png[20], png[21], png[22], png[23]]);
        assert_eq!(w, 32, "PNG width mismatch");
        assert_eq!(h, 32, "PNG height mismatch");
    }
}
