use eframe::egui;
use egui::{Color32, Context, Key, TopBottomPanel, SidePanel, CentralPanel, menu};
use uuid::Uuid;

use animcore::{
    Artboard, Scene,
    paint::{Color, Fill, Paint, Stroke, StrokeCap, StrokeJoin},
    schema::{Geometry, Node, ShapeData},
    read_anim, write_anim,
    state_machine::StateMachine,
};

use crate::canvas::Canvas;
use crate::panels;
use crate::sm_editor::SmEditorWindow;
use crate::tools::Tool;

pub struct AnimCoreApp {
    scene:      Scene,
    canvas:     Canvas,
    tool:       Tool,
    selected:   Option<Uuid>,
    playing:    bool,
    time:       f32,
    sm_editor:  SmEditorWindow,
    sm:         Option<StateMachine>,
    status:     String,
}

impl AnimCoreApp {
    pub fn new(_cc: &eframe::CreationContext<'_>) -> Self {
        let artboard = Artboard {
            id: Uuid::new_v4(),
            name: "Artboard".into(),
            width: 800.0,
            height: 600.0,
            background: Color { r: 1.0, g: 1.0, b: 1.0, a: 1.0 },
            nodes: vec![],
            animations: vec![],
            constraints: vec![],
        };

        Self {
            scene:     Scene::new(artboard),
            canvas:    Canvas::new(),
            tool:      Tool::default(),
            selected:  None,
            playing:   false,
            time:      0.0,
            sm_editor: SmEditorWindow::new(),
            sm:        None,
            status:    "Ready".into(),
        }
    }

    // ── Menu actions ──────────────────────────────────────────────────────────

    fn open_file(&mut self) {
        let path = rfd::FileDialog::new()
            .add_filter("AnimCore", &["anim"])
            .add_filter("SVG", &["svg"])
            .pick_file();

        let Some(path) = path else { return };

        match path.extension().and_then(|e| e.to_str()) {
            Some("anim") => {
                match std::fs::read(&path) {
                    Ok(bytes) => match read_anim(&bytes) {
                        Ok(doc) => {
                            if let Some(ab) = doc.artboards.into_iter().next() {
                                self.scene = Scene::new(ab);
                                self.selected = None;
                                self.status = format!("Opened {:?}", path.file_name().unwrap_or_default());
                            }
                        }
                        Err(e) => self.status = format!("Error: {e}"),
                    },
                    Err(e) => self.status = format!("Read error: {e}"),
                }
            }
            Some("svg") => {
                match std::fs::read_to_string(&path) {
                    Ok(svg) => match animcore::from_svg_str(&svg) {
                        Ok(ab) => {
                            self.scene = Scene::new(ab);
                            self.selected = None;
                            self.status = format!("Imported SVG {:?}", path.file_name().unwrap_or_default());
                        }
                        Err(e) => self.status = format!("SVG error: {e}"),
                    },
                    Err(e) => self.status = format!("Read error: {e}"),
                }
            }
            _ => self.status = "Unknown file type".into(),
        }
    }

    fn save_file(&mut self) {
        let path = rfd::FileDialog::new()
            .add_filter("AnimCore", &["anim"])
            .set_file_name("scene.anim")
            .save_file();

        let Some(path) = path else { return };

        let doc = animcore::schema::Document {
            version: 2,
            artboards: vec![self.scene.artboard.clone()],
        };
        let bytes = write_anim(&doc);
        match std::fs::write(&path, bytes) {
            Ok(_)  => self.status = format!("Saved {:?}", path.file_name().unwrap_or_default()),
            Err(e) => self.status = format!("Write error: {e}"),
        }
    }

    fn export_svg(&mut self) {
        let path = rfd::FileDialog::new()
            .add_filter("SVG", &["svg"])
            .set_file_name("scene.svg")
            .save_file();

        let Some(path) = path else { return };

        let svg = animcore::to_svg_str(&self.scene.artboard);
        match std::fs::write(&path, svg) {
            Ok(_)  => self.status = format!("Exported SVG {:?}", path.file_name().unwrap_or_default()),
            Err(e) => self.status = format!("Write error: {e}"),
        }
    }

    fn add_node(&mut self, geometry: Geometry) {
        let cx = self.scene.artboard.width  * 0.5;
        let cy = self.scene.artboard.height * 0.5;
        let node = Node {
            id:          Uuid::new_v4(),
            name:        format!("{:?}", geometry_name(&geometry)),
            parent_id:   None,
            transform:   animcore::Transform { x: cx, y: cy, ..Default::default() },
            visible:     true,
            opacity:     1.0,
            clip_children: false,
            effects:     vec![],
            shape: Some(ShapeData {
                geometry,
                paint: Paint {
                    fill:        Fill::Solid(Color { r: 0.3, g: 0.6, b: 1.0, a: 1.0 }),
                    stroke:      Some(Stroke {
                        fill:  Fill::Solid(Color { r: 0.0, g: 0.0, b: 0.0, a: 1.0 }),
                        width: 2.0,
                        cap:   StrokeCap::Butt,
                        join:  StrokeJoin::Miter,
                        miter_limit: 4.0,
                        dash:  vec![],
                        dash_offset: 0.0,
                    }),
                    blend_mode:  animcore::BlendMode::Normal,
                    opacity:     1.0,
                },
            }),
        };
        let id = node.id;
        self.scene.artboard.nodes.push(node);
        self.selected = Some(id);
    }

    fn delete_selected(&mut self) {
        if let Some(id) = self.selected.take() {
            self.scene.artboard.nodes.retain(|n| n.id != id);
        }
    }
}

impl eframe::App for AnimCoreApp {
    fn update(&mut self, ctx: &Context, _frame: &mut eframe::Frame) {
        // Keyboard shortcuts
        ctx.input(|i| {
            if i.key_pressed(Key::V) { self.tool = Tool::Select; }
            if i.key_pressed(Key::R) { self.tool = Tool::Rect; }
            if i.key_pressed(Key::E) { self.tool = Tool::Ellipse; }
            if i.key_pressed(Key::P) { self.tool = Tool::Pen; }
            if i.key_pressed(Key::Delete) || i.key_pressed(Key::Backspace) {
                self.delete_selected();
            }
            if i.key_pressed(Key::Space) { self.playing = !self.playing; }
        });

        // Advance playback
        if self.playing {
            let dt = ctx.input(|i| i.unstable_dt).min(0.1);
            self.scene.advance(dt);
            self.time = self.scene.player.as_ref().map(|p| p.time).unwrap_or(0.0);
            ctx.request_repaint();
        }

        // ── Menu bar ──────────────────────────────────────────────────────────
        TopBottomPanel::top("menu_bar").show(ctx, |ui| {
            menu::bar(ui, |ui| {
                ui.menu_button("File", |ui| {
                    if ui.button("Open…").clicked() { self.open_file(); ui.close_menu(); }
                    if ui.button("Save…").clicked() { self.save_file(); ui.close_menu(); }
                    ui.separator();
                    if ui.button("Export SVG…").clicked() { self.export_svg(); ui.close_menu(); }
                });

                ui.menu_button("Add", |ui| {
                    if ui.button("Rectangle").clicked() {
                        self.add_node(Geometry::Rect { width: 120.0, height: 80.0, corner_radius: 0.0 });
                        ui.close_menu();
                    }
                    if ui.button("Ellipse").clicked() {
                        self.add_node(Geometry::Ellipse { radius_x: 60.0, radius_y: 40.0 });
                        ui.close_menu();
                    }
                });

                ui.menu_button("View", |ui| {
                    if ui.button("Reset Camera").clicked() { self.canvas.reset_view(); ui.close_menu(); }
                    ui.separator();
                    if ui.button("State Machine Editor").clicked() {
                        self.sm_editor.open = true;
                        ui.close_menu();
                    }
                });

                // Tool palette in menu bar
                ui.separator();
                for &t in &[Tool::Select, Tool::Rect, Tool::Ellipse, Tool::Pen] {
                    let active = self.tool == t;
                    let btn = egui::Button::new(t.label())
                        .fill(if active { Color32::from_rgb(80, 120, 200) } else { Color32::TRANSPARENT });
                    if ui.add(btn).on_hover_text(t.tooltip()).clicked() {
                        self.tool = t;
                    }
                }
            });
        });

        // ── Status bar ────────────────────────────────────────────────────────
        TopBottomPanel::bottom("status_bar").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label(&self.status);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(format!("Zoom: {:.0}%", self.canvas.zoom * 100.0));
                });
            });
        });

        // ── Timeline ─────────────────────────────────────────────────────────
        TopBottomPanel::bottom("timeline").min_height(120.0).show(ctx, |ui| {
            panels::timeline(ui, &mut self.scene, &mut self.time, &mut self.playing);
        });

        // ── Node tree (left) ──────────────────────────────────────────────────
        SidePanel::left("node_tree").default_width(180.0).show(ctx, |ui| {
            panels::node_tree(ui, &self.scene, &mut self.selected);
        });

        // ── Properties (right) ───────────────────────────────────────────────
        SidePanel::right("properties").default_width(220.0).show(ctx, |ui| {
            panels::properties(ui, &mut self.scene, self.selected);
        });

        // ── Canvas ────────────────────────────────────────────────────────────
        CentralPanel::default().show(ctx, |ui| {
            let response = self.canvas.show(ui, &self.scene, self.selected);

            // Shape creation via drag in the canvas area
            if response.drag_started_by(egui::PointerButton::Primary) {
                match self.tool {
                    Tool::Rect => {
                        self.add_node(Geometry::Rect { width: 100.0, height: 60.0, corner_radius: 0.0 });
                    }
                    Tool::Ellipse => {
                        self.add_node(Geometry::Ellipse { radius_x: 50.0, radius_y: 30.0 });
                    }
                    _ => {}
                }
            }
        });

        // ── SM editor window ──────────────────────────────────────────────────
        if let Some(sm) = &self.sm {
            self.sm_editor.show(ctx, sm);
        } else {
            // Show a placeholder empty SM when opened
            if self.sm_editor.open {
                let placeholder = StateMachine {
                    name: String::new(),
                    inputs: vec![],
                    states: vec![],
                    transitions: vec![],
                    entry_state: 0,
                };
                self.sm_editor.show(ctx, &placeholder);
            }
        }
    }
}

fn geometry_name(g: &Geometry) -> &'static str {
    match g {
        Geometry::Rect { .. }          => "Rect",
        Geometry::Ellipse { .. }       => "Ellipse",
        Geometry::Path(_)              => "Path",
        Geometry::NestedArtboard(_)    => "Nested",
    }
}
