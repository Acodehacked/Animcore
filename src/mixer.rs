use std::collections::HashMap;
use uuid::Uuid;

use crate::playback::{AnimationPlayer, NodePose};
use crate::schema::Animation;

pub struct AnimationLayer {
    pub player: AnimationPlayer,
    pub weight: f32,
    /// When true, values ADD onto the result instead of being blended.
    pub additive: bool,
}

impl AnimationLayer {
    pub fn new(animation: Animation, weight: f32) -> Self {
        Self { player: AnimationPlayer::new(animation), weight, additive: false }
    }

    pub fn additive(animation: Animation, weight: f32) -> Self {
        Self { player: AnimationPlayer::new(animation), weight, additive: true }
    }
}

/// Blends multiple animation layers into a single pose each tick.
pub struct AnimationMixer {
    pub layers: Vec<AnimationLayer>,
}

impl AnimationMixer {
    pub fn new() -> Self {
        Self { layers: vec![] }
    }

    /// Add a normal (blended) layer; returns the layer index.
    pub fn add_layer(&mut self, animation: Animation, weight: f32) -> usize {
        self.layers.push(AnimationLayer::new(animation, weight));
        self.layers.len() - 1
    }

    /// Add an additive layer; returns the layer index.
    pub fn add_additive_layer(&mut self, animation: Animation, weight: f32) -> usize {
        self.layers.push(AnimationLayer::additive(animation, weight));
        self.layers.len() - 1
    }

    pub fn set_weight(&mut self, index: usize, weight: f32) {
        if let Some(l) = self.layers.get_mut(index) {
            l.weight = weight.clamp(0.0, 1.0);
        }
    }

    pub fn advance(&mut self, delta: f32) {
        for layer in &mut self.layers {
            if layer.weight > 0.0 {
                layer.player.advance(delta);
            }
        }
    }

    /// Blend all layers into one pose map.
    pub fn evaluate(&self) -> HashMap<Uuid, NodePose> {
        let mut result: HashMap<Uuid, NodePose> = HashMap::new();

        let base: Vec<_> = self.layers.iter().filter(|l| !l.additive && l.weight > 0.0).collect();
        let additive: Vec<_> = self.layers.iter().filter(|l| l.additive && l.weight > 0.0).collect();

        let total: f32 = base.iter().map(|l| l.weight).sum();

        // Normalized weighted blend for base layers
        if total > 0.0 {
            for layer in &base {
                let t = layer.weight / total;
                for (id, pose) in layer.player.evaluate() {
                    blend_pose_into(result.entry(id).or_default(), &pose, t);
                }
            }
        }

        // Additive layers stack on top
        for layer in &additive {
            for (id, pose) in layer.player.evaluate() {
                add_pose_into(result.entry(id).or_default(), &pose, layer.weight);
            }
        }

        result
    }
}

impl Default for AnimationMixer {
    fn default() -> Self { Self::new() }
}

// ── Blend helpers ─────────────────────────────────────────────────────────────

fn mix(a: Option<f32>, b: Option<f32>, t: f32) -> Option<f32> {
    match (a, b) {
        (Some(va), Some(vb)) => Some(va + vb * t),
        (Some(va), None)     => Some(va),
        (None, Some(vb))     => Some(vb * t),
        (None, None)         => None,
    }
}

fn add(a: Option<f32>, b: Option<f32>, w: f32) -> Option<f32> {
    match (a, b) {
        (Some(va), Some(vb)) => Some(va + vb * w),
        _ => a,
    }
}

fn blend_pose_into(base: &mut NodePose, other: &NodePose, t: f32) {
    base.x            = mix(base.x,            other.x,            t);
    base.y            = mix(base.y,            other.y,            t);
    base.rotation     = mix(base.rotation,     other.rotation,     t);
    base.scale_x      = mix(base.scale_x,      other.scale_x,      t);
    base.scale_y      = mix(base.scale_y,      other.scale_y,      t);
    base.skew_x       = mix(base.skew_x,       other.skew_x,       t);
    base.skew_y       = mix(base.skew_y,       other.skew_y,       t);
    base.opacity      = mix(base.opacity,      other.opacity,      t);
    base.stroke_width = mix(base.stroke_width, other.stroke_width, t);
    for i in 0..4 { base.fill_color[i] = mix(base.fill_color[i], other.fill_color[i], t); }
}

fn add_pose_into(base: &mut NodePose, other: &NodePose, w: f32) {
    base.x            = add(base.x,            other.x,            w);
    base.y            = add(base.y,            other.y,            w);
    base.rotation     = add(base.rotation,     other.rotation,     w);
    base.scale_x      = add(base.scale_x,      other.scale_x,      w);
    base.scale_y      = add(base.scale_y,      other.scale_y,      w);
    base.skew_x       = add(base.skew_x,       other.skew_x,       w);
    base.skew_y       = add(base.skew_y,       other.skew_y,       w);
    base.opacity      = add(base.opacity,      other.opacity,      w);
    base.stroke_width = add(base.stroke_width, other.stroke_width, w);
    for i in 0..4 { base.fill_color[i] = add(base.fill_color[i], other.fill_color[i], w); }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::{Easing, Keyframe, LoopMode, Property, Track};

    fn make_anim(node_id: Uuid, from: f32, to: f32, dur: f32) -> Animation {
        Animation {
            id: Uuid::new_v4(),
            name: "test".into(),
            duration_secs: dur,
            fps: 60,
            loop_mode: LoopMode::Once,
            tracks: vec![Track {
                node_id,
                property: Property::X,
                keyframes: vec![
                    Keyframe { time_secs: 0.0, value: from, easing: Easing::Linear },
                    Keyframe { time_secs: dur, value: to,   easing: Easing::Linear },
                ],
            }],
        }
    }

    #[test]
    fn equal_weight_blend_averages() {
        let id = Uuid::new_v4();
        let mut mixer = AnimationMixer::new();
        let i0 = mixer.add_layer(make_anim(id, 0.0, 100.0, 1.0), 0.5);
        let i1 = mixer.add_layer(make_anim(id, 0.0, 200.0, 1.0), 0.5);
        // At t=1s, layer0 gives x=100, layer1 gives x=200
        mixer.layers[i0].player.time = 1.0;
        mixer.layers[i1].player.time = 1.0;
        let poses = mixer.evaluate();
        let x = poses[&id].x.unwrap();
        assert!((x - 150.0).abs() < 0.1, "expected ~150, got {x}");
    }

    #[test]
    fn additive_layer_stacks() {
        let id = Uuid::new_v4();
        let mut mixer = AnimationMixer::new();
        let i0 = mixer.add_layer(make_anim(id, 100.0, 100.0, 1.0), 1.0);
        let i1 = mixer.add_additive_layer(make_anim(id, 0.0, 50.0, 1.0), 1.0);
        mixer.layers[i0].player.time = 0.0;
        mixer.layers[i1].player.time = 1.0;
        let poses = mixer.evaluate();
        // Base gives 100, additive gives +50 → total 150
        let x = poses[&id].x.unwrap();
        assert!((x - 150.0).abs() < 0.1, "expected 150, got {x}");
    }

    #[test]
    fn zero_weight_layer_ignored() {
        let id = Uuid::new_v4();
        let mut mixer = AnimationMixer::new();
        let i0 = mixer.add_layer(make_anim(id, 0.0, 100.0, 1.0), 1.0);
        let i1 = mixer.add_layer(make_anim(id, 0.0, 999.0, 1.0), 0.0);
        mixer.layers[i0].player.time = 1.0;
        mixer.layers[i1].player.time = 1.0;
        let poses = mixer.evaluate();
        let x = poses[&id].x.unwrap();
        assert!((x - 100.0).abs() < 0.1, "expected 100, got {x}");
    }
}
