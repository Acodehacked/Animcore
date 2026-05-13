/// AnimCore WebAssembly bindings.
///
/// Build:
///   wasm-pack build --target web     # → ES module + .wasm
///   wasm-pack build --target bundler # → for webpack / Vite
///   wasm-pack build --target nodejs  # → CommonJS
///
/// Usage (TypeScript):
///   import init, { WasmScene } from './animcore_wasm.js';
///   await init();
///   const scene = WasmScene.from_anim(new Uint8Array(animBytes));
///   scene.play("idle");
///   requestAnimationFrame(function tick(now) {
///     scene.advance(1 / 60);
///     const rgba = scene.render_rgba();
///     // paint rgba to canvas…
///     requestAnimationFrame(tick);
///   });

use wasm_bindgen::prelude::*;

use animcore::{read_anim, Scene, Document};
use animcore::renderer::skia::SkiaRenderer;
use animcore::state_machine::{StateMachine, StateMachinePlayer, SmEvent};

// ── WasmScene ─────────────────────────────────────────────────────────────────

/// A self-contained animation scene ready to tick and render.
#[wasm_bindgen]
pub struct WasmScene {
    scene: Scene,
    sm:    Option<StateMachinePlayer>,
}

#[wasm_bindgen]
impl WasmScene {
    // ── Constructors ──────────────────────────────────────────────────────────

    /// Load from a JSON-encoded `Document`.
    #[wasm_bindgen(constructor)]
    pub fn from_json(json: &str) -> Result<WasmScene, JsValue> {
        let doc: Document = serde_json::from_str(json)
            .map_err(|e| JsValue::from_str(&format!("JSON parse error: {e}")))?;
        let artboard = doc.artboards.into_iter().next()
            .ok_or_else(|| JsValue::from_str("Document has no artboards"))?;
        Ok(WasmScene { scene: Scene::new(artboard), sm: None })
    }

    /// Load from raw `.anim` binary bytes.
    pub fn from_anim(bytes: &[u8]) -> Result<WasmScene, JsValue> {
        let doc = read_anim(bytes)
            .map_err(|e| JsValue::from_str(&format!(".anim load error: {e}")))?;
        let artboard = doc.artboards.into_iter().next()
            .ok_or_else(|| JsValue::from_str("Document has no artboards"))?;
        Ok(WasmScene { scene: Scene::new(artboard), sm: None })
    }

    // ── State machine ─────────────────────────────────────────────────────────

    /// Attach a state machine from a JSON-encoded `StateMachine`.
    pub fn attach_state_machine(&mut self, json: &str) -> Result<(), JsValue> {
        let sm: StateMachine = serde_json::from_str(json)
            .map_err(|e| JsValue::from_str(&format!("SM parse error: {e}")))?;
        self.sm = Some(StateMachinePlayer::new(sm));
        Ok(())
    }

    /// Set a boolean input on the attached state machine.
    pub fn set_bool(&mut self, name: &str, value: bool) {
        if let Some(sm) = &mut self.sm { sm.set_bool(name, value); }
    }

    /// Set a numeric input on the attached state machine.
    pub fn set_number(&mut self, name: &str, value: f32) {
        if let Some(sm) = &mut self.sm { sm.set_number(name, value); }
    }

    /// Fire a trigger input on the attached state machine.
    pub fn fire_trigger(&mut self, name: &str) {
        if let Some(sm) = &mut self.sm { sm.fire_trigger(name); }
    }

    /// Name of the currently active state machine state, or empty string.
    pub fn current_state(&self) -> String {
        self.sm.as_ref().map(|s| s.current_state_name().to_string()).unwrap_or_default()
    }

    /// Drain and return state-change events since last call, as a JSON array.
    /// E.g. `[{"StateEntered":"run"},{"StateExited":"idle"}]`
    pub fn drain_events(&mut self) -> String {
        let evs: Vec<serde_json::Value> = self.sm.as_mut()
            .map(|sm| sm.drain_events())
            .unwrap_or_default()
            .into_iter()
            .map(|e| match e {
                SmEvent::StateEntered(s) => serde_json::json!({"StateEntered": s}),
                SmEvent::StateExited(s)  => serde_json::json!({"StateExited":  s}),
            })
            .collect();
        serde_json::to_string(&evs).unwrap_or_else(|_| "[]".into())
    }

    // ── Playback ──────────────────────────────────────────────────────────────

    /// Start playing a named animation directly (bypasses state machine).
    pub fn play(&mut self, name: &str) {
        self.scene.play(name);
    }

    /// Advance by `delta_secs`. If a state machine is attached, it drives animation.
    pub fn advance(&mut self, delta_secs: f32) {
        if let Some(sm) = &mut self.sm {
            let anims = &self.scene.artboard.animations;
            let _ = sm.advance(delta_secs, anims);
            let state = sm.current_state_name().to_string();
            let playing = self.scene.player.as_ref()
                .map_or(false, |p| p.animation.name == state);
            if !playing { self.scene.play(&state); }
        }
        self.scene.advance(delta_secs);
    }

    // ── Rendering ─────────────────────────────────────────────────────────────

    /// Render the current frame and return raw RGBA8 bytes (width×height×4).
    pub fn render_rgba(&mut self) -> Vec<u8> {
        let mut r = SkiaRenderer::new();
        self.scene.render(&mut r);
        r.end_frame()
    }

    /// Render the current frame as a PNG byte vector.
    pub fn render_png(&mut self) -> Vec<u8> {
        self.scene.render_to_png()
    }

    // ── Metadata ──────────────────────────────────────────────────────────────

    pub fn width(&self)  -> f32 { self.scene.artboard.width }
    pub fn height(&self) -> f32 { self.scene.artboard.height }
    pub fn artboard_name(&self) -> String { self.scene.artboard.name.clone() }

    /// List animation names as a JSON array string.
    pub fn animation_names(&self) -> String {
        let names: Vec<&str> = self.scene.artboard.animations.iter()
            .map(|a| a.name.as_str())
            .collect();
        serde_json::to_string(&names).unwrap_or_else(|_| "[]".into())
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn minimal_json() -> &'static str {
        r#"{"version":1,"artboards":[{"id":"00000000-0000-0000-0000-000000000001","name":"Test","width":100.0,"height":100.0,"background":{"r":1.0,"g":1.0,"b":1.0,"a":1.0},"nodes":[],"animations":[],"constraints":[]}]}"#
    }

    #[test]
    fn from_json_round_trip() {
        let mut s = WasmScene::from_json(minimal_json()).unwrap();
        assert_eq!(s.width(), 100.0);
        assert_eq!(s.artboard_name(), "Test");
        s.advance(0.016);
        let rgba = s.render_rgba();
        assert_eq!(rgba.len(), 100 * 100 * 4);
    }

    #[test]
    fn render_png_valid_header() {
        let mut s = WasmScene::from_json(minimal_json()).unwrap();
        let png = s.render_png();
        assert!(png.len() > 8, "PNG too short");
        assert_eq!(&png[0..4], b"\x89PNG", "not a PNG");
    }

    #[test]
    fn from_anim_bytes() {
        use animcore::{write_anim, Artboard, Document};
        use animcore::paint::Color;
        let doc = Document {
            version: 1,
            artboards: vec![Artboard {
                id: uuid::Uuid::new_v4(),
                name: "Btn".into(),
                width: 64.0, height: 64.0,
                background: Color::WHITE,
                nodes: vec![], animations: vec![], constraints: vec![],
            }],
        };
        let bytes = write_anim(&doc);
        let scene = WasmScene::from_anim(&bytes).unwrap();
        assert_eq!(scene.artboard_name(), "Btn");
    }
}
