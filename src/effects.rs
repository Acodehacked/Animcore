use serde::{Deserialize, Serialize};

use crate::paint::Color;

#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum Effect {
    DropShadow {
        offset_x: f32,
        offset_y: f32,
        blur_radius: f32,
        color: Color,
    },
    OuterGlow {
        blur_radius: f32,
        color: Color,
        opacity: f32,
    },
    InnerGlow {
        blur_radius: f32,
        color: Color,
        opacity: f32,
    },
}
