//! A loading spinner that turns in steps: it asks for a frame every
//! [`STEP`], where egui's own asks for one every frame (60 per second while
//! a page loads).

use std::f32::consts::TAU;
use std::time::Duration;

use egui::{Color32, Response, Sense, Shape, Stroke, Ui, Widget};

/// How long each position shows.
const STEP: Duration = Duration::from_millis(100);
/// Positions in a turn.
const POSITIONS: u32 = 12;
/// The arc drawn, in parts of a turn.
const ARC: f32 = 0.7;

#[derive(Default)]
pub struct Spinner {
    size: Option<f32>,
    color: Option<Color32>,
}

impl Spinner {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn size(mut self, size: f32) -> Self {
        self.size = Some(size);
        self
    }

    pub fn color(mut self, color: impl Into<Color32>) -> Self {
        self.color = Some(color.into());
        self
    }
}

/// Where the arc starts at `time` (seconds), in turns: the same through a
/// step.
fn start(time: f64) -> f32 {
    let step = (time / STEP.as_secs_f64()).floor() as u64;
    (step % POSITIONS as u64) as f32 / POSITIONS as f32
}

impl Widget for Spinner {
    fn ui(self, ui: &mut Ui) -> Response {
        let size = self
            .size
            .unwrap_or_else(|| ui.style().spacing.interact_size.y);
        let (rect, response) = ui.allocate_exact_size(egui::vec2(size, size), Sense::hover());
        if ui.is_rect_visible(rect) {
            ui.ctx().request_repaint_after(STEP);
            let color = self
                .color
                .unwrap_or_else(|| ui.visuals().strong_text_color());
            let radius = (rect.height() / 2.0 - 1.5).max(1.0);
            let from = start(ui.input(|i| i.time)) * TAU;
            const POINTS: usize = 16;
            let points = (0..=POINTS)
                .map(|i| {
                    let angle = from + ARC * TAU * i as f32 / POINTS as f32;
                    rect.center() + radius * egui::vec2(angle.cos(), angle.sin())
                })
                .collect();
            ui.painter().add(Shape::line(
                points,
                Stroke::new((size / 8.0).max(1.5), color),
            ));
        }
        response
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_turns_in_steps() {
        assert_eq!(start(0.0), start(0.09));
        assert_ne!(start(0.05), start(0.15));
        assert_eq!(start(0.05), start(POSITIONS as f64 * 0.1 + 0.05));
    }
}
