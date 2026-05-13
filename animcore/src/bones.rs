/// Skeleton rigging — bone hierarchy and mesh vertex weighting.

use std::collections::HashMap;
use nalgebra::Matrix3;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

// ── Data model ────────────────────────────────────────────────────────────────

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Bone {
    pub id: Uuid,
    pub name: String,
    pub parent_id: Option<Uuid>,
    /// Rest-pose transform (local, relative to parent).
    pub rest_x: f32,
    pub rest_y: f32,
    pub rest_rotation: f32,
    pub rest_length: f32,
}

/// A single vertex in a mesh, with up to 4 bone influences.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Vertex {
    /// Rest-pose position.
    pub rest: [f32; 2],
    /// Up to 4 (bone_id, weight) pairs; weights must sum to 1.0.
    pub influences: Vec<(Uuid, f32)>,
}

/// A mesh attached to a skeleton.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Mesh {
    pub vertices: Vec<Vertex>,
    /// Triangles as vertex indices.
    pub indices: Vec<[u32; 3]>,
}

// ── Skeleton ──────────────────────────────────────────────────────────────────

/// Computes rest-pose world matrices for all bones (ordered parent-before-child).
pub fn compute_rest_matrices(bones: &[Bone]) -> HashMap<Uuid, Matrix3<f32>> {
    let mut world: HashMap<Uuid, Matrix3<f32>> = HashMap::new();
    for bone in bones {
        let local = bone_local_matrix(bone.rest_x, bone.rest_y, bone.rest_rotation);
        let parent = bone.parent_id
            .and_then(|pid| world.get(&pid))
            .copied()
            .unwrap_or(Matrix3::identity());
        world.insert(bone.id, parent * local);
    }
    world
}

/// Deform mesh vertices using current bone world matrices and rest-pose inverses.
///
/// `current` – live bone world matrices (from animation).
/// `rest_inv` – inverse of rest-pose world matrices (precomputed once).
pub fn deform_mesh(
    mesh: &Mesh,
    current: &HashMap<Uuid, Matrix3<f32>>,
    rest_inv: &HashMap<Uuid, Matrix3<f32>>,
) -> Vec<[f32; 2]> {
    mesh.vertices.iter().map(|v| {
        let mut sumx = 0.0f32;
        let mut sumy = 0.0f32;
        for &(bone_id, weight) in &v.influences {
            if weight < 1e-6 { continue; }
            let cur = current.get(&bone_id).copied().unwrap_or(Matrix3::identity());
            let inv = rest_inv.get(&bone_id).copied().unwrap_or(Matrix3::identity());
            let skinning = cur * inv;
            let p = nalgebra::Vector3::new(v.rest[0], v.rest[1], 1.0);
            let transformed = skinning * p;
            sumx += transformed.x * weight;
            sumy += transformed.y * weight;
        }
        [sumx, sumy]
    }).collect()
}

fn bone_local_matrix(x: f32, y: f32, rot: f32) -> Matrix3<f32> {
    let (s, c) = rot.sin_cos();
    Matrix3::new(
        c, -s, x,
        s,  c, y,
        0.0, 0.0, 1.0,
    )
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn make_bone(name: &str, x: f32, y: f32, parent: Option<Uuid>) -> Bone {
        Bone {
            id: Uuid::new_v4(),
            name: name.into(),
            parent_id: parent,
            rest_x: x,
            rest_y: y,
            rest_rotation: 0.0,
            rest_length: 50.0,
        }
    }

    #[test]
    fn rest_matrices_parent_child() {
        let root = make_bone("root", 0.0, 0.0, None);
        let child = Bone { parent_id: Some(root.id), ..make_bone("child", 50.0, 0.0, None) };
        let bones = vec![root.clone(), child.clone()];
        let world = compute_rest_matrices(&bones);
        let child_pos = [world[&child.id][(0, 2)], world[&child.id][(1, 2)]];
        assert!((child_pos[0] - 50.0).abs() < 1e-4, "child x should be 50, got {}", child_pos[0]);
    }

    #[test]
    fn deform_identity_returns_rest() {
        let bone = make_bone("b", 0.0, 0.0, None);
        let rest = compute_rest_matrices(&[bone.clone()]);
        let rest_inv: HashMap<Uuid, Matrix3<f32>> = rest.iter()
            .map(|(&id, m)| (id, m.try_inverse().unwrap_or(Matrix3::identity())))
            .collect();

        let mesh = Mesh {
            vertices: vec![
                Vertex { rest: [10.0, 20.0], influences: vec![(bone.id, 1.0)] },
            ],
            indices: vec![],
        };
        let deformed = deform_mesh(&mesh, &rest, &rest_inv);
        assert!((deformed[0][0] - 10.0).abs() < 1e-3);
        assert!((deformed[0][1] - 20.0).abs() < 1e-3);
    }
}
