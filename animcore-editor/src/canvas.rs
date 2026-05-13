use egui::{Color32, Pos2, Rect, Response, Sense, TextureHandle, Ui, Vec2};
use animcore::Scene;
use animcore::renderer::Renderer;
use animcore::renderer::skia::SkiaRenderer;
use animcore::schema::{Geometry, Node};
use animcore::transform::Transform;
use uuid::Uuid;

pub struct Canvas {
    pub offset: Vec2,
    pub zoom: f32,
    texture: Option<TextureHandle>,
}

impl Canvas {
    pub fn new() -> Self {
        Self { offset: Vec2::ZERO, zoom: 1.0, texture: None }
    }

    pub fn show(&mut self, ui: &mut Ui, scene: &Scene, selected: Option<Uuid>) -> Response {
        let available = ui.available_size();
        let (response, painter) =
            ui.allocate_painter(available, Sense::click_and_drag());

        // Middle-mouse or alt+drag → pan
        if response.dragged_by(egui::PointerButton::Middle)
            || (response.dragged_by(egui::PointerButton::Primary)
                && ui.input(|i| i.modifiers.alt))
        {
            self.offset += response.drag_delta();
        }

        // Scroll → zoom around cursor
        let scroll = ui.input(|i| i.smooth_scroll_delta.y);
        if scroll != 0.0 {
            let factor = (scroll * 0.001).exp();
            if let Some(cursor) = response.hover_pos() {
                let pre = self.canvas_to_artboard(cursor, response.rect);
                self.zoom = (self.zoom * factor).clamp(0.05, 40.0);
                let post = self.canvas_to_artboard(cursor, response.rect);
                self.offset += Vec2::new(
                    (post.x - pre.x) * self.zoom,
                    (post.y - pre.y) * self.zoom,
                );
            } else {
                self.zoom = (self.zoom * factor).clamp(0.05, 40.0);
            }
        }

        self.upload_texture(ui.ctx(), scene);

        let aw = scene.artboard.width;
        let ah = scene.artboard.height;

        if let Some(tex) = &self.texture {
            let center = response.rect.center() + self.offset;
            let half = Vec2::new(aw * self.zoom * 0.5, ah * self.zoom * 0.5);
            let dest = Rect::from_min_max(center - half, center + half);

            // Artboard background shadow
            painter.rect_filled(
                dest.expand(4.0),
                2.0,
                egui::Color32::from_black_alpha(60),
            );

            painter.image(
                tex.id(),
                dest,
                Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
                egui::Color32::WHITE,
            );
        }

        // Selection bounds overlay
        if let Some(id) = selected {
            if let Some(node) = scene.artboard.nodes.iter().find(|n| n.id == id) {
                if let Some([min_x, min_y, max_x, max_y]) = node_aabb(node) {
                    let min_s = self.artboard_to_screen(
                        Pos2::new(min_x, min_y), response.rect, aw, ah,
                    );
                    let max_s = self.artboard_to_screen(
                        Pos2::new(max_x, max_y), response.rect, aw, ah,
                    );
                    let sel = Rect::from_min_max(min_s, max_s);
                    let accent = Color32::from_rgb(80, 160, 255);

                    painter.rect_stroke(sel, 0.0, egui::Stroke::new(1.5, accent));

                    // 8 handles: 4 corners + 4 edge midpoints
                    let handles = [
                        sel.left_top(),
                        sel.center_top(),
                        sel.right_top(),
                        sel.right_center(),
                        sel.right_bottom(),
                        sel.center_bottom(),
                        sel.left_bottom(),
                        sel.left_center(),
                    ];
                    for &h in &handles {
                        let hr = Rect::from_center_size(h, Vec2::splat(7.0));
                        painter.rect_filled(hr, 1.0, Color32::WHITE);
                        painter.rect_stroke(hr, 1.0, egui::Stroke::new(1.5, accent));
                    }
                }
            }
        }

        response
    }

    fn canvas_to_artboard(&self, pos: Pos2, rect: Rect) -> Pos2 {
        let center = rect.center() + self.offset;
        Pos2::new(
            (pos.x - center.x) / self.zoom,
            (pos.y - center.y) / self.zoom,
        )
    }

    // Maps an artboard-space point (origin = artboard top-left) to screen pixels.
    fn artboard_to_screen(&self, pos: Pos2, rect: Rect, aw: f32, ah: f32) -> Pos2 {
        let center = rect.center() + self.offset;
        Pos2::new(
            center.x - aw * self.zoom * 0.5 + pos.x * self.zoom,
            center.y - ah * self.zoom * 0.5 + pos.y * self.zoom,
        )
    }

    fn upload_texture(&mut self, ctx: &egui::Context, scene: &Scene) {
        let w = scene.artboard.width as u32;
        let h = scene.artboard.height as u32;
        if w == 0 || h == 0 { return; }

        let mut r = SkiaRenderer::new();
        scene.render(&mut r);
        let rgba = r.end_frame();

        let image = egui::ColorImage::from_rgba_unmultiplied(
            [w as usize, h as usize],
            &rgba,
        );

        match &mut self.texture {
            Some(t) => t.set(image, egui::TextureOptions::LINEAR),
            None => {
                self.texture = Some(ctx.load_texture(
                    "artboard",
                    image,
                    egui::TextureOptions::LINEAR,
                ));
            }
        }
    }

    pub fn reset_view(&mut self) {
        self.offset = Vec2::ZERO;
        self.zoom = 1.0;
    }
}

// Computes an axis-aligned bounding box [min_x, min_y, max_x, max_y] in artboard space.
fn node_aabb(node: &Node) -> Option<[f32; 4]> {
    let shape = node.shape.as_ref()?;

    let local_corners: Vec<[f32; 2]> = match &shape.geometry {
        Geometry::Rect { width, height, .. } => vec![
            [0.0, 0.0],
            [*width, 0.0],
            [*width, *height],
            [0.0, *height],
        ],
        Geometry::Ellipse { radius_x, radius_y } => vec![
            [-*radius_x, -*radius_y],
            [*radius_x, -*radius_y],
            [*radius_x, *radius_y],
            [-*radius_x, *radius_y],
        ],
        Geometry::Path(p) => p.points.iter().copied().collect(),
        Geometry::NestedArtboard(ab) => vec![
            [0.0, 0.0],
            [ab.width, 0.0],
            [ab.width, ab.height],
            [0.0, ab.height],
        ],
    };

    if local_corners.is_empty() {
        return None;
    }

    let mat = node.transform.to_matrix();
    let world_pts: Vec<[f32; 2]> = local_corners.iter()
        .map(|&p| Transform::apply(&mat, p))
        .collect();

    let min_x = world_pts.iter().map(|p| p[0]).fold(f32::MAX, f32::min);
    let min_y = world_pts.iter().map(|p| p[1]).fold(f32::MAX, f32::min);
    let max_x = world_pts.iter().map(|p| p[0]).fold(f32::MIN, f32::max);
    let max_y = world_pts.iter().map(|p| p[1]).fold(f32::MIN, f32::max);

    Some([min_x, min_y, max_x, max_y])
}
