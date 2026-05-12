use std::collections::HashMap;
use uuid::Uuid;

use crate::paint::Paint;
use crate::playback::{AnimationPlayer, NodePose};
use crate::renderer::Renderer;
use crate::schema::{Artboard, Node};
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

        // Collect animation poses (or empty map when no player)
        let poses: HashMap<Uuid, NodePose> = self
            .player
            .as_ref()
            .map(|p| p.evaluate())
            .unwrap_or_default();

        // Compute world matrices respecting parent hierarchy
        let world = compute_world_transforms_live(&self.artboard.nodes, &poses);

        // Draw each visible node that has a shape
        for node in &self.artboard.nodes {
            if !node.visible {
                continue;
            }
            let shape = match &node.shape {
                Some(s) => s,
                None => continue,
            };

            let world_mat = world.get(&node.id).copied().unwrap_or(Matrix3::identity());
            let opacity = poses
                .get(&node.id)
                .and_then(|p| p.opacity)
                .unwrap_or(node.opacity);

            let path = shape.geometry.to_path();

            // Apply animated paint overrides
            let paint = apply_paint_pose(&shape.paint, poses.get(&node.id));

            renderer.draw_path(&path, &paint, &world_mat, opacity);
        }
    }
}

fn compute_world_transforms_live(
    nodes: &[Node],
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

    // Overlay animated RGBA on solid fill if all channels are provided via tracks
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
