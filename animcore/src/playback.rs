use crate::schema::*;
use std::collections::HashMap;
use uuid::Uuid;

pub struct AnimationPlayer {
    pub animation: Animation,
    pub time: f32,
}

impl AnimationPlayer {
    pub fn new(animation: Animation) -> Self {
        Self { animation, time: 0.0 }
    }

    pub fn advance(&mut self, delta_secs: f32) {
        let dur = self.animation.duration_secs;
        if dur <= 0.0 {
            return;
        }
        self.time += delta_secs;
        match self.animation.loop_mode {
            LoopMode::Loop => {
                if self.time > dur {
                    self.time %= dur;
                }
            }
            LoopMode::PingPong => {
                let period = dur * 2.0;
                self.time %= period;
                if self.time > dur {
                    self.time = period - self.time;
                }
            }
            LoopMode::Once => {
                self.time = self.time.min(dur);
            }
        }
    }

    pub fn is_finished(&self) -> bool {
        matches!(self.animation.loop_mode, LoopMode::Once)
            && self.time >= self.animation.duration_secs
    }

    /// Returns a per-node pose map with all animated property values at the current time.
    pub fn evaluate(&self) -> HashMap<Uuid, NodePose> {
        let mut poses: HashMap<Uuid, NodePose> = HashMap::new();

        for track in &self.animation.tracks {
            let value = interpolate(&track.keyframes, self.time);
            let pose = poses.entry(track.node_id).or_default();
            match track.property {
                Property::X          => pose.x = Some(value),
                Property::Y          => pose.y = Some(value),
                Property::Rotation   => pose.rotation = Some(value),
                Property::ScaleX     => pose.scale_x = Some(value),
                Property::ScaleY     => pose.scale_y = Some(value),
                Property::SkewX      => pose.skew_x = Some(value),
                Property::SkewY      => pose.skew_y = Some(value),
                Property::Opacity    => pose.opacity = Some(value),
                Property::StrokeWidth => pose.stroke_width = Some(value),
                Property::FillColorR => pose.fill_color[0] = Some(value),
                Property::FillColorG => pose.fill_color[1] = Some(value),
                Property::FillColorB => pose.fill_color[2] = Some(value),
                Property::FillColorA => pose.fill_color[3] = Some(value),
                Property::PathPointX(i) => {
                    pose.path_points.entry(i).or_insert([None; 2])[0] = Some(value);
                }
                Property::PathPointY(i) => {
                    pose.path_points.entry(i).or_insert([None; 2])[1] = Some(value);
                }
            }
        }
        poses
    }
}

/// Animated values for a single node at a point in time.
/// `None` means "use the node's base value".
#[derive(Default, Debug, Clone)]
pub struct NodePose {
    pub x: Option<f32>,
    pub y: Option<f32>,
    pub rotation: Option<f32>,
    pub scale_x: Option<f32>,
    pub scale_y: Option<f32>,
    pub skew_x: Option<f32>,
    pub skew_y: Option<f32>,
    pub opacity: Option<f32>,
    pub stroke_width: Option<f32>,
    pub fill_color: [Option<f32>; 4],
    pub path_points: HashMap<u32, [Option<f32>; 2]>,
}

impl NodePose {
    /// Apply this pose onto a node's base transform, returning the live transform.
    pub fn apply_to_transform(&self, base: &crate::transform::Transform) -> crate::transform::Transform {
        crate::transform::Transform {
            x:        self.x.unwrap_or(base.x),
            y:        self.y.unwrap_or(base.y),
            rotation: self.rotation.unwrap_or(base.rotation),
            scale_x:  self.scale_x.unwrap_or(base.scale_x),
            scale_y:  self.scale_y.unwrap_or(base.scale_y),
            skew_x:   self.skew_x.unwrap_or(base.skew_x),
            skew_y:   self.skew_y.unwrap_or(base.skew_y),
        }
    }
}

// ── Interpolation ─────────────────────────────────────────────────────────────

pub fn interpolate(keyframes: &[Keyframe], time: f32) -> f32 {
    if keyframes.is_empty() {
        return 0.0;
    }
    let first = keyframes.first().unwrap();
    let last = keyframes.last().unwrap();
    if time <= first.time_secs {
        return first.value;
    }
    if time >= last.time_secs {
        return last.value;
    }
    for window in keyframes.windows(2) {
        let (a, b) = (&window[0], &window[1]);
        if time >= a.time_secs && time <= b.time_secs {
            let t = (time - a.time_secs) / (b.time_secs - a.time_secs);
            return match &a.easing {
                Easing::Hold => a.value,
                Easing::Linear => a.value + t * (b.value - a.value),
                Easing::CubicBezier(x1, y1, x2, y2) => {
                    let t_eased = solve_bezier(t, *x1, *y1, *x2, *y2);
                    a.value + t_eased * (b.value - a.value)
                }
            };
        }
    }
    0.0
}

fn solve_bezier(t: f32, x1: f32, y1: f32, x2: f32, y2: f32) -> f32 {
    let mut guess = t;
    for _ in 0..8 {
        let x = bezier_component(guess, x1, x2) - t;
        let dx = bezier_derivative(guess, x1, x2);
        if dx.abs() < 1e-6 {
            break;
        }
        guess -= x / dx;
    }
    bezier_component(guess, y1, y2)
}

fn bezier_component(t: f32, p1: f32, p2: f32) -> f32 {
    3.0 * (1.0 - t).powi(2) * t * p1
        + 3.0 * (1.0 - t) * t.powi(2) * p2
        + t.powi(3)
}

fn bezier_derivative(t: f32, p1: f32, p2: f32) -> f32 {
    3.0 * (1.0 - t).powi(2) * p1
        + 6.0 * (1.0 - t) * t * (p2 - p1)
        + 3.0 * t.powi(2) * (1.0 - p2)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kf(t: f32, v: f32) -> Keyframe {
        Keyframe { time_secs: t, value: v, easing: Easing::Linear }
    }

    #[test]
    fn linear_midpoint() {
        let kfs = vec![kf(0.0, 0.0), kf(1.0, 100.0)];
        assert!((interpolate(&kfs, 0.5) - 50.0).abs() < 1e-4);
    }

    #[test]
    fn hold_easing() {
        let kfs = vec![
            Keyframe { time_secs: 0.0, value: 10.0, easing: Easing::Hold },
            Keyframe { time_secs: 1.0, value: 20.0, easing: Easing::Linear },
        ];
        assert!((interpolate(&kfs, 0.5) - 10.0).abs() < 1e-4);
    }

    #[test]
    fn pingpong_mode() {
        let anim = Animation {
            id: Uuid::new_v4(),
            name: "pp".into(),
            duration_secs: 1.0,
            fps: 60,
            loop_mode: LoopMode::PingPong,
            tracks: vec![],
        };
        let mut player = AnimationPlayer::new(anim);
        player.advance(1.5);
        assert!((player.time - 0.5).abs() < 1e-4);
    }
}
