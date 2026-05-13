use std::collections::HashMap;
use std::f32::consts::PI;

use nalgebra::Matrix3;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::schema::Node;

/// Constraints are stored on `Artboard` and reference nodes by ID.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum Constraint {
    /// Two-bone IK. `bone_ids` = [root, mid, end] in the chain.
    IK {
        bone_ids: [Uuid; 3],
        target_id: Uuid,
        /// +1.0 = bend left/up, -1.0 = bend right/down.
        bend_direction: f32,
        /// Blend factor: 0.0 = no effect, 1.0 = full IK.
        strength: f32,
    },
    /// Rotate `node_id` to always face `target_id`.
    Aim {
        node_id: Uuid,
        target_id: Uuid,
        /// Radians added to the computed angle (axis offset).
        offset_angle: f32,
        strength: f32,
    },
    /// Constrain `node_id` to stay within [min, max] distance of `target_id`.
    Distance {
        node_id: Uuid,
        target_id: Uuid,
        min_distance: f32,
        max_distance: f32,
        strength: f32,
    },
}

/// Solve all constraints on a mutable working copy of the node list.
/// `world` is updated in-place to stay consistent.
pub fn solve_constraints(
    constraints: &[Constraint],
    nodes: &mut Vec<Node>,
    world: &mut HashMap<Uuid, Matrix3<f32>>,
) {
    for c in constraints {
        match c {
            Constraint::IK { bone_ids, target_id, bend_direction, strength } => {
                solve_ik(nodes, world, bone_ids, *target_id, *bend_direction, *strength);
            }
            Constraint::Aim { node_id, target_id, offset_angle, strength } => {
                solve_aim(nodes, world, *node_id, *target_id, *offset_angle, *strength);
            }
            Constraint::Distance { node_id, target_id, min_distance, max_distance, strength } => {
                solve_distance(nodes, world, *node_id, *target_id, *min_distance, *max_distance, *strength);
            }
        }
    }
}

// ── IK (analytical two-bone) ──────────────────────────────────────────────────

fn solve_ik(
    nodes: &mut Vec<Node>,
    world: &mut HashMap<Uuid, Matrix3<f32>>,
    bone_ids: &[Uuid; 3],
    target_id: Uuid,
    bend_dir: f32,
    strength: f32,
) {
    let root_pos   = world_pos(world, bone_ids[0]);
    let mid_pos    = world_pos(world, bone_ids[1]);
    let end_pos    = world_pos(world, bone_ids[2]);
    let target_pos = world_pos(world, target_id);

    let len1 = dist2(root_pos, mid_pos);
    let len2 = dist2(mid_pos, end_pos);

    if len1 < 1e-6 || len2 < 1e-6 {
        return;
    }

    let max_reach = len1 + len2;
    let d = dist2(root_pos, target_pos).clamp(1e-6, max_reach * 0.9999);

    let cos_a = ((d * d + len1 * len1 - len2 * len2) / (2.0 * d * len1)).clamp(-1.0, 1.0);
    let cos_b = ((len1 * len1 + len2 * len2 - d * d) / (2.0 * len1 * len2)).clamp(-1.0, 1.0);

    let alpha = cos_a.acos();
    let beta  = cos_b.acos();

    let angle_to_target = (target_pos[1] - root_pos[1]).atan2(target_pos[0] - root_pos[0]);

    let root_world_rot = angle_to_target - alpha * bend_dir;
    let mid_world_rot  = root_world_rot  + (PI - beta) * bend_dir;

    // ── Root ──────────────────────────────────────────────────────────────
    {
        let parent_rot = find_parent_rot(nodes, world, bone_ids[0]);
        if let Some(n) = nodes.iter_mut().find(|n| n.id == bone_ids[0]) {
            let target_local = root_world_rot - parent_rot;
            n.transform.rotation = lerp(n.transform.rotation, target_local, strength);
        }
    }
    recompute_world(nodes, world, bone_ids[0]);

    // ── Mid ───────────────────────────────────────────────────────────────
    {
        let parent_rot = find_parent_rot(nodes, world, bone_ids[1]);
        if let Some(n) = nodes.iter_mut().find(|n| n.id == bone_ids[1]) {
            let target_local = mid_world_rot - parent_rot;
            n.transform.rotation = lerp(n.transform.rotation, target_local, strength);
        }
    }
    recompute_world(nodes, world, bone_ids[1]);
    recompute_world(nodes, world, bone_ids[2]);
}

// ── Aim ───────────────────────────────────────────────────────────────────────

fn solve_aim(
    nodes: &mut Vec<Node>,
    world: &mut HashMap<Uuid, Matrix3<f32>>,
    node_id: Uuid,
    target_id: Uuid,
    offset_angle: f32,
    strength: f32,
) {
    let self_pos   = world_pos(world, node_id);
    let target_pos = world_pos(world, target_id);

    let angle = (target_pos[1] - self_pos[1]).atan2(target_pos[0] - self_pos[0]) + offset_angle;
    let parent_rot = find_parent_rot(nodes, world, node_id);

    if let Some(n) = nodes.iter_mut().find(|n| n.id == node_id) {
        let target_local = angle - parent_rot;
        n.transform.rotation = lerp(n.transform.rotation, target_local, strength);
    }
    recompute_world(nodes, world, node_id);
}

// ── Distance ──────────────────────────────────────────────────────────────────

fn solve_distance(
    nodes: &mut Vec<Node>,
    world: &mut HashMap<Uuid, Matrix3<f32>>,
    node_id: Uuid,
    target_id: Uuid,
    min_dist: f32,
    max_dist: f32,
    strength: f32,
) {
    let self_pos   = world_pos(world, node_id);
    let target_pos = world_pos(world, target_id);
    let d = dist2(self_pos, target_pos);

    let desired_world = if d < min_dist && d > 1e-6 {
        let ux = (self_pos[0] - target_pos[0]) / d;
        let uy = (self_pos[1] - target_pos[1]) / d;
        [target_pos[0] + ux * min_dist, target_pos[1] + uy * min_dist]
    } else if d > max_dist && d > 1e-6 {
        let ux = (self_pos[0] - target_pos[0]) / d;
        let uy = (self_pos[1] - target_pos[1]) / d;
        [target_pos[0] + ux * max_dist, target_pos[1] + uy * max_dist]
    } else {
        return;
    };

    let blended = [
        lerp(self_pos[0], desired_world[0], strength),
        lerp(self_pos[1], desired_world[1], strength),
    ];

    // Convert world position back to parent-local space
    let parent_id = nodes.iter().find(|n| n.id == node_id).and_then(|n| n.parent_id);
    let local_pos = match parent_id.and_then(|pid| world.get(&pid)) {
        Some(parent_mat) => {
            let inv = parent_mat.try_inverse().unwrap_or(Matrix3::identity());
            let v = inv * nalgebra::Vector3::new(blended[0], blended[1], 1.0);
            [v.x, v.y]
        }
        None => blended,
    };

    if let Some(n) = nodes.iter_mut().find(|n| n.id == node_id) {
        n.transform.x = local_pos[0];
        n.transform.y = local_pos[1];
    }
    recompute_world(nodes, world, node_id);
}

// ── Shared helpers ────────────────────────────────────────────────────────────

fn world_pos(world: &HashMap<Uuid, Matrix3<f32>>, id: Uuid) -> [f32; 2] {
    world.get(&id).map(|m| [m[(0, 2)], m[(1, 2)]]).unwrap_or([0.0, 0.0])
}

fn world_rotation(m: &Matrix3<f32>) -> f32 {
    m[(1, 0)].atan2(m[(0, 0)])
}

fn find_parent_rot(nodes: &[Node], world: &HashMap<Uuid, Matrix3<f32>>, id: Uuid) -> f32 {
    nodes.iter()
        .find(|n| n.id == id)
        .and_then(|n| n.parent_id)
        .and_then(|pid| world.get(&pid))
        .map(world_rotation)
        .unwrap_or(0.0)
}

fn recompute_world(nodes: &[Node], world: &mut HashMap<Uuid, Matrix3<f32>>, id: Uuid) {
    if let Some(n) = nodes.iter().find(|n| n.id == id) {
        let local = n.transform.to_matrix();
        let parent = n.parent_id
            .and_then(|pid| world.get(&pid))
            .copied()
            .unwrap_or(Matrix3::identity());
        world.insert(id, parent * local);
    }
}

fn dist2(a: [f32; 2], b: [f32; 2]) -> f32 {
    ((b[0] - a[0]).powi(2) + (b[1] - a[1]).powi(2)).sqrt()
}

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::Node;
    use crate::transform::Transform;

    fn node_at(name: &str, x: f32, y: f32) -> Node {
        let mut n = Node::new(name);
        n.transform = Transform::translation(x, y);
        n
    }

    fn world_for(nodes: &[Node]) -> HashMap<Uuid, Matrix3<f32>> {
        let mut w = HashMap::new();
        for n in nodes {
            let local = n.transform.to_matrix();
            let parent = n.parent_id.and_then(|p| w.get(&p)).copied().unwrap_or(Matrix3::identity());
            w.insert(n.id, parent * local);
        }
        w
    }

    #[test]
    fn aim_constraint_faces_target() {
        let aim_node = node_at("aim", 0.0, 0.0);
        let target       = node_at("target", 100.0, 0.0);
        let aim_id    = aim_node.id;
        let target_id = target.id;

        let mut nodes = vec![aim_node, target];
        let mut world = world_for(&nodes);

        let constraints = [Constraint::Aim {
            node_id: aim_id,
            target_id,
            offset_angle: 0.0,
            strength: 1.0,
        }];

        solve_constraints(&constraints, &mut nodes, &mut world);

        // After aim constraint, node should point right (rotation ≈ 0)
        let rot = nodes.iter().find(|n| n.id == aim_id).unwrap().transform.rotation;
        assert!(rot.abs() < 0.01, "expected ~0 rad, got {rot}");
    }

    #[test]
    fn distance_constraint_enforces_max() {
        let node   = node_at("node",   200.0, 0.0);
        let target      = node_at("target",  0.0,  0.0);
        let node_id   = node.id;
        let target_id = target.id;

        let mut nodes = vec![node, target];
        let mut world = world_for(&nodes);

        let constraints = [Constraint::Distance {
            node_id,
            target_id,
            min_distance: 0.0,
            max_distance: 50.0,
            strength: 1.0,
        }];

        solve_constraints(&constraints, &mut nodes, &mut world);

        let pos = world_pos(&world, node_id);
        let d = (pos[0] * pos[0] + pos[1] * pos[1]).sqrt();
        assert!((d - 50.0).abs() < 1.0, "expected dist ≈ 50, got {d}");
    }

    #[test]
    fn ik_end_reaches_target() {
        // Three collinear nodes: root(0,0) → mid(50,0) → end(100,0)
        // Target at (0, 100) — should cause the chain to bend upward
        let root = node_at("root", 0.0, 0.0);
        let mut mid  = node_at("mid",  50.0, 0.0);
        let mut end  = node_at("end",  100.0, 0.0);
        mid.parent_id = Some(root.id);
        end.parent_id = Some(mid.id);
        let target = node_at("target", 0.0, 100.0);

        let root_id   = root.id;
        let mid_id    = mid.id;
        let end_id    = end.id;
        let target_id = target.id;

        let mut nodes = vec![root, mid, end, target];
        let mut world = world_for(&nodes);

        let constraints = [Constraint::IK {
            bone_ids: [root_id, mid_id, end_id],
            target_id,
            bend_direction: 1.0,
            strength: 1.0,
        }];

        solve_constraints(&constraints, &mut nodes, &mut world);

        // End effector should be close to (0, 100)
        let ep = world_pos(&world, end_id);
        let dx = ep[0] - 0.0;
        let dy = ep[1] - 100.0;
        let err = (dx * dx + dy * dy).sqrt();
        assert!(err < 5.0, "end effector error={err}, pos={ep:?}");
    }
}
