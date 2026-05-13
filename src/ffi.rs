/// C-compatible FFI layer — enables Flutter (dart:ffi), iOS (Swift), Android (JNI).
///
/// Build:
///   cargo build --release --features skia-renderer
/// Generate C header:
///   cbindgen --crate animcore --output animcore.h
///
/// All functions are `extern "C"` + `#[no_mangle]`. Opaque pointers hide Rust internals.

use std::ffi::{c_char, CStr, CString};
use std::panic::catch_unwind;
use std::ptr;

use crate::format::read_anim;
use crate::renderer::skia::SkiaRenderer;
use crate::scene::Scene;
use crate::schema::Document;
use crate::state_machine::{StateMachine, StateMachinePlayer};

// ── Opaque handles ────────────────────────────────────────────────────────────

pub struct AnimHandle {
    scene:    Scene,
    renderer: SkiaRenderer,
    sm:       Option<StateMachinePlayer>,
}

// ── Lifecycle ─────────────────────────────────────────────────────────────────

/// Create a scene from a JSON-encoded `Document`. Returns null on error.
/// Free with `animcore_destroy`.
#[no_mangle]
pub extern "C" fn animcore_create(json_ptr: *const c_char) -> *mut AnimHandle {
    let result = catch_unwind(|| {
        if json_ptr.is_null() { return ptr::null_mut(); }
        let json = unsafe { CStr::from_ptr(json_ptr) }.to_str().ok()?;
        let doc: Document = serde_json::from_str(json).ok()?;
        make_handle(doc)
    });
    result.ok().flatten().unwrap_or(ptr::null_mut())
}

/// Create a scene from raw `.anim` binary bytes. Returns null on error.
/// Free with `animcore_destroy`.
#[no_mangle]
pub extern "C" fn animcore_create_from_anim(
    data: *const u8,
    len:  u32,
) -> *mut AnimHandle {
    let result = catch_unwind(|| {
        if data.is_null() { return ptr::null_mut(); }
        let bytes = unsafe { std::slice::from_raw_parts(data, len as usize) };
        let doc = read_anim(bytes).ok()?;
        make_handle(doc)
    });
    result.ok().flatten().unwrap_or(ptr::null_mut())
}

fn make_handle(doc: Document) -> Option<*mut AnimHandle> {
    let artboard = doc.artboards.into_iter().next()?;
    Some(Box::into_raw(Box::new(AnimHandle {
        scene:    Scene::new(artboard),
        renderer: SkiaRenderer::new(),
        sm:       None,
    })))
}

/// Free a scene handle.
#[no_mangle]
pub extern "C" fn animcore_destroy(handle: *mut AnimHandle) {
    if !handle.is_null() {
        let _ = catch_unwind(|| unsafe { drop(Box::from_raw(handle)) });
    }
}

// ── Playback ──────────────────────────────────────────────────────────────────

/// Start playing a named animation.
#[no_mangle]
pub extern "C" fn animcore_play(handle: *mut AnimHandle, name_ptr: *const c_char) {
    let _ = catch_unwind(|| {
        if handle.is_null() || name_ptr.is_null() { return; }
        let name = unsafe { CStr::from_ptr(name_ptr) }.to_str().unwrap_or("");
        unsafe { (*handle).scene.play(name) };
    });
}

/// Advance the animation by `delta_secs` seconds.
#[no_mangle]
pub extern "C" fn animcore_advance(handle: *mut AnimHandle, delta_secs: f32) {
    let _ = catch_unwind(|| {
        if handle.is_null() { return; }
        unsafe { (*handle).scene.advance(delta_secs) };
    });
}

// ── State machine controls ────────────────────────────────────────────────────

/// Attach a JSON-encoded `StateMachine` to the handle.
#[no_mangle]
pub extern "C" fn animcore_attach_state_machine(
    handle: *mut AnimHandle,
    json_ptr: *const c_char,
) {
    let _ = catch_unwind(|| {
        if handle.is_null() || json_ptr.is_null() { return; }
        let json = unsafe { CStr::from_ptr(json_ptr) }.to_str().unwrap_or("");
        if let Ok(sm) = serde_json::from_str::<StateMachine>(json) {
            unsafe { (*handle).sm = Some(StateMachinePlayer::new(sm)); }
        }
    });
}

/// Set a boolean input on the attached state machine.
#[no_mangle]
pub extern "C" fn animcore_set_bool(handle: *mut AnimHandle, name_ptr: *const c_char, value: u8) {
    let _ = catch_unwind(|| {
        if handle.is_null() || name_ptr.is_null() { return; }
        let name = unsafe { CStr::from_ptr(name_ptr) }.to_str().unwrap_or("");
        if let Some(sm) = unsafe { (*handle).sm.as_mut() } {
            sm.set_bool(name, value != 0);
        }
    });
}

/// Set a numeric input on the attached state machine.
#[no_mangle]
pub extern "C" fn animcore_set_number(handle: *mut AnimHandle, name_ptr: *const c_char, value: f32) {
    let _ = catch_unwind(|| {
        if handle.is_null() || name_ptr.is_null() { return; }
        let name = unsafe { CStr::from_ptr(name_ptr) }.to_str().unwrap_or("");
        if let Some(sm) = unsafe { (*handle).sm.as_mut() } {
            sm.set_number(name, value);
        }
    });
}

/// Fire a trigger input on the attached state machine.
#[no_mangle]
pub extern "C" fn animcore_fire_trigger(handle: *mut AnimHandle, name_ptr: *const c_char) {
    let _ = catch_unwind(|| {
        if handle.is_null() || name_ptr.is_null() { return; }
        let name = unsafe { CStr::from_ptr(name_ptr) }.to_str().unwrap_or("");
        if let Some(sm) = unsafe { (*handle).sm.as_mut() } {
            sm.fire_trigger(name);
        }
    });
}

/// Advance the state machine by `delta_secs`; also advances the scene animation.
/// The state machine drives which animation plays; call this instead of `animcore_advance`
/// when a state machine is attached.
#[no_mangle]
pub extern "C" fn animcore_advance_sm(handle: *mut AnimHandle, delta_secs: f32) {
    let _ = catch_unwind(|| {
        if handle.is_null() { return; }
        let h = unsafe { &mut *handle };
        if let Some(sm) = &mut h.sm {
            let anims = &h.scene.artboard.animations;
            let _ = sm.advance(delta_secs, anims);
            // Keep the scene's simple player in sync with the SM's active state.
            let state_name = sm.current_state_name().to_string();
            let already_playing = h.scene.player.as_ref()
                .map_or(false, |p| p.animation.name == state_name);
            if !already_playing {
                h.scene.play(&state_name);
            }
        }
        h.scene.advance(delta_secs);
    });
}

// ── Rendering ─────────────────────────────────────────────────────────────────

/// Render into `out_rgba` (must be ≥ `width * height * 4` bytes). Returns bytes written.
#[no_mangle]
pub extern "C" fn animcore_render(
    handle:   *mut AnimHandle,
    out_rgba: *mut u8,
    out_len:  u32,
) -> u32 {
    let result = catch_unwind(|| -> u32 {
        if handle.is_null() || out_rgba.is_null() { return 0; }
        let h = unsafe { &mut *handle };
        h.scene.render(&mut h.renderer);
        let pixels = h.renderer.end_frame();
        let n = pixels.len().min(out_len as usize);
        unsafe { ptr::copy_nonoverlapping(pixels.as_ptr(), out_rgba, n); }
        n as u32
    });
    result.unwrap_or(0)
}

/// Render the current frame as a PNG and write it to `out_buf` (must be large enough).
/// Returns the byte length of the PNG, or 0 on error.
#[no_mangle]
pub extern "C" fn animcore_render_png(
    handle:  *mut AnimHandle,
    out_buf: *mut u8,
    out_len: u32,
) -> u32 {
    let result = catch_unwind(|| -> u32 {
        if handle.is_null() || out_buf.is_null() { return 0; }
        let h = unsafe { &mut *handle };
        h.scene.render(&mut h.renderer);
        let png = h.renderer.encode_png();
        let n = png.len().min(out_len as usize);
        unsafe { ptr::copy_nonoverlapping(png.as_ptr(), out_buf, n); }
        n as u32
    });
    result.unwrap_or(0)
}

// ── Metadata ──────────────────────────────────────────────────────────────────

/// Return the artboard width in pixels.
#[no_mangle]
pub extern "C" fn animcore_width(handle: *const AnimHandle) -> f32 {
    if handle.is_null() { return 0.0; }
    unsafe { (*handle).scene.artboard.width }
}

/// Return the artboard height in pixels.
#[no_mangle]
pub extern "C" fn animcore_height(handle: *const AnimHandle) -> f32 {
    if handle.is_null() { return 0.0; }
    unsafe { (*handle).scene.artboard.height }
}

/// Return the name of the currently active state (or empty string).
/// Caller must free the returned string with `animcore_free_string`.
#[no_mangle]
pub extern "C" fn animcore_current_state(handle: *const AnimHandle) -> *mut c_char {
    let result = catch_unwind(|| {
        if handle.is_null() { return ptr::null_mut(); }
        let name = unsafe {
            (*handle).sm.as_ref()
                .map(|sm| sm.current_state_name().to_string())
                .unwrap_or_default()
        };
        CString::new(name).map(|s| s.into_raw()).unwrap_or(ptr::null_mut())
    });
    result.unwrap_or(ptr::null_mut())
}

/// Free a string returned by the animcore C API.
#[no_mangle]
pub extern "C" fn animcore_free_string(s: *mut c_char) {
    if !s.is_null() {
        let _ = catch_unwind(|| unsafe { drop(CString::from_raw(s)) });
    }
}
