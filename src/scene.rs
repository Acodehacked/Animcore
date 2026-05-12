use std::collections::HashMap;
use uuid::Uuid;

use crate::paint::Paint;
use crate::playback::{AnimationPlayer, NodePose};
use crate::renderer::Renderer;
use crate::schema::Artboard;
use nalgebra::Matrix3;

/// High-level handle for driving a single artboard with one active animation.
pub struct Scene {
    pub artboard: Artboard,
    pub player: Option<AnimationPlayer>,
}

impl Scene {
    pub fn new(artboard: Artboard) -> Self {
        Self { artboard, player: None }
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
        if let Some(p) = &mut self.player {
            p.advance(delta_secs);
        }
    }

    /// Render the current frame into `renderer`.
    pub fn render(&self, renderer: &mut dyn Renderer) {
        let bg = self.artboard.background.to_u8();
        renderer.begin_frame(
            self.artboard.width as u32,
            self.artboard.height as u32,
            bg,
        );

        let poses: HashMap<Uuid, NodePose> = self
            .player
            .as_ref()
            .map(|p| p.evaluate())
            .unwrap_or_default();

        let world = compute_world_transforms_live(&self.artboard.nodes, &poses);

        // Build a map of which nodes clip their children (node_id → clip world matrix)
        let clip_nodes: HashMap<Uuid, Matrix3<f32>> = self
            .artboard
            .nodes
            .iter()
            .filter(|n| n.clip_children && n.shape.is_some())
            .map(|n| (n.id, world.get(&n.id).copied().unwrap_or(Matrix3::identity())))
            .collect();

        for node in &self.artboard.nodes {
            if !node.visible {
                continue;
            }

            let world_mat = world.get(&node.id).copied().unwrap_or(Matrix3::identity());
            let opacity = poses
                .get(&node.id)
                .and_then(|p| p.opacity)
                .unwrap_or(node.opacity);

            // Apply parent clip if the direct parent clips its children
            let parent_clips = node
                .parent_id
                .and_then(|pid| {
                    self.artboard.nodes.iter().find(|n| n.id == pid)
                })
                .map(|p| p.clip_children)
                .unwrap_or(false);

            if parent_clips {
                if let Some(parent_id) = node.parent_id {
                    if let Some(parent_node) = self.artboard.nodes.iter().find(|n| n.id == parent_id) {
                        if let Some(shape) = &parent_node.shape {
                            let clip_mat = clip_nodes
                                .get(&parent_id)
                                .copied()
                                .unwrap_or(Matrix3::identity());
                            renderer.push_clip(&shape.geometry.to_path(), &clip_mat);
                        }
                    }
                }
            }

            if let Some(shape) = &node.shape {
                let path = shape.geometry.to_path();
                let paint = apply_paint_pose(&shape.paint, poses.get(&node.id));
                renderer.draw_path(&path, &paint, &world_mat, opacity, &node.effects);
            }

            if parent_clips {
                renderer.pop_clip();
            }
        }
    }
}

fn compute_world_transforms_live(
    nodes: &[crate::schema::Node],
    poses: &HashMap<Uuid, NodePose>,
) -> HashMap<Uuid, Matrix3<f32>> {
    let mut world: HashMap<Uuid, Matrix3<f32>> = HashMap::new();
    let identity = Matrix3::identity();

    for node in nodes {
        let live_transform = if let Some(pose) = poses.get(&node.id) {
            pose.apply_to_transform(&node.transform)
        } else {
            node.transform.clone()
        };
        let local = live_transform.to_matrix();
        let parent_mat = node
            .parent_id
            .and_then(|pid| world.get(&pid))
            .copied()
            .unwrap_or(identity);
        world.insert(node.id, parent_mat * local);
    }
    world
}

fn apply_paint_pose(base: &Paint, pose: Option<&NodePose>) -> Paint {
    let pose = match pose {
        Some(p) => p,
        None => return base.clone(),
    };

    let mut paint = base.clone();

    if let crate::paint::Fill::Solid(ref mut color) = paint.fill {
        if let Some(r) = pose.fill_color[0] { color.r = r; }
        if let Some(g) = pose.fill_color[1] { color.g = g; }
        if let Some(b) = pose.fill_color[2] { color.b = b; }
        if let Some(a) = pose.fill_color[3] { color.a = a; }
    }

    if let Some(sw) = pose.stroke_width {
        if let Some(stroke) = &mut paint.stroke {
            stroke.width = sw;
        }
    }

    paint
}
