use crate::app::Message;
pub use crate::geometry::BoardView;
use crate::ui;
use iced::advanced::text::{self, Paragraph as _};
use iced::widget::canvas::{self, Action, Event, Frame, Geometry, Path, Stroke, Text};
use iced::{Color, Point, Rectangle, Renderer, Size, Theme, Vector, mouse};

const CARD_TEXT_WIDTH: f32 = 248.0;
const BODY_LINE_HEIGHT: f32 = 16.0;
const BODY_PREVIEW_HEIGHT: f32 = 130.0;

fn text_height(content: &str, width: f32, size: f32, line_height: f32) -> f32 {
    type Paragraph = <Renderer as text::Renderer>::Paragraph;
    Paragraph::with_text(text::Text {
        content,
        bounds: Size::new(width, f32::INFINITY),
        size: size.into(),
        line_height: text::LineHeight::Absolute(line_height.into()),
        font: iced::Font::default(),
        align_x: text::Alignment::Default,
        align_y: iced::alignment::Vertical::Top,
        shaping: text::Shaping::Advanced,
        wrapping: text::Wrapping::WordOrGlyph,
    })
    .min_height()
    .max(line_height)
}

/// Find the longest visible prefix by the same font layout used for drawing.
fn fitting_prefix(content: &str, width: f32, size: f32, line_height: f32, height: f32) -> String {
    if content.is_empty() || height < line_height {
        return String::new();
    }
    let mut pending_boundaries = content
        .char_indices()
        .skip(1)
        .map(|(offset, _)| offset)
        .chain([content.len()]);
    let mut boundaries = vec![0];
    let mut low = 0;
    let mut high: usize = 16;
    // Grow only through the visible prefix. Long scripts must not be reshaped
    // or have every character indexed merely to paint one card line.
    loop {
        let missing = high.saturating_sub(boundaries.len() - 1);
        boundaries.extend(pending_boundaries.by_ref().take(missing));
        high = high.min(boundaries.len() - 1);
        if text_height(&content[..boundaries[high]], width, size, line_height) > height {
            break;
        }
        if boundaries[high] == content.len() {
            return content.to_owned();
        }
        low = high;
        high = high.saturating_mul(2);
    }
    while high - low > 1 {
        let middle = low + (high - low) / 2;
        if text_height(&content[..boundaries[middle]], width, size, line_height) <= height {
            low = middle;
        } else {
            high = middle;
        }
    }
    content[..boundaries[low]].to_owned()
}

fn draw_card_body(frame: &mut Frame, body: &str, origin: Point) {
    let mut y = 0.0;
    let mut overflow = false;
    'lines: for line in body.split('\n') {
        let mut content = line.trim_start_matches([' ', '\t']);
        let indent = line.len() - content.len();
        // Keep the original hierarchy on wrapped lines by positioning each paragraph.
        let indent = line[..indent]
            .chars()
            .map(|c| if c == '\t' { 16.0 } else { 4.0 })
            .sum::<f32>()
            .min(CARD_TEXT_WIDTH - 24.0);
        let width = CARD_TEXT_WIDTH - indent;
        loop {
            if y + BODY_LINE_HEIGHT > BODY_PREVIEW_HEIGHT {
                overflow = true;
                break 'lines;
            }
            let visible = fitting_prefix(content, width, 12.0, BODY_LINE_HEIGHT, BODY_LINE_HEIGHT);
            if !visible.is_empty() {
                frame.fill_text(Text {
                    content: visible.clone(),
                    position: origin + Vector::new(16.0 + indent, 48.0 + y),
                    // Already wrapped in board coordinates. Iced's GPU canvas
                    // scales the font without scaling a finite max_width.
                    color: ui::TEXT_SECONDARY,
                    size: 12.0.into(),
                    line_height: text::LineHeight::Absolute(BODY_LINE_HEIGHT.into()),
                    shaping: text::Shaping::Advanced,
                    ..Text::default()
                });
            }
            y += BODY_LINE_HEIGHT;
            if visible.len() == content.len() {
                break;
            }
            if visible.is_empty() {
                overflow = true;
                break 'lines;
            }
            content = &content[visible.len()..];
        }
    }
    if overflow {
        frame.fill_text(Text {
            content: "… 内容未完，完整内容见详情".into(),
            position: origin + Vector::new(16.0, 186.0),
            color: ui::TEXT_SECONDARY,
            size: 11.0.into(),
            shaping: text::Shaping::Advanced,
            ..Text::default()
        });
    }
}

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
        frame.fill_rectangle(Point::ORIGIN, bounds.size(), ui::INK);
        for x in (0..bounds.width as usize).step_by(28) {
            for y in (0..bounds.height as usize).step_by(28) {
                frame.fill(&Path::circle(Point::new(x as f32, y as f32), 0.7), ui::LINE);
            }
        }
        if self.cards.is_empty() {
            frame.fill_text(Text {
                content: "生成结果将在这里展开\n滚轮缩放 · 拖动卡片 · 拖动空白平移".into(),
                position: Point::new(28.0, 36.0),
                color: ui::TEXT_SECONDARY,
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
                frame.fill(&rect, ui::SURFACE);
                frame.stroke(
                    &rect,
                    Stroke::default().with_color(ui::LINE).with_width(1.0),
                );
                frame.fill_rectangle(origin, Size::new(3.0, 210.0), ui::GOLD);
                let title = card.title.replace(['\n', '\r'], " ");
                let mut visible_title = fitting_prefix(&title, CARD_TEXT_WIDTH, 16.0, 20.0, 20.0);
                if visible_title.len() < title.len() {
                    // Leave space for the visible overflow marker inside the same width.
                    visible_title =
                        fitting_prefix(&title, CARD_TEXT_WIDTH - 16.0, 16.0, 20.0, 20.0);
                    visible_title.push('…');
                }
                frame.fill_text(Text {
                    content: visible_title,
                    position: origin + Vector::new(16.0, 15.0),
                    color: Color::WHITE,
                    size: 16.0.into(),
                    line_height: text::LineHeight::Absolute(20.0.into()),
                    shaping: text::Shaping::Advanced,
                    ..Text::default()
                });
                draw_card_body(frame, &card.body, origin);
            }
        });
        frame.fill_text(Text {
            content: format!("{}%", (self.view.zoom * 100.0).round()),
            position: Point::new((bounds.width - 52.0).max(16.0), 14.0),
            color: ui::TEXT_MUTED,
            size: 11.0.into(),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preview_wraps_by_glyph_width_instead_of_a_fixed_character_count() {
        let latin = "abcdefghijklmnopqrstuvwxyz".repeat(4);
        let chinese = "这是一段用于检查画布内容换行的长文字".repeat(4);
        let latin_prefix = fitting_prefix(&latin, CARD_TEXT_WIDTH, 12.0, 16.0, 16.0);
        let chinese_prefix = fitting_prefix(&chinese, CARD_TEXT_WIDTH, 12.0, 16.0, 16.0);
        assert!(latin_prefix.chars().count() > 26);
        assert!(chinese_prefix.chars().count() < 26);
        assert!(latin.starts_with(&latin_prefix));
        assert!(chinese.starts_with(&chinese_prefix));
        assert!(text_height(&latin_prefix, CARD_TEXT_WIDTH, 12.0, 16.0) <= 16.0);
        assert!(text_height(&chinese_prefix, CARD_TEXT_WIDTH, 12.0, 16.0) <= 16.0);
    }

    #[test]
    fn long_script_lines_have_the_same_visible_prefix_as_a_short_excerpt() {
        for excerpt in [
            "这是一段很长的中文剧本台词，画布只应测量实际可见的部分。".repeat(4),
            "A long dialogue line should preserve exactly the same visible prefix. ".repeat(4),
        ] {
            let long_line = format!("{excerpt}{}", "后续剧本内容仍然完整保留。".repeat(10_000));
            let expected = fitting_prefix(&excerpt, CARD_TEXT_WIDTH, 12.0, 16.0, 16.0);
            assert!(!expected.is_empty());
            assert!(expected.len() < excerpt.len());
            assert_eq!(
                fitting_prefix(&long_line, CARD_TEXT_WIDTH, 12.0, 16.0, 16.0),
                expected
            );
        }
    }
}
