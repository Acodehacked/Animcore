#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Tool {
    #[default]
    Select,
    Rect,
    Ellipse,
    Pen,
}

impl Tool {
    pub fn label(self) -> &'static str {
        match self {
            Tool::Select  => "S",
            Tool::Rect    => "R",
            Tool::Ellipse => "E",
            Tool::Pen     => "P",
        }
    }

    pub fn tooltip(self) -> &'static str {
        match self {
            Tool::Select  => "Select (V)",
            Tool::Rect    => "Rectangle (R)",
            Tool::Ellipse => "Ellipse (E)",
            Tool::Pen     => "Pen (P)",
        }
    }
}
