# AnimCore — Rust Animation Engine

> Open-source, multi-platform animation runtime + editor. Think Rive but fully open, smaller files, and Rust-native.

---

## Vision

AnimCore is a Rive-equivalent built in Rust:
- **Runtime-first**: the playback engine ships as a tiny native library, WASM module, or FFI bundle
- **Compact file format**: binary `.anim` format that beats Rive's `.riv` in size for equivalent scenes
- **State machines**: full FSM with blend trees, triggers, and number/bool inputs
- **Vector-native**: GPU-accelerated smooth vector rendering via `vello`; CPU fallback via `tiny-skia`
- **Multiplatform**: Web (WASM), Flutter, iOS, Android, Bevy, desktop (egui editor)
- **SVG interop**: import SVGs as editable paths, export scenes to SVG

---

## Repository Layout (target)

```
animcore/               ← core runtime library (this crate)
  src/
    schema.rs           ← data model (Document, Artboard, Node, Shape, …)
    playback.rs         ← AnimationPlayer, interpolation, easing
    state_machine.rs    ← FSM, blend trees, input variables
    renderer/
      mod.rs
      vello.rs          ← GPU renderer (vello)
      skia.rs           ← CPU renderer (tiny-skia)
    paint.rs            ← Fill, Stroke, Gradient, BlendMode
    path.rs             ← CubicBezier paths, SVG path data parser
    format/
      mod.rs
      binary.rs         ← compact .anim serializer/deserializer
      svg.rs            ← SVG import / export
    constraints.rs      ← IK, aim, distance constraints
    bones.rs            ← skeleton rigging, mesh deformation
    lib.rs

animcore-editor/        ← egui desktop editor (separate crate)
animcore-wasm/          ← wasm-bindgen JS bindings
animcore-flutter/       ← dart:ffi C bindings
animcore-bevy/          ← Bevy plugin
```

---

## Core Dependencies

| Crate | Purpose |
|---|---|
| `nalgebra` | transforms, matrices, vectors |
| `serde` + `serde_json` | dev/debug serialization |
| `uuid` | node identity |
| `vello` | GPU vector rendering (production renderer) |
| `tiny-skia` | CPU vector rendering (fallback / server-side) |
| `peniko` | paint / brush types shared with vello |
| `kurbo` | 2D geometry, bezier math |
| `petgraph` | state machine graph |
| `bitcode` or `rkyv` | zero-copy binary file format |
| `wasm-bindgen` | WASM bindings |
| `cbindgen` | C header for FFI runtimes |
| `egui` + `eframe` | editor UI |

---

## File Format `.anim`

Binary, versioned, chunk-based (similar to IFF/RIFF but tighter):

```
[magic: 4 bytes "ANIM"]
[version: u16]
[chunk: type(u16) + length(u32) + data]
  ARTB — artboard definitions
  NODE — node tree
  PATH — path data (f32 control points, delta-encoded)
  ANIM — animation tracks + keyframes
  SMSM — state machine definitions
  GRAD — gradient table
  ASST — embedded assets (images, fonts)
  ENND — end marker
```

Goals: < 10 KB for a typical UI animation, gzip-friendly, streamable.

---

## Rendering Architecture

```
Document
  └─ Artboard
       └─ Node tree  ──→  RenderTree (flattened, sorted by draw order)
                               └─ Renderer trait
                                    ├─ VelloRenderer  (wgpu-backed, GPU)
                                    └─ SkiaRenderer   (tiny-skia, CPU)
```

`Renderer` trait:
```rust
pub trait Renderer {
    fn begin_frame(&mut self, width: u32, height: u32);
    fn draw_path(&mut self, path: &AnimPath, paint: &Paint, transform: Mat3);
    fn end_frame(&mut self) -> RgbaImage;
}
```

---

## State Machine Model

Modelled after Rive's state machine but simpler FSM to start:

```
StateMachine {
  inputs: Vec<Input>,       // Bool, Number, Trigger
  states: Vec<State>,       // AnimationState | BlendState | AnyState
  transitions: Vec<Transition {
    from, to,
    conditions: Vec<Condition>,
    duration_secs: f32,
  }>
}
```

Blend states support 1D (single float axis) and 2D (two-float blend space).

---

## Phases

### Phase 1 — Solid Foundation ✅ COMPLETE
- [x] Document / Artboard / Node schema
- [x] Keyframe animation player
- [x] Linear + cubic bezier easing + Hold easing
- [x] LoopMode: Once / Loop / PingPong
- [x] Rich paint: Fill / Stroke / Gradient (linear + radial) / BlendMode
- [x] Cubic bezier path type (`AnimPath`) with SVG `d`-string parser
- [x] Transform hierarchy with skew, parent→child world matrices
- [x] `tiny-skia` CPU renderer (`SkiaRenderer`) behind `skia-renderer` feature flag
- [x] `Scene` type: ties artboard + player + renderer
- [x] 14 unit tests passing (path, playback, transform, renderer)

### Phase 2 — Visual Richness ✅ COMPLETE
- [x] Linear and radial gradients (Phase 1 carry-over)
- [x] Stroke properties: cap, join, dash, miter (Phase 1 carry-over)
- [x] Blend modes: all 12 standard modes
- [x] Drop shadows — Gaussian blur (3× box blur) composited before fill
- [x] Outer glow — same shadow engine, zero offset
- [x] InnerGlow stub (Phase 3 full implementation)
- [x] Clipping masks — `push_clip` / `pop_clip` on Renderer trait; `clip_children` Node flag
- [x] SVG import — rect, circle, ellipse, path, polygon, polyline, line, nested `<g>` groups; hex/rgb/named colors; transform attribute (translate/scale/rotate/skewX/skewY/matrix)
- [x] SVG export — all geometry types, solid/gradient fills, strokes, opacity, linearGradient/radialGradient `<defs>`

### Phase 3 — Advanced Animation
- [ ] Multi-animation artboard (multiple named `Animation` objects)
- [ ] Animation mixing with weights
- [ ] Nested artboards (artboard as a node)
- [ ] Per-property easing overrides
- [ ] Additive animation layers
- [ ] Constraints: IK, aim-constraint, distance-constraint

### Phase 4 — State Machines
- [ ] `StateMachine` struct + petgraph-backed FSM
- [ ] Input variables: Bool, Number, Trigger
- [ ] Transition conditions and cross-fade blending
- [ ] 1D blend trees (locomotion speed → walk/run)
- [ ] 2D blend trees (x/y axes → directional movement)
- [ ] Listener events (fire callbacks on state entry/exit)

### Phase 5 — Binary File Format
- [ ] Chunk-based `.anim` writer
- [ ] `.anim` reader with version checks
- [ ] Delta encoding for path control points
- [ ] String table deduplication
- [ ] Embedded asset blobs
- [ ] Migration between format versions

### Phase 6 — GPU Renderer
- [ ] Integrate `vello` + `wgpu` renderer
- [ ] `Renderer` trait abstraction
- [ ] `VelloRenderer` implementation
- [ ] `SkiaRenderer` CPU fallback
- [ ] Offscreen rendering for thumbnail generation

### Phase 7 — Platform Runtimes
- [ ] `animcore-wasm`: `wasm-bindgen` JS API, canvas2d / WebGPU output
- [ ] `animcore-flutter`: `dart:ffi` C API via `cbindgen`
- [ ] `animcore-bevy`: Bevy `Plugin` + `AnimCoreBundle` component
- [ ] `animcore-ios`: Swift package via static lib + `cbindgen` headers
- [ ] `animcore-android`: JNI bindings via `jni` crate

### Phase 8 — Editor (`animcore-editor`)
- [ ] `egui` + `eframe` desktop shell
- [ ] Canvas viewport with pan/zoom
- [ ] Node tree panel
- [ ] Timeline panel with drag-and-drop keyframes
- [ ] Properties panel (transform, paint, easing curve editor)
- [ ] Shape tools: rect, ellipse, pen (bezier)
- [ ] State machine visual editor (nodes + arrows)
- [ ] File open/save (`.anim` format)
- [ ] SVG import dialog

### Phase 9 — Advanced Features
- [ ] Skeleton rigging (bone hierarchy)
- [ ] Mesh deformation with bones
- [ ] Text nodes (freetype / cosmic-text)
- [ ] Scroll/physics simulation (spring, inertia)
- [ ] Audio sync markers
- [ ] Lottie import (subset)
- [ ] GIF / APNG / WebP export

---

## Coding Conventions

- **No `unwrap()` in library code** — use `Result<_, AnimError>` everywhere
- **No `std::sync::Mutex`** in the render hot path — design for single-threaded ticking
- **Prefer `f32`** for all animation values; `f64` only for time accumulation
- **UUID for stable IDs** in the schema; `u32` handles for runtime performance
- **Feature flags** for heavy deps (`vello`, `egui`) so the runtime stays lean
  ```toml
  [features]
  default = ["skia-renderer"]
  gpu = ["vello", "wgpu"]
  editor = ["egui", "eframe", "gpu"]
  wasm = ["wasm-bindgen"]
  ```
- All public structs derive `Serialize, Deserialize, Debug, Clone`
- No comments explaining *what* code does; only *why* for non-obvious choices

---

## Testing Strategy

- Unit tests: interpolation math, bezier solver, easing curves
- Snapshot tests: render a known scene → compare PNG hashes
- Fuzzing: binary format reader must not panic on arbitrary input
- Integration: load a `.anim` file, tick state machine, verify state transitions

---

## Performance Targets

| Metric | Target |
|---|---|
| File size (typical UI scene) | < 15 KB uncompressed |
| Frame tick (no renderer) | < 50 µs per artboard |
| WASM bundle (runtime only) | < 200 KB gzip |
| Memory per artboard | < 2 MB |

---

## Non-Goals (for now)

- 3D rendering or 3D transforms
- Video decode
- Real-time collaboration / multiplayer editing
- Cloud sync
