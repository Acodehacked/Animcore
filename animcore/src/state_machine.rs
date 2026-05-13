use std::collections::HashMap;
use uuid::Uuid;
use serde::{Deserialize, Serialize};

use crate::playback::{AnimationPlayer, NodePose};
use crate::schema::Animation;

// ── Events ────────────────────────────────────────────────────────────────────

/// Fired when a state machine changes state.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub enum SmEvent {
    /// A state has been fully entered (crossfade complete, or instant transition).
    StateEntered(String),
    /// A state has begun exiting (crossfade started, or instant transition).
    StateExited(String),
}

// ── Data model ────────────────────────────────────────────────────────────────

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct StateMachine {
    pub name: String,
    pub inputs: Vec<InputDef>,
    pub states: Vec<StateNode>,
    pub transitions: Vec<Transition>,
    pub entry_state: usize,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct InputDef {
    pub name: String,
    pub kind: InputKind,
    /// Initial value: bool→0/1, number→float, trigger→0.
    pub default_value: f32,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub enum InputKind {
    Bool,
    Number,
    Trigger,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum StateNode {
    Animation {
        name: String,
        animation_name: String,
        speed: f32,
    },
    Blend1D {
        name: String,
        param: String,
        clips: Vec<Blend1DClip>,
    },
    Blend2D {
        name: String,
        param_x: String,
        param_y: String,
        clips: Vec<Blend2DClip>,
    },
}

impl StateNode {
    pub fn name(&self) -> &str {
        match self {
            StateNode::Animation { name, .. }
            | StateNode::Blend1D { name, .. }
            | StateNode::Blend2D { name, .. } => name.as_str(),
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Blend1DClip {
    pub threshold: f32,
    pub animation_name: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Blend2DClip {
    pub x: f32,
    pub y: f32,
    pub animation_name: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Transition {
    pub from: TransitionFrom,
    pub to: usize,
    pub conditions: Vec<Condition>,
    pub duration_secs: f32,
    pub has_exit_time: bool,
    /// Normalized (0–1) position in the source animation to trigger.
    pub exit_time: f32,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub enum TransitionFrom {
    State(usize),
    AnyState,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum Condition {
    BoolIs { name: String, value: bool },
    NumberLess { name: String, value: f32 },
    NumberGreater { name: String, value: f32 },
    NumberEquals { name: String, value: f32, epsilon: f32 },
    Trigger { name: String },
}

// ── Runtime ───────────────────────────────────────────────────────────────────

pub struct StateMachinePlayer {
    pub definition: StateMachine,
    inputs: HashMap<String, f32>,
    current_state: usize,
    state_time: f32,
    crossfade: Option<Crossfade>,
    pending_events: Vec<SmEvent>,
}

struct Crossfade {
    from: usize,
    from_time: f32,
    to: usize,
    to_time: f32,
    duration: f32,
    elapsed: f32,
}

impl StateMachinePlayer {
    pub fn new(definition: StateMachine) -> Self {
        let mut inputs = HashMap::new();
        for d in &definition.inputs {
            inputs.insert(d.name.clone(), d.default_value);
        }
        let entry = definition.entry_state
            .min(definition.states.len().saturating_sub(1));
        Self { definition, inputs, current_state: entry, state_time: 0.0, crossfade: None, pending_events: Vec::new() }
    }

    /// Drain all events accumulated since the last call (state entries and exits).
    pub fn drain_events(&mut self) -> Vec<SmEvent> {
        std::mem::take(&mut self.pending_events)
    }

    pub fn set_bool(&mut self, name: &str, v: bool) {
        self.inputs.insert(name.to_string(), if v { 1.0 } else { 0.0 });
    }
    pub fn set_number(&mut self, name: &str, v: f32) {
        self.inputs.insert(name.to_string(), v);
    }
    pub fn fire_trigger(&mut self, name: &str) {
        self.inputs.insert(name.to_string(), 1.0);
    }
    pub fn get_number(&self, name: &str) -> f32 {
        self.inputs.get(name).copied().unwrap_or(0.0)
    }
    pub fn current_state_name(&self) -> &str {
        self.definition.states.get(self.current_state)
            .map(StateNode::name).unwrap_or("")
    }

    pub fn advance(&mut self, delta: f32, animations: &[Animation]) -> HashMap<Uuid, NodePose> {
        self.state_time += delta;
        if let Some(cf) = &mut self.crossfade {
            cf.elapsed += delta;
            cf.to_time += delta;
        }

        // Complete finished crossfade.
        let done = self.crossfade.as_ref().map_or(false, |cf| cf.elapsed >= cf.duration);
        if done {
            let cf = self.crossfade.take().unwrap();
            self.current_state = cf.to;
            self.state_time = cf.to_time;
            let entered = self.definition.states[self.current_state].name().to_string();
            self.pending_events.push(SmEvent::StateEntered(entered));
        }

        // Check for a new transition only when not already crossfading.
        if self.crossfade.is_none() {
            if let Some((to, dur, triggers)) = self.check_transitions(animations) {
                let exited = self.definition.states[self.current_state].name().to_string();
                self.pending_events.push(SmEvent::StateExited(exited));
                // Seed elapsed with delta so that crossfades shorter than one tick
                // complete within the same advance call.
                if dur <= 1e-6 || delta >= dur {
                    self.current_state = to;
                    self.state_time = delta.min(dur);
                    let entered = self.definition.states[self.current_state].name().to_string();
                    self.pending_events.push(SmEvent::StateEntered(entered));
                } else {
                    self.crossfade = Some(Crossfade {
                        from: self.current_state,
                        from_time: self.state_time,
                        to,
                        to_time: delta,
                        duration: dur,
                        elapsed: delta,
                    });
                }
                for t in triggers {
                    self.inputs.insert(t, 0.0);
                }
            }
        }

        self.evaluate(animations)
    }

    fn check_transitions(&self, animations: &[Animation]) -> Option<(usize, f32, Vec<String>)> {
        let cur = self.current_state;
        let state_dur = self.state_duration(cur, animations);
        let t_norm = if state_dur > 0.0 { (self.state_time % state_dur.max(1e-6)) / state_dur } else { 0.0 };

        for tr in &self.definition.transitions {
            let from_ok = match &tr.from {
                TransitionFrom::State(s) => *s == cur,
                TransitionFrom::AnyState => true,
            };
            if !from_ok { continue; }
            if tr.has_exit_time && t_norm < tr.exit_time { continue; }

            let mut ok = true;
            let mut consume = vec![];
            for cond in &tr.conditions {
                if !self.eval_condition(cond, &mut consume) {
                    ok = false;
                    break;
                }
            }
            if ok {
                return Some((tr.to, tr.duration_secs, consume));
            }
        }
        None
    }

    fn eval_condition(&self, cond: &Condition, consume: &mut Vec<String>) -> bool {
        match cond {
            Condition::BoolIs { name, value } => {
                (self.inputs.get(name.as_str()).copied().unwrap_or(0.0) >= 0.5) == *value
            }
            Condition::NumberLess { name, value } => {
                self.inputs.get(name.as_str()).copied().unwrap_or(0.0) < *value
            }
            Condition::NumberGreater { name, value } => {
                self.inputs.get(name.as_str()).copied().unwrap_or(0.0) > *value
            }
            Condition::NumberEquals { name, value, epsilon } => {
                (self.inputs.get(name.as_str()).copied().unwrap_or(0.0) - value).abs() <= *epsilon
            }
            Condition::Trigger { name } => {
                if self.inputs.get(name.as_str()).copied().unwrap_or(0.0) >= 0.5 {
                    consume.push(name.clone());
                    true
                } else {
                    false
                }
            }
        }
    }

    fn state_duration(&self, idx: usize, animations: &[Animation]) -> f32 {
        match self.definition.states.get(idx) {
            Some(StateNode::Animation { animation_name, .. }) => {
                animations.iter().find(|a| a.name == *animation_name)
                    .map_or(1.0, |a| a.duration_secs)
            }
            Some(StateNode::Blend1D { clips, .. }) => {
                clips.first()
                    .and_then(|c| animations.iter().find(|a| a.name == c.animation_name))
                    .map_or(1.0, |a| a.duration_secs)
            }
            Some(StateNode::Blend2D { clips, .. }) => {
                clips.first()
                    .and_then(|c| animations.iter().find(|a| a.name == c.animation_name))
                    .map_or(1.0, |a| a.duration_secs)
            }
            None => 1.0,
        }
    }

    fn evaluate(&self, animations: &[Animation]) -> HashMap<Uuid, NodePose> {
        let base = self.eval_state(self.current_state, self.state_time, animations);
        if let Some(cf) = &self.crossfade {
            let t = (cf.elapsed / cf.duration).clamp(0.0, 1.0);
            let target = self.eval_state(cf.to, cf.to_time, animations);
            blend_poses(base, target, t)
        } else {
            base
        }
    }

    fn eval_state(&self, idx: usize, time: f32, animations: &[Animation]) -> HashMap<Uuid, NodePose> {
        match self.definition.states.get(idx) {
            Some(StateNode::Animation { animation_name, speed, .. }) => {
                if let Some(anim) = animations.iter().find(|a| a.name == *animation_name) {
                    let mut p = AnimationPlayer::new(anim.clone());
                    p.time = time * speed;
                    p.evaluate()
                } else {
                    HashMap::new()
                }
            }
            Some(StateNode::Blend1D { param, clips, .. }) => {
                let v = self.inputs.get(param.as_str()).copied().unwrap_or(0.0);
                blend1d(v, clips, time, animations)
            }
            Some(StateNode::Blend2D { param_x, param_y, clips, .. }) => {
                let vx = self.inputs.get(param_x.as_str()).copied().unwrap_or(0.0);
                let vy = self.inputs.get(param_y.as_str()).copied().unwrap_or(0.0);
                blend2d(vx, vy, clips, time, animations)
            }
            None => HashMap::new(),
        }
    }
}

// ── 1D blend tree ─────────────────────────────────────────────────────────────

fn blend1d(
    v: f32,
    clips: &[Blend1DClip],
    time: f32,
    animations: &[Animation],
) -> HashMap<Uuid, NodePose> {
    if clips.is_empty() {
        return HashMap::new();
    }
    if clips.len() == 1 {
        return eval_anim_at(&clips[0].animation_name, time, 1.0, animations);
    }
    let mut sorted: Vec<&Blend1DClip> = clips.iter().collect();
    sorted.sort_by(|a, b| a.threshold.partial_cmp(&b.threshold).unwrap());

    if v <= sorted[0].threshold {
        return eval_anim_at(&sorted[0].animation_name, time, 1.0, animations);
    }
    if v >= sorted.last().unwrap().threshold {
        return eval_anim_at(&sorted.last().unwrap().animation_name, time, 1.0, animations);
    }
    for i in 0..sorted.len() - 1 {
        let lo = sorted[i];
        let hi = sorted[i + 1];
        if v >= lo.threshold && v <= hi.threshold {
            let range = hi.threshold - lo.threshold;
            let t = if range > 1e-6 { (v - lo.threshold) / range } else { 0.0 };
            let a = eval_anim_at(&lo.animation_name, time, 1.0, animations);
            let b = eval_anim_at(&hi.animation_name, time, 1.0, animations);
            return blend_poses(a, b, t);
        }
    }
    HashMap::new()
}

// ── 2D blend tree (inverse-distance weighting) ────────────────────────────────

fn blend2d(
    vx: f32,
    vy: f32,
    clips: &[Blend2DClip],
    time: f32,
    animations: &[Animation],
) -> HashMap<Uuid, NodePose> {
    if clips.is_empty() {
        return HashMap::new();
    }
    let weights: Vec<f32> = clips.iter().map(|c| {
        let d2 = (vx - c.x).powi(2) + (vy - c.y).powi(2);
        if d2 < 1e-8 { f32::INFINITY } else { 1.0 / d2 }
    }).collect();

    if let Some(exact) = weights.iter().position(|w| w.is_infinite()) {
        return eval_anim_at(&clips[exact].animation_name, time, 1.0, animations);
    }

    let total: f32 = weights.iter().sum();
    let mut result: HashMap<Uuid, NodePose> = HashMap::new();
    for (clip, &w) in clips.iter().zip(&weights) {
        let nw = w / total;
        if nw < 1e-6 { continue; }
        for (id, pose) in eval_anim_at(&clip.animation_name, time, 1.0, animations) {
            accumulate_pose(result.entry(id).or_default(), &pose, nw);
        }
    }
    result
}

// ── Pose math helpers ─────────────────────────────────────────────────────────

fn eval_anim_at(
    name: &str,
    time: f32,
    _weight: f32,
    animations: &[Animation],
) -> HashMap<Uuid, NodePose> {
    if let Some(anim) = animations.iter().find(|a| a.name == name) {
        let mut p = AnimationPlayer::new(anim.clone());
        p.time = time;
        p.evaluate()
    } else {
        HashMap::new()
    }
}

pub fn blend_poses(
    mut a: HashMap<Uuid, NodePose>,
    b: HashMap<Uuid, NodePose>,
    t: f32,
) -> HashMap<Uuid, NodePose> {
    scale_all_poses(&mut a, 1.0 - t);
    for (id, pose) in b {
        accumulate_pose(a.entry(id).or_default(), &pose, t);
    }
    a
}

fn scale_all_poses(map: &mut HashMap<Uuid, NodePose>, s: f32) {
    for p in map.values_mut() {
        scale_pose(p, s);
    }
}

fn scale_pose(p: &mut NodePose, s: f32) {
    p.x            = p.x.map(|v| v * s);
    p.y            = p.y.map(|v| v * s);
    p.rotation     = p.rotation.map(|v| v * s);
    p.scale_x      = p.scale_x.map(|v| v * s);
    p.scale_y      = p.scale_y.map(|v| v * s);
    p.skew_x       = p.skew_x.map(|v| v * s);
    p.skew_y       = p.skew_y.map(|v| v * s);
    p.opacity      = p.opacity.map(|v| v * s);
    p.stroke_width = p.stroke_width.map(|v| v * s);
    for ch in &mut p.fill_color { *ch = ch.map(|v| v * s); }
}

fn accumulate_pose(base: &mut NodePose, other: &NodePose, w: f32) {
    fn acc(a: Option<f32>, b: Option<f32>, w: f32) -> Option<f32> {
        match (a, b) {
            (Some(va), Some(vb)) => Some(va + vb * w),
            (Some(va), None) => Some(va),
            (None, Some(vb)) => Some(vb * w),
            (None, None) => None,
        }
    }
    base.x            = acc(base.x,            other.x,            w);
    base.y            = acc(base.y,            other.y,            w);
    base.rotation     = acc(base.rotation,     other.rotation,     w);
    base.scale_x      = acc(base.scale_x,      other.scale_x,      w);
    base.scale_y      = acc(base.scale_y,      other.scale_y,      w);
    base.skew_x       = acc(base.skew_x,       other.skew_x,       w);
    base.skew_y       = acc(base.skew_y,       other.skew_y,       w);
    base.opacity      = acc(base.opacity,      other.opacity,      w);
    base.stroke_width = acc(base.stroke_width, other.stroke_width, w);
    for i in 0..4 {
        base.fill_color[i] = acc(base.fill_color[i], other.fill_color[i], w);
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::{Easing, Keyframe, LoopMode, Property, Track};

    fn anim(name: &str, from: f32, to: f32) -> Animation {
        Animation {
            id: Uuid::new_v4(),
            name: name.to_string(),
            duration_secs: 1.0,
            fps: 60,
            loop_mode: LoopMode::Loop,
            tracks: vec![Track {
                node_id: Uuid::nil(),
                property: Property::X,
                keyframes: vec![
                    Keyframe { time_secs: 0.0, value: from, easing: Easing::Linear },
                    Keyframe { time_secs: 1.0, value: to,   easing: Easing::Linear },
                ],
            }],
        }
    }

    fn simple_sm() -> StateMachine {
        StateMachine {
            name: "test".into(),
            inputs: vec![
                InputDef { name: "speed".into(), kind: InputKind::Number, default_value: 0.0 },
                InputDef { name: "jump".into(),  kind: InputKind::Trigger, default_value: 0.0 },
            ],
            states: vec![
                StateNode::Animation { name: "idle".into(), animation_name: "idle".into(), speed: 1.0 },
                StateNode::Animation { name: "run".into(),  animation_name: "run".into(),  speed: 1.0 },
                StateNode::Animation { name: "jump".into(), animation_name: "jump".into(), speed: 1.0 },
            ],
            transitions: vec![
                Transition {
                    from: TransitionFrom::State(0),
                    to: 1,
                    conditions: vec![Condition::NumberGreater { name: "speed".into(), value: 0.5 }],
                    duration_secs: 0.2,
                    has_exit_time: false,
                    exit_time: 0.0,
                },
                Transition {
                    from: TransitionFrom::State(1),
                    to: 0,
                    conditions: vec![Condition::NumberLess { name: "speed".into(), value: 0.5 }],
                    duration_secs: 0.2,
                    has_exit_time: false,
                    exit_time: 0.0,
                },
                Transition {
                    from: TransitionFrom::AnyState,
                    to: 2,
                    conditions: vec![Condition::Trigger { name: "jump".into() }],
                    duration_secs: 0.1,
                    has_exit_time: false,
                    exit_time: 0.0,
                },
            ],
            entry_state: 0,
        }
    }

    #[test]
    fn starts_in_entry_state() {
        let sm = StateMachinePlayer::new(simple_sm());
        assert_eq!(sm.current_state_name(), "idle");
    }

    #[test]
    fn transitions_on_number_condition() {
        let animations = vec![anim("idle", 0.0, 1.0), anim("run", 0.0, 2.0), anim("jump", 0.0, 3.0)];
        let mut sm = StateMachinePlayer::new(simple_sm());
        sm.set_number("speed", 1.0);
        sm.advance(0.0, &animations);  // triggers transition check
        assert_eq!(sm.current_state_name(), "idle"); // crossfade just started
        sm.advance(0.3, &animations);  // past the 0.2s crossfade
        assert_eq!(sm.current_state_name(), "run");
    }

    #[test]
    fn trigger_consumed_after_use() {
        let animations = vec![anim("idle", 0.0, 1.0), anim("run", 0.0, 2.0), anim("jump", 0.0, 3.0)];
        let mut sm = StateMachinePlayer::new(simple_sm());
        sm.fire_trigger("jump");
        sm.advance(0.2, &animations); // past crossfade
        assert_eq!(sm.current_state_name(), "jump");
        // trigger should be consumed — should NOT transition again from idle→run via another jump
        assert_eq!(sm.get_number("jump"), 0.0);
    }

    #[test]
    fn listener_events_fire_on_transition() {
        let animations = vec![anim("idle", 0.0, 1.0), anim("run", 0.0, 2.0), anim("jump", 0.0, 3.0)];
        let mut sm = StateMachinePlayer::new(simple_sm());
        sm.set_number("speed", 1.0);

        // First advance starts the crossfade → StateExited("idle")
        sm.advance(0.0, &animations);
        let evs = sm.drain_events();
        assert!(
            evs.iter().any(|e| *e == SmEvent::StateExited("idle".into())),
            "expected StateExited(idle), got {evs:?}"
        );

        // Advance past crossfade → StateEntered("run")
        sm.advance(0.3, &animations);
        let evs = sm.drain_events();
        assert!(
            evs.iter().any(|e| *e == SmEvent::StateEntered("run".into())),
            "expected StateEntered(run), got {evs:?}"
        );

        // Events are drained — second drain is empty
        assert!(sm.drain_events().is_empty());
    }

    #[test]
    fn instant_transition_emits_both_events() {
        let animations = vec![anim("idle", 0.0, 1.0), anim("run", 0.0, 2.0), anim("jump", 0.0, 3.0)];
        let mut sm = StateMachinePlayer::new(StateMachine {
            transitions: vec![Transition {
                from: TransitionFrom::State(0),
                to: 1,
                conditions: vec![Condition::NumberGreater { name: "speed".into(), value: 0.5 }],
                duration_secs: 0.0,  // instant
                has_exit_time: false,
                exit_time: 0.0,
            }],
            ..simple_sm()
        });
        sm.set_number("speed", 1.0);
        sm.advance(0.0, &animations);
        let evs = sm.drain_events();
        assert!(evs.iter().any(|e| *e == SmEvent::StateExited("idle".into())), "{evs:?}");
        assert!(evs.iter().any(|e| *e == SmEvent::StateEntered("run".into())), "{evs:?}");
        assert_eq!(sm.current_state_name(), "run");
    }

    #[test]
    fn blend1d_interpolates() {
        let animations = vec![
            anim("walk", 0.0, 10.0),
            anim("run", 0.0, 20.0),
        ];
        let clips = vec![
            Blend1DClip { threshold: 0.0, animation_name: "walk".into() },
            Blend1DClip { threshold: 1.0, animation_name: "run".into() },
        ];
        // At t=0.5 midway through animation, v=0.5: should blend 50/50
        let poses = blend1d(0.5, &clips, 0.5, &animations);
        let x = poses[&Uuid::nil()].x.unwrap();
        // walk at t=0.5 gives x=5, run at t=0.5 gives x=10, blend gives 7.5
        assert!((x - 7.5).abs() < 0.1, "expected 7.5, got {x}");
    }
}
