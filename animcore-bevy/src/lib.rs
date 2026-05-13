/// AnimCore Bevy plugin.
///
/// Add `AnimCorePlugin` to your Bevy app to enable AnimCore animation playback.
/// Spawn entities with `AnimCoreBundle` to display animations.
///
/// # Example
///
/// ```no_run
/// use bevy::prelude::*;
/// use animcore_bevy::{AnimCorePlugin, AnimCoreBundle, AnimCoreHandle};
/// use animcore::{Artboard, Scene};
///
/// fn main() {
///     App::new()
///         .add_plugins(DefaultPlugins)
///         .add_plugins(AnimCorePlugin)
///         .add_systems(Startup, spawn_anim)
///         .run();
/// }
///
/// fn spawn_anim(mut commands: Commands, asset_server: Res<AssetServer>) {
///     commands.spawn(AnimCoreBundle::new(my_artboard()).playing("idle"));
/// }
/// # fn my_artboard() -> Artboard { unimplemented!() }
/// ```

use bevy::prelude::*;
use bevy::render::texture::Image;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use bevy::sprite::Sprite;

use animcore::{Artboard, Scene};
use animcore::renderer::skia::SkiaRenderer;
use animcore::state_machine::{StateMachine, StateMachinePlayer, SmEvent};

// ── Plugin ────────────────────────────────────────────────────────────────────

/// Registers AnimCore systems into a Bevy `App`.
pub struct AnimCorePlugin;

impl Plugin for AnimCorePlugin {
    fn build(&self, app: &mut App) {
        app
            .add_event::<AnimStateEvent>()
            .add_systems(Update, (advance_animations, upload_frame_textures).chain());
    }
}

// ── Components ────────────────────────────────────────────────────────────────

/// Hold an AnimCore scene on a Bevy entity.
#[derive(Component)]
pub struct AnimCoreHandle {
    pub scene:    Scene,
    pub sm:       Option<StateMachinePlayer>,
    /// When true, the system uploads a new `Image` every frame.
    pub auto_render: bool,
}

impl AnimCoreHandle {
    pub fn new(artboard: Artboard) -> Self {
        Self { scene: Scene::new(artboard), sm: None, auto_render: true }
    }

    /// Start playing a named animation immediately.
    pub fn playing(mut self, name: &str) -> Self {
        self.scene.play(name);
        self
    }

    /// Attach a state machine.
    pub fn with_state_machine(mut self, sm: StateMachine) -> Self {
        self.sm = Some(StateMachinePlayer::new(sm));
        self
    }

    pub fn set_bool(&mut self, name: &str, v: bool) {
        if let Some(sm) = &mut self.sm { sm.set_bool(name, v); }
    }
    pub fn set_number(&mut self, name: &str, v: f32) {
        if let Some(sm) = &mut self.sm { sm.set_number(name, v); }
    }
    pub fn fire_trigger(&mut self, name: &str) {
        if let Some(sm) = &mut self.sm { sm.fire_trigger(name); }
    }
    pub fn current_state(&self) -> &str {
        self.sm.as_ref().map(|s| s.current_state_name()).unwrap_or("")
    }
}

/// Convenience bundle: `AnimCoreHandle` + the Bevy `Sprite` components needed to display it.
#[derive(Bundle)]
pub struct AnimCoreBundle {
    pub handle: AnimCoreHandle,
    pub sprite: Sprite,
    pub transform: Transform,
    pub global_transform: GlobalTransform,
    pub visibility: Visibility,
    pub inherited_visibility: InheritedVisibility,
    pub view_visibility: ViewVisibility,
}

impl AnimCoreBundle {
    pub fn new(artboard: Artboard) -> Self {
        Self {
            handle: AnimCoreHandle::new(artboard),
            sprite: Sprite::default(),
            transform: Transform::default(),
            global_transform: GlobalTransform::default(),
            visibility: Visibility::Visible,
            inherited_visibility: InheritedVisibility::default(),
            view_visibility: ViewVisibility::default(),
        }
    }

    pub fn playing(mut self, name: &str) -> Self {
        self.handle = self.handle.playing(name);
        self
    }
}

// ── Events ────────────────────────────────────────────────────────────────────

/// Fired when an AnimCore state machine changes state.
#[derive(Event)]
pub struct AnimStateEvent {
    pub entity: Entity,
    pub event:  SmEvent,
}

// ── Systems ───────────────────────────────────────────────────────────────────

fn advance_animations(
    time:     Res<Time>,
    mut ev:   EventWriter<AnimStateEvent>,
    mut query: Query<(Entity, &mut AnimCoreHandle)>,
) {
    let dt = time.delta_secs();
    for (entity, mut handle) in query.iter_mut() {
        if let Some(sm) = &mut handle.sm {
            let anims = &handle.scene.artboard.animations;
            let _ = sm.advance(dt, anims);
            for e in sm.drain_events() {
                ev.send(AnimStateEvent { entity, event: e });
            }
            let state = sm.current_state_name().to_string();
            let playing = handle.scene.player.as_ref()
                .map_or(false, |p| p.animation.name == state);
            if !playing {
                let name = state;
                handle.scene.play(&name);
            }
        }
        handle.scene.advance(dt);
    }
}

fn upload_frame_textures(
    mut images: ResMut<Assets<Image>>,
    mut query:  Query<(&AnimCoreHandle, &mut Sprite)>,
) {
    for (handle, mut sprite) in query.iter_mut() {
        if !handle.auto_render { continue; }

        let w = handle.scene.artboard.width as u32;
        let h = handle.scene.artboard.height as u32;
        if w == 0 || h == 0 { continue; }

        let mut renderer = SkiaRenderer::new();
        handle.scene.render(&mut renderer);
        let rgba = renderer.end_frame();

        let image = Image::new(
            Extent3d { width: w, height: h, depth_or_array_layers: 1 },
            TextureDimension::D2,
            rgba,
            TextureFormat::Rgba8UnormSrgb,
            bevy::render::render_asset::RenderAssetUsages::RENDER_WORLD,
        );
        let handle = images.add(image);
        sprite.image = handle;
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use animcore::paint::Color;
    use animcore::Document;

    fn test_artboard() -> Artboard {
        Artboard {
            id: uuid::Uuid::new_v4(),
            name: "Test".into(),
            width: 64.0,
            height: 64.0,
            background: Color::WHITE,
            nodes: vec![],
            animations: vec![],
            constraints: vec![],
        }
    }

    #[test]
    fn handle_creates_scene() {
        let h = AnimCoreHandle::new(test_artboard());
        assert_eq!(h.scene.artboard.name, "Test");
        assert_eq!(h.current_state(), "");
    }

    #[test]
    fn plugin_registers_without_panic() {
        let mut app = App::new();
        app.add_plugins(AnimCorePlugin);
        app.update(); // single tick; no window / GPU needed
    }

    #[test]
    fn advance_system_runs() {
        let mut app = App::new();
        app.add_plugins((bevy::time::TimePlugin, AnimCorePlugin));
        app.world_mut().spawn(AnimCoreBundle::new(test_artboard()).playing("idle"));
        app.update();
    }
}
