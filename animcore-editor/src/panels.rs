use egui::{Color32, DragValue, Grid, ScrollArea, Ui};
use animcore::Scene;
use uuid::Uuid;

// ── Node Tree ─────────────────────────────────────────────────────────────────

pub fn node_tree(ui: &mut Ui, scene: &Scene, selected: &mut Option<Uuid>) {
    ui.heading("Nodes");
    ui.separator();
    ScrollArea::vertical().show(ui, |ui| {
        for node in &scene.artboard.nodes {
            let indent = if node.parent_id.is_some() { "  " } else { "" };
            let label = format!("{}{}", indent, node.name);
            let is_sel = *selected == Some(node.id);
            if ui.selectable_label(is_sel, &label).clicked() {
                *selected = Some(node.id);
            }
        }
    });
}

// ── Properties Panel ──────────────────────────────────────────────────────────

pub fn properties(ui: &mut Ui, scene: &mut Scene, selected: Option<Uuid>) {
    ui.heading("Properties");
    ui.separator();

    let Some(id) = selected else {
        ui.label("Nothing selected");
        return;
    };

    let Some(node) = scene.artboard.nodes.iter_mut().find(|n| n.id == id) else {
        ui.label("Node not found");
        return;
    };

    ui.label(format!("Name: {}", node.name));
    ui.add_space(4.0);

    Grid::new("transform_grid").num_columns(2).spacing([8.0, 4.0]).show(ui, |ui| {
        ui.label("X");
        ui.add(DragValue::new(&mut node.transform.x).speed(0.5));
        ui.end_row();

        ui.label("Y");
        ui.add(DragValue::new(&mut node.transform.y).speed(0.5));
        ui.end_row();

        ui.label("Rotation");
        ui.add(DragValue::new(&mut node.transform.rotation)
            .speed(0.5)
            .suffix("°"));
        ui.end_row();

        ui.label("Scale X");
        ui.add(DragValue::new(&mut node.transform.scale_x).speed(0.01).range(0.01..=100.0_f32));
        ui.end_row();

        ui.label("Scale Y");
        ui.add(DragValue::new(&mut node.transform.scale_y).speed(0.01).range(0.01..=100.0_f32));
        ui.end_row();

        ui.label("Opacity");
        ui.add(DragValue::new(&mut node.opacity).speed(0.01).range(0.0..=1.0_f32));
        ui.end_row();
    });

    ui.add_space(8.0);

    if let Some(shape) = &mut node.shape {
        ui.label("Paint");
        ui.separator();

        if let animcore::Fill::Solid(ref mut c) = shape.paint.fill {
            let color32 = Color32::from_rgba_premultiplied(
                (c.r * 255.0) as u8,
                (c.g * 255.0) as u8,
                (c.b * 255.0) as u8,
                (c.a * 255.0) as u8,
            );
            let mut col = color32;
            if ui.color_edit_button_srgba(&mut col).changed() {
                let [r, g, b, a] = col.to_array();
                c.r = r as f32 / 255.0;
                c.g = g as f32 / 255.0;
                c.b = b as f32 / 255.0;
                c.a = a as f32 / 255.0;
            }
        }

        if let Some(stroke) = &mut shape.paint.stroke {
            Grid::new("stroke_grid").num_columns(2).spacing([8.0, 4.0]).show(ui, |ui| {
                ui.label("Stroke Width");
                ui.add(DragValue::new(&mut stroke.width).speed(0.1).range(0.0..=100.0_f32));
                ui.end_row();
                if let animcore::Fill::Solid(ref mut sc) = stroke.fill {
                    ui.label("Stroke Color");
                    let mut col = egui::Color32::from_rgba_premultiplied(
                        (sc.r * 255.0) as u8,
                        (sc.g * 255.0) as u8,
                        (sc.b * 255.0) as u8,
                        (sc.a * 255.0) as u8,
                    );
                    if ui.color_edit_button_srgba(&mut col).changed() {
                        let [r, g, b, a] = col.to_array();
                        sc.r = r as f32 / 255.0;
                        sc.g = g as f32 / 255.0;
                        sc.b = b as f32 / 255.0;
                        sc.a = a as f32 / 255.0;
                    }
                    ui.end_row();
                }
            });
        }
    }
}

// ── Timeline Panel ────────────────────────────────────────────────────────────

pub fn timeline(ui: &mut Ui, scene: &mut Scene, time: &mut f32, playing: &mut bool) {
    ui.horizontal(|ui| {
        let label = if *playing { "⏸" } else { "▶" };
        if ui.button(label).clicked() {
            *playing = !*playing;
        }
        if ui.button("⏮").clicked() {
            *time = 0.0;
        }

        let duration = scene
            .player
            .as_ref()
            .map(|p| p.animation.duration_secs)
            .unwrap_or(1.0)
            .max(0.01);

        ui.label(format!("{:.2}s / {:.2}s", time, duration));
        ui.add(egui::Slider::new(time, 0.0..=duration).show_value(false));
    });

    ui.separator();

    let Some(player) = scene.player.as_ref() else {
        ui.label("No animation loaded");
        return;
    };

    let duration = player.animation.duration_secs.max(0.01);

    // Snapshot immutable data so we can mutate scene.player after rendering
    let track_data: Vec<(String, Vec<f32>)> = player.animation.tracks.iter()
        .map(|t| (
            format!("{:?}", t.property),
            t.keyframes.iter().map(|kf| kf.time_secs).collect(),
        ))
        .collect();

    let available = ui.available_size();
    let track_h = 20.0_f32;
    let label_w = 120.0_f32;
    let timeline_w = (available.x - label_w).max(1.0);

    let mut kf_mutations: Vec<(usize, usize, f32)> = vec![];

    ScrollArea::vertical().show(ui, |ui| {
        for (ti, (name, kf_times)) in track_data.iter().enumerate() {
            ui.horizontal(|ui| {
                ui.set_min_height(track_h);
                ui.add_sized([label_w, track_h], egui::Label::new(name.as_str()));

                let (rect, track_resp) = ui.allocate_exact_size(
                    egui::Vec2::new(timeline_w, track_h),
                    egui::Sense::click(),
                );

                // Click on track background → seek to that time
                if track_resp.clicked() {
                    if let Some(pos) = track_resp.interact_pointer_pos() {
                        let t = ((pos.x - rect.left()) / timeline_w * duration)
                            .clamp(0.0, duration);
                        *time = t;
                    }
                }

                let painter = ui.painter_at(rect);
                painter.rect_filled(rect, 0.0, Color32::from_gray(40));

                for (ki, &kf_t) in kf_times.iter().enumerate() {
                    let x = rect.left() + (kf_t / duration) * timeline_w;
                    let center = egui::Pos2::new(x, rect.center().y);

                    // Allocate a drag-sensitive rect around the diamond
                    let kf_rect = egui::Rect::from_center_size(
                        center,
                        egui::Vec2::new(12.0, 14.0),
                    );
                    let kf_resp = ui.interact(
                        kf_rect,
                        egui::Id::new(("kf", ti, ki)),
                        egui::Sense::drag(),
                    );

                    if kf_resp.dragged() {
                        let dt = kf_resp.drag_delta().x / timeline_w * duration;
                        kf_mutations.push((ti, ki, (kf_t + dt).clamp(0.0, duration)));
                    }

                    let diamond_color = if kf_resp.dragged() || kf_resp.hovered() {
                        Color32::from_rgb(255, 240, 120)
                    } else {
                        Color32::from_rgb(255, 200, 80)
                    };

                    let diamond = [
                        egui::Pos2::new(center.x, center.y - 6.0),
                        egui::Pos2::new(center.x + 5.0, center.y),
                        egui::Pos2::new(center.x, center.y + 6.0),
                        egui::Pos2::new(center.x - 5.0, center.y),
                    ];
                    painter.add(egui::Shape::convex_polygon(
                        diamond.to_vec(),
                        diamond_color,
                        egui::Stroke::NONE,
                    ));
                }

                // Time cursor
                let cx = rect.left() + (*time / duration) * timeline_w;
                painter.line_segment(
                    [egui::Pos2::new(cx, rect.top()), egui::Pos2::new(cx, rect.bottom())],
                    egui::Stroke::new(1.0, Color32::from_rgb(255, 80, 80)),
                );
            });
        }
    });

    // Apply keyframe mutations after the immutable borrow of scene.player is released
    for (ti, ki, new_t) in kf_mutations {
        if let Some(player) = scene.player.as_mut() {
            if let Some(track) = player.animation.tracks.get_mut(ti) {
                if let Some(kf) = track.keyframes.get_mut(ki) {
                    kf.time_secs = new_t;
                }
            }
        }
    }
}
