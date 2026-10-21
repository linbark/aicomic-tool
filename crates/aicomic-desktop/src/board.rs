use crate::app::Message;
pub use crate::geometry::BoardView;
use iced::widget::canvas::{self, Action, Event, Frame, Geometry, Path, Stroke, Text};
use iced::{Color, Point, Rectangle, Renderer, Size, Theme, Vector, mouse};

pub struct BoardCard {
    pub title: String,
    pub body: String,
}
pub struct Board {
    pub cards: Vec<BoardCard>,
    pub view: BoardView,
}
#[derive(Default)]
pub struct Interaction {
    drag: Option<(Option<usize>, Point)>,
}

impl canvas::Program<Message> for Board {
    type State = Interaction;
    fn update(
        &self,
        state: &mut Interaction,
        event: &Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Option<Action<Message>> {
        match event {
            Event::Mouse(mouse::Event::ButtonReleased(
                mouse::Button::Left | mouse::Button::Middle,
            )) => {
                state.drag = None;
                Some(Action::request_redraw())
            }
            Event::Mouse(mouse::Event::ButtonPressed(
                button @ (mouse::Button::Left | mouse::Button::Middle),
            )) => {
                let cursor = cursor.position_in(bounds)?;
                let point = self.view.world([cursor.x, cursor.y]);
                let selected = if *button == mouse::Button::Left {
                    (0..self.cards.len()).rev().find(|index| {
                        let pos = self.view.position(*index);
                        Rectangle::new(Point::new(pos[0], pos[1]), Size::new(280.0, 210.0))
                            .contains(Point::new(point[0], point[1]))
                    })
                } else {
                    None
                };
                state.drag = Some((selected, cursor));
                Some(Action::request_redraw().and_capture())
            }
            Event::Mouse(mouse::Event::CursorMoved { .. }) => {
                let (selected, previous) = state.drag?;
                let cursor = cursor.position_in(bounds)?;
                let delta = cursor - previous;
                let mut view = self.view.clone();
                if let Some(index) = selected {
                    view.drag_card(index, [delta.x, delta.y]);
                } else {
                    view.pan[0] += delta.x;
                    view.pan[1] += delta.y;
                }
                state.drag = Some((selected, cursor));
                Some(Action::publish(Message::Board(view)).and_capture())
            }
            Event::Mouse(mouse::Event::WheelScrolled { delta }) => {
                let cursor = cursor.position_in(bounds)?;
                let dy = match delta {
                    mouse::ScrollDelta::Lines { y, .. } => *y * 0.12,
                    mouse::ScrollDelta::Pixels { y, .. } => *y * 0.002,
                };
                let mut view = self.view.clone();
                view.zoom_at(dy.exp(), [cursor.x, cursor.y]);
                Some(Action::publish(Message::Board(view)).and_capture())
            }
            _ => None,
        }
    }
    fn draw(
        &self,
        _state: &Interaction,
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        frame.fill_rectangle(Point::ORIGIN, bounds.size(), Color::from_rgb8(12, 20, 35));
        for x in (0..bounds.width as usize).step_by(28) {
            for y in (0..bounds.height as usize).step_by(28) {
                frame.fill(
                    &Path::circle(Point::new(x as f32, y as f32), 0.7),
                    Color::from_rgb8(43, 54, 76),
                );
            }
        }
        if self.cards.is_empty() {
            frame.fill_text(Text {
                content: "生成结果将在这里展开\n滚轮缩放 · 拖动卡片 · 拖动空白平移".into(),
                position: Point::new(28.0, 36.0),
                color: Color::from_rgb8(148, 163, 184),
                size: 18.0.into(),
                ..Text::default()
            });
        }
        frame.with_save(|frame| {
            frame.translate(Vector::new(self.view.pan[0], self.view.pan[1]));
            frame.scale(self.view.zoom);
            for (index, card) in self.cards.iter().enumerate() {
                let pos = self.view.position(index);
                let origin = Point::new(pos[0], pos[1]);
                let rect = Path::rectangle(origin, Size::new(280.0, 210.0));
                frame.fill(&rect, Color::from_rgb8(24, 34, 55));
                frame.stroke(
                    &rect,
                    Stroke::default()
                        .with_color(Color::from_rgb8(73, 80, 124))
                        .with_width(1.0),
                );
                frame.fill_rectangle(
                    origin,
                    Size::new(3.0, 210.0),
                    Color::from_rgb8(129, 140, 248),
                );
                frame.fill_text(Text {
                    content: card.title.chars().take(20).collect(),
                    position: origin + Vector::new(16.0, 15.0),
                    color: Color::WHITE,
                    size: 16.0.into(),
                    ..Text::default()
                });
                let lines: Vec<String> = card
                    .body
                    .lines()
                    .filter(|s| !s.trim().is_empty())
                    .take(7)
                    .map(|s| s.chars().take(26).collect())
                    .collect();
                frame.fill_text(Text {
                    content: lines.join("\n"),
                    position: origin + Vector::new(16.0, 48.0),
                    color: Color::from_rgb8(183, 195, 218),
                    size: 12.0.into(),
                    ..Text::default()
                });
            }
        });
        frame.fill_text(Text {
            content: format!("{}%", (self.view.zoom * 100.0).round()),
            position: Point::new(16.0, bounds.height - 30.0),
            color: Color::from_rgb8(148, 163, 184),
            size: 12.0.into(),
            ..Text::default()
        });
        vec![frame.into_geometry()]
    }
    fn mouse_interaction(
        &self,
        state: &Interaction,
        _bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> mouse::Interaction {
        if state.drag.is_some() {
            mouse::Interaction::Grabbing
        } else {
            mouse::Interaction::default()
        }
    }
}
