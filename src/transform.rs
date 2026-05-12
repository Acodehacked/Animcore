use nalgebra::{Matrix3, Vector2, Vector3};
use serde::{Deserialize, Serialize};

/// Affine 2D transform stored as decomposed components for easy animation.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct Transform {
    pub x: f32,
    pub y: f32,
    pub rotation: f32,   // radians
    pub scale_x: f32,
    pub scale_y: f32,
    pub skew_x: f32,     // radians
    pub skew_y: f32,
}

impl Default for Transform {
    fn default() -> Self {
        Self {
            x: 0.0, y: 0.0,
            rotation: 0.0,
            scale_x: 1.0, scale_y: 1.0,
            skew_x: 0.0, skew_y: 0.0,
        }
    }
}

impl Transform {
    pub fn translation(x: f32, y: f32) -> Self {
        Self { x, y, ..Default::default() }
    }

    /// Convert to a 3×3 column-major affine matrix.
    pub fn to_matrix(&self) -> Matrix3<f32> {
        let cos_r = self.rotation.cos();
        let sin_r = self.rotation.sin();

        // T * R * Skew * S
        let sx = self.scale_x;
        let sy = self.scale_y;
        let tx = self.skew_x.tan();
        let ty = self.skew_y.tan();

        // combined in one step (column-major):
        //  [sx*(cos_r - ty*sin_r)   sy*(tx*cos_r - sin_r)   x]
        //  [sx*(sin_r + ty*cos_r)   sy*(tx*sin_r + cos_r)   y]
        //  [0                       0                        1]
        Matrix3::new(
            sx * (cos_r - ty * sin_r),  sy * (tx * cos_r - sin_r), self.x,
            sx * (sin_r + ty * cos_r),  sy * (tx * sin_r + cos_r), self.y,
            0.0,                         0.0,                        1.0,
        )
    }

    /// Multiply this transform with a parent matrix to produce a world matrix.
    pub fn world_matrix(&self, parent: &Matrix3<f32>) -> Matrix3<f32> {
        parent * self.to_matrix()
    }

    /// Apply the world matrix to a 2D point.
    pub fn apply(matrix: &Matrix3<f32>, point: [f32; 2]) -> [f32; 2] {
        let v = matrix * Vector3::new(point[0], point[1], 1.0);
        [v.x, v.y]
    }

    /// Decompose a matrix back into a Transform (approximate, no skew recovery).
    pub fn from_matrix(m: &Matrix3<f32>) -> Self {
        let sx = Vector2::new(m[(0, 0)], m[(1, 0)]).norm();
        let sy = Vector2::new(m[(0, 1)], m[(1, 1)]).norm();
        let rotation = m[(1, 0)].atan2(m[(0, 0)]);
        Self {
            x: m[(0, 2)],
            y: m[(1, 2)],
            rotation,
            scale_x: sx,
            scale_y: sy,
            skew_x: 0.0,
            skew_y: 0.0,
        }
    }
}

/// Walk a flat node list and compute world matrices for every node.
/// Nodes must be ordered so parents appear before children.
use std::collections::HashMap;
use uuid::Uuid;

pub fn compute_world_transforms(
    nodes: &[crate::schema::Node],
) -> HashMap<Uuid, Matrix3<f32>> {
    let mut world: HashMap<Uuid, Matrix3<f32>> = HashMap::new();
    let identity = Matrix3::identity();

    for node in nodes {
        let local = node.transform.to_matrix();
        let parent_mat = node
            .parent_id
            .and_then(|pid| world.get(&pid))
            .unwrap_or(&identity);
        world.insert(node.id, parent_mat * local);
    }
    world
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f32::consts::PI;

    #[test]
    fn identity_roundtrip() {
        let t = Transform::default();
        let m = t.to_matrix();
        assert!((m[(0, 0)] - 1.0).abs() < 1e-5);
        assert!((m[(1, 1)] - 1.0).abs() < 1e-5);
        assert!((m[(0, 2)]).abs() < 1e-5);
    }

    #[test]
    fn translation_apply() {
        let t = Transform::translation(100.0, 50.0);
        let m = t.to_matrix();
        let p = Transform::apply(&m, [0.0, 0.0]);
        assert!((p[0] - 100.0).abs() < 1e-4);
        assert!((p[1] - 50.0).abs() < 1e-4);
    }

    #[test]
    fn rotation_90_degrees() {
        let t = Transform { rotation: PI / 2.0, ..Default::default() };
        let m = t.to_matrix();
        let p = Transform::apply(&m, [1.0, 0.0]);
        assert!((p[0] - 0.0).abs() < 1e-5);
        assert!((p[1] - 1.0).abs() < 1e-5);
    }

    #[test]
    fn parent_child_chain() {
        let parent = Transform::translation(100.0, 0.0).to_matrix();
        let child  = Transform::translation(50.0, 0.0);
        let world  = child.world_matrix(&parent);
        let p = Transform::apply(&world, [0.0, 0.0]);
        assert!((p[0] - 150.0).abs() < 1e-4);
    }
}
