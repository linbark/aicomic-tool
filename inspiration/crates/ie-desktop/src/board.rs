//! Native functional graph: cards, ports, directed edges and pointer dragging.
use crate::app::Message;
use crate::atelier::{self, Atelier};
use crate::style::game;
use iced::widget::canvas::{self, Action, Frame, Geometry, Path, Stroke, Text};
use iced::{Color, Font, Point, Rectangle, Renderer, Size, Theme, mouse};

const CARD: Size = Size::new(224., 142.);
pub struct Board<'a> {
    pub atelier: &'a Atelier,
}
#[derive(Default)]
pub struct State {
    drag: Option<(usize, Point)>,
}
impl Board<'_> {
    fn hit(&self, p: Point) -> Option<usize> {
        self.atelier
            .workshop
            .nodes
            .iter()
            .enumerate()
            .rev()
            .find(|(_, n)| Rectangle::new(Point::new(n.position.0, n.position.1), CARD).contains(p))
            .map(|(i, _)| i)
    }
}
fn send(m: atelier::Message) -> Option<Action<Message>> {
    Some(Action::publish(Message::Atelier(m)).and_capture())
}
fn label(
    frame: &mut Frame,
    value: impl Into<String>,
    x: f32,
    y: f32,
    size: f32,
    color: Color,
    display: bool,
) {
    frame.fill_text(Text {
        content: value.into(),
        position: Point::new(x, y),
        max_width: 196.,
        size: size.into(),
        color,
        font: Font::with_name(if display {
            "Songti SC"
        } else if cfg!(target_os = "macos") {
            "PingFang SC"
        } else {
            "Microsoft YaHei"
        }),
        shaping: iced::advanced::text::Shaping::Advanced,
        ..Text::default()
    });
}
fn truncate(value: &str, n: usize) -> String {
    if value.chars().count() > n {
        format!("{}…", value.chars().take(n).collect::<String>())
    } else {
        value.into()
    }
}
impl canvas::Program<Message> for Board<'_> {
    type State = State;
    fn update(
        &self,
        state: &mut State,
        event: &canvas::Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Option<Action<Message>> {
        let p = cursor.position_in(bounds);
        match event {
            canvas::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                if let Some(p) = p
                    && let Some(i) = self.hit(p)
                {
                    let n = &self.atelier.workshop.nodes[i];
                    if !self.atelier.running() {
                        state.drag = Some((i, Point::new(p.x - n.position.0, p.y - n.position.1)));
                    }
                    return send(atelier::Message::SelectNode(i));
                }
            }
            canvas::Event::Mouse(mouse::Event::CursorMoved { .. }) => {
                if let Some((i, offset)) = state.drag
                    && let Some(p) = p
                {
                    return send(atelier::Message::MoveNode(
                        i,
                        (p.x - offset.x).clamp(8., bounds.width - CARD.width - 8.),
                        (p.y - offset.y).clamp(8., bounds.height - CARD.height - 8.),
                    ));
                }
            }
            canvas::Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
                if state.drag.take().is_some() {
                    return Some(Action::capture());
                }
            }
            _ => {}
        }
        None
    }
    fn mouse_interaction(
        &self,
        state: &State,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> mouse::Interaction {
        if state.drag.is_some() {
            mouse::Interaction::Grabbing
        } else if cursor
            .position_in(bounds)
            .is_some_and(|p| self.hit(p).is_some())
        {
            mouse::Interaction::Grab
        } else {
            mouse::Interaction::default()
        }
    }
    fn draw(
        &self,
        _state: &State,
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        let mut shade = game::INK;
        shade.a = 0.79;
        frame.fill_rectangle(Point::ORIGIN, bounds.size(), shade);
        let grid = Color {
            a: 0.055,
            ..game::PAPER
        };
        for x in (0..bounds.width as u32).step_by(32) {
            frame.stroke(
                &Path::line(
                    Point::new(x as f32, 0.),
                    Point::new(x as f32, bounds.height),
                ),
                Stroke::default().with_color(grid),
            );
        }
        for y in (0..bounds.height as u32).step_by(32) {
            frame.stroke(
                &Path::line(Point::new(0., y as f32), Point::new(bounds.width, y as f32)),
                Stroke::default().with_color(grid),
            );
        }
        let nodes = &self.atelier.workshop.nodes;
        for node in nodes {
            for input in node
                .inputs
                .split(',')
                .map(str::trim)
                .filter(|s| !s.is_empty())
            {
                if let Some(from) = nodes.iter().find(|n| n.id == input) {
                    let start = Point::new(
                        from.position.0 + CARD.width,
                        from.position.1 + CARD.height / 2.,
                    );
                    let end = Point::new(node.position.0, node.position.1 + CARD.height / 2.);
                    let bend = ((end.x - start.x).abs() * 0.5).max(42.);
                    let path = Path::new(|p| {
                        p.move_to(start);
                        p.bezier_curve_to(
                            Point::new(start.x + bend, start.y),
                            Point::new(end.x - bend, end.y),
                            end,
                        );
                    });
                    frame.stroke(
                        &path,
                        Stroke::default().with_width(2.).with_color(game::COPPER),
                    );
                    let arrow = Path::new(|p| {
                        p.move_to(Point::new(end.x - 7., end.y - 4.));
                        p.line_to(end);
                        p.line_to(Point::new(end.x - 7., end.y + 4.));
                    });
                    frame.stroke(
                        &arrow,
                        Stroke::default().with_width(2.).with_color(game::COPPER),
                    );
                }
            }
        }
        for (i, node) in nodes.iter().enumerate() {
            let (x, y) = node.position;
            let rect = Path::rounded_rectangle(Point::new(x, y), CARD, 7.0.into());
            let selected = self.atelier.workshop.selected == Some(i);
            frame.fill(
                &rect,
                if node.op == "source" {
                    game::PANEL
                } else {
                    game::PAPER
                },
            );
            frame.stroke(
                &rect,
                Stroke::default()
                    .with_width(if selected { 2.5 } else { 1. })
                    .with_color(if selected {
                        game::COPPER_LIGHT
                    } else {
                        game::COPPER
                    }),
            );
            let dark = node.op != "source";
            let ink = if dark { game::PAPER_INK } else { game::PAPER };
            let muted = if dark { game::PAPER_MUTED } else { game::MUTED };
            let kind = match node.op.as_str() {
                "source" => "源料",
                "export" => "导出",
                "recipe" => "配方",
                _ => "手法",
            };
            let version = node
                .revision
                .as_ref()
                .and_then(|v| v["version"].as_u64())
                .map(|v| format!(" · v{v}"))
                .unwrap_or_default();
            label(
                &mut frame,
                format!("{kind}{version}"),
                x + 14.,
                y + 12.,
                12.,
                muted,
                false,
            );
            label(
                &mut frame,
                truncate(node.name(), 8),
                x + 14.,
                y + 36.,
                21.,
                ink,
                true,
            );
            label(
                &mut frame,
                truncate(&node.id, 26),
                x + 14.,
                y + 72.,
                12.,
                muted,
                false,
            );
            let state = self
                .atelier
                .run
                .as_ref()
                .and_then(|r| {
                    r.events
                        .iter()
                        .rev()
                        .find(|e| e["node_id"].as_str() == Some(&node.id))
                })
                .and_then(|e| e["status"].as_str())
                .unwrap_or("已装配");
            label(
                &mut frame,
                format!(
                    "{} · {}",
                    if node.op == "source" {
                        "任务输入"
                    } else {
                        "固定版本"
                    },
                    state
                ),
                x + 14.,
                y + 113.,
                12.,
                ink,
                false,
            );
            if node.op != "source" {
                frame.fill(
                    &Path::circle(Point::new(x, y + CARD.height / 2.), 4.),
                    game::COPPER_LIGHT,
                );
            }
            frame.fill(
                &Path::circle(Point::new(x + CARD.width, y + CARD.height / 2.), 4.),
                game::COPPER_LIGHT,
            );
        }
        if nodes.len() == 1 {
            label(
                &mut frame,
                "从藏经阁取用已保存版本",
                330.,
                104.,
                20.,
                game::PAPER,
                true,
            );
            label(
                &mut frame,
                "点击加入，再用输入依赖连接主线与支线。",
                330.,
                142.,
                13.,
                game::MUTED,
                false,
            );
        }
        vec![frame.into_geometry()]
    }
}
