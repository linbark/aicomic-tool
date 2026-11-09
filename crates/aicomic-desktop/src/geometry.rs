use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct BoardView {
    pub zoom: f32,
    pub pan: [f32; 2],
    pub positions: HashMap<usize, [f32; 2]>,
}
impl Default for BoardView {
    fn default() -> Self {
        Self {
            zoom: 1.0,
            pan: [24.0, 24.0],
            positions: HashMap::new(),
        }
    }
}
impl BoardView {
    pub fn world(&self, screen: [f32; 2]) -> [f32; 2] {
        [
            (screen[0] - self.pan[0]) / self.zoom,
            (screen[1] - self.pan[1]) / self.zoom,
        ]
    }
    pub fn zoom_at(&mut self, factor: f32, cursor: [f32; 2]) {
        if !factor.is_finite() || factor <= 0.0 {
            return;
        }
        let point = self.world(cursor);
        self.zoom = (self.zoom * factor).clamp(0.1, 4.0);
        self.pan = [
            cursor[0] - point[0] * self.zoom,
            cursor[1] - point[1] * self.zoom,
        ];
    }
    pub fn drag_card(&mut self, index: usize, delta: [f32; 2]) {
        let position = self.position(index);
        self.positions.insert(
            index,
            [
                position[0] + delta[0] / self.zoom,
                position[1] + delta[1] / self.zoom,
            ],
        );
    }
    pub fn position(&self, index: usize) -> [f32; 2] {
        self.positions
            .get(&index)
            .copied()
            .unwrap_or([(index % 2) as f32 * 300.0, (index / 2) as f32 * 240.0])
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn zoom_keeps_the_point_under_cursor_fixed() {
        let mut view = BoardView::default();
        let cursor = [400.0, 300.0];
        let before = view.world(cursor);
        view.zoom_at(1.5, cursor);
        assert!((view.world(cursor)[0] - before[0]).abs() < 0.001);
        assert!((view.world(cursor)[1] - before[1]).abs() < 0.001);
        assert!((view.zoom - 1.5).abs() < 0.001);
    }
    #[test]
    fn card_drag_respects_zoom() {
        let mut view = BoardView {
            zoom: 0.5,
            ..BoardView::default()
        };
        let initial = view.position(2);
        view.drag_card(2, [20.0, 10.0]);
        assert_eq!(view.position(2), [initial[0] + 40.0, initial[1] + 20.0]);
    }
}
