use egui::{Color32, Pos2, Rect, Stroke, Vec2, Window};
use animcore::state_machine::{StateMachine, TransitionFrom};

pub struct SmEditorWindow {
    pub open: bool,
    node_positions: Vec<Pos2>,
}

impl SmEditorWindow {
    pub fn new() -> Self {
        Self { open: false, node_positions: vec![] }
    }

    pub fn show(&mut self, ctx: &egui::Context, sm: &StateMachine) {
        if !self.open { return; }

        if self.node_positions.len() != sm.states.len() {
            self.node_positions = layout_states(sm.states.len());
        }

        // Extract to locals to avoid split-borrow through closure
        let mut open = self.open;
        let positions = &mut self.node_positions;

        Window::new("State Machine")
            .open(&mut open)
            .resizable(true)
            .default_size([640.0, 480.0])
            .show(ctx, |ui| {
                draw_graph(ui, sm, positions);
            });

        self.open = open;
    }
}

fn draw_graph(ui: &mut egui::Ui, sm: &StateMachine, positions: &mut Vec<Pos2>) {
    let available = ui.available_size();
    let (response, painter) = ui.allocate_painter(available, egui::Sense::click_and_drag());
    let origin = response.rect.min;

    // Transitions as arrows
    for t in &sm.transitions {
        let from_idx = match &t.from {
            TransitionFrom::State(i) => Some(*i),
            TransitionFrom::AnyState => None,
        };
        let to_idx = t.to;

        if let Some(fi) = from_idx {
            if fi < positions.len() && to_idx < positions.len() {
                let fp = origin + positions[fi].to_vec2() + Vec2::new(60.0, 18.0);
                let tp = origin + positions[to_idx].to_vec2() + Vec2::new(60.0, 18.0);
                painter.line_segment([fp, tp], Stroke::new(1.5, Color32::from_gray(180)));
                draw_arrow_head(&painter, fp, tp);
            }
        }
    }

    let node_w = 120.0_f32;
    let node_h = 36.0_f32;

    for (i, state) in sm.states.iter().enumerate() {
        let pos = origin + positions[i].to_vec2();
        let rect = Rect::from_min_size(pos, Vec2::new(node_w, node_h));

        let bg = if i == 0 {
            Color32::from_rgb(60, 120, 200)
        } else {
            Color32::from_rgb(60, 60, 80)
        };

        painter.rect_filled(rect, 6.0, bg);
        painter.rect_stroke(rect, 6.0, Stroke::new(1.0, Color32::from_gray(160)));
        painter.text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            state.name(),
            egui::FontId::proportional(12.0),
            Color32::WHITE,
        );

        let node_resp = ui.interact(rect, ui.id().with(i), egui::Sense::drag());
        if node_resp.dragged() {
            positions[i] += node_resp.drag_delta();
        }
    }

    painter.text(
        origin + Vec2::new(8.0, available.y - 16.0),
        egui::Align2::LEFT_BOTTOM,
        "Blue = entry state  •  Drag nodes to rearrange",
        egui::FontId::proportional(11.0),
        Color32::from_gray(140),
    );
}

fn layout_states(count: usize) -> Vec<Pos2> {
    let cols = ((count as f32).sqrt().ceil() as usize).max(1);
    (0..count)
        .map(|i| {
            Pos2::new(
                20.0 + (i % cols) as f32 * 160.0,
                20.0 + (i / cols) as f32 * 80.0,
            )
        })
        .collect()
}

fn draw_arrow_head(painter: &egui::Painter, from: Pos2, to: Pos2) {
    let dir = (to - from).normalized();
    let perp = Vec2::new(-dir.y, dir.x);
    let tip = to;
    let left  = tip - dir * 10.0 + perp * 5.0;
    let right = tip - dir * 10.0 - perp * 5.0;
    painter.add(egui::Shape::convex_polygon(
        vec![tip, left, right],
        Color32::from_gray(180),
        Stroke::NONE,
    ));
}
