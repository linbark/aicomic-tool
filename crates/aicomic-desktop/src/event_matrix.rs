//! Episode rows and local scene columns, with event paths across the story.
use crate::geometry::BoardView;
use crate::ui;
use iced::widget::canvas::{self, Action, Event, Frame, Geometry, Path, Stroke, Text};
use iced::{Color, Element, Length, Point, Rectangle, Renderer, Size, Theme, Vector, mouse};
use serde_json::Value;
use std::collections::{BTreeSet, HashMap};

const LEFT: f32 = 200.0;
const TOP: f32 = 100.0;
const COLUMN: f32 = 150.0;
const ROW: f32 = 88.0;
const RADIUS: f32 = 17.0;
const LEGEND_WIDTH: f32 = 180.0;

#[derive(Clone, Debug)]
pub enum MatrixMessage {
    View(BoardView),
    Select(i64),
}
pub fn matrix<'a>(
    events: &'a [Value],
    episodes: &'a [Value],
    scenes: &'a [Value],
    selected: Option<i64>,
    view: &BoardView,
) -> Element<'a, MatrixMessage> {
    iced::widget::canvas(Matrix {
        events,
        episodes,
        scenes,
        selected,
        view: view.clone(),
    })
    .width(Length::Fill)
    .height(Length::Fill)
    .into()
}
struct Matrix<'a> {
    events: &'a [Value],
    episodes: &'a [Value],
    scenes: &'a [Value],
    selected: Option<i64>,
    view: BoardView,
}
#[derive(Default)]
pub struct Interaction {
    drag: Option<Point>,
}
#[derive(Debug)]
struct EpisodeRow {
    episode_id: i64,
    order: i64,
    title: String,
    scenes: Vec<Value>,
}
#[derive(Debug)]
struct EpisodeGrid {
    rows: Vec<EpisodeRow>,
    positions: HashMap<i64, (usize, usize)>,
    max_columns: usize,
}
#[derive(Debug)]
struct EventPath {
    event_id: i64,
    name: String,
    color: Color,
    points: Vec<(usize, usize)>,
}
fn id(value: &Value) -> Option<i64> {
    value["id"].as_i64()
}
fn label<'a>(value: &'a Value, field: &str) -> &'a str {
    value[field].as_str().unwrap_or("")
}
fn episode_grid(episodes: &[Value], scenes: &[Value]) -> EpisodeGrid {
    let mut grouped: HashMap<i64, EpisodeRow> = episodes
        .iter()
        .filter_map(|episode| {
            let episode_id = id(episode)?;
            Some((
                episode_id,
                EpisodeRow {
                    episode_id,
                    order: episode["order"].as_i64().unwrap_or(episode_id),
                    title: label(episode, "title").into(),
                    scenes: vec![],
                },
            ))
        })
        .collect();
    for scene in scenes {
        if id(scene).is_none() {
            continue;
        }
        let episode_id = scene["episode_id"].as_i64().unwrap_or(0);
        grouped
            .entry(episode_id)
            .or_insert_with(|| EpisodeRow {
                episode_id,
                order: scene["episode_order"].as_i64().unwrap_or(episode_id),
                title: label(scene, "episode_title").into(),
                scenes: vec![],
            })
            .scenes
            .push(scene.clone());
    }
    let mut rows: Vec<_> = grouped.into_values().collect();
    rows.sort_by_key(|row| (row.order, row.episode_id));
    let mut positions = HashMap::new();
    let mut max_columns = 5;
    for (row_index, row) in rows.iter_mut().enumerate() {
        row.scenes.sort_by_key(|scene| {
            (
                scene["sequence_number"].as_i64().unwrap_or(0),
                id(scene).unwrap_or(0),
            )
        });
        row.scenes.dedup_by_key(|scene| id(scene));
        max_columns = max_columns.max(row.scenes.len());
        for (column, scene) in row.scenes.iter().enumerate() {
            if let Some(scene_id) = id(scene) {
                positions.insert(scene_id, (row_index, column));
            }
        }
    }
    EpisodeGrid {
        rows,
        positions,
        max_columns,
    }
}
fn event_paths(events: &[Value], grid: &EpisodeGrid) -> Vec<EventPath> {
    events
        .iter()
        .filter_map(|event| {
            let event_id = id(event)?;
            let mut points = BTreeSet::new();
            if let Some(nodes) = event["nodes"].as_array() {
                for node in nodes {
                    if node["target_type"] != "scene"
                        || node["event_id"]
                            .as_i64()
                            .is_some_and(|owner| owner != event_id)
                    {
                        continue;
                    }
                    if let Some(position) = node["target_id"]
                        .as_i64()
                        .and_then(|scene_id| grid.positions.get(&scene_id))
                    {
                        points.insert(*position);
                    }
                }
            }
            Some(EventPath {
                event_id,
                name: label(event, "name").into(),
                color: parse_color(label(event, "color")),
                points: points.into_iter().collect(),
            })
        })
        .collect()
}
fn center(row: usize, column: usize) -> Point {
    Point::new(
        LEFT + (column as f32 + 0.5) * COLUMN,
        TOP + (row as f32 + 0.5) * ROW,
    )
}
fn event_offset(index: usize, count: usize) -> f32 {
    ((index as f32 - count.saturating_sub(1) as f32 * 0.5) * 4.0).clamp(-8.0, 8.0)
}
fn curve_points(from: Point, to: Point, offset: f32) -> [Point; 4] {
    let from = from + Vector::new(0.0, offset);
    let to = to + Vector::new(0.0, offset);
    if (to.y - from.y).abs() > 1.0 {
        let middle = (from.y + to.y) * 0.5;
        [
            from,
            Point::new(from.x, middle),
            Point::new(to.x, middle),
            to,
        ]
    } else {
        let span = (to.x - from.x) * 0.36;
        [
            from,
            Point::new(from.x + span, from.y - 18.0),
            Point::new(to.x - span, to.y + 18.0),
            to,
        ]
    }
}
fn curve_at(points: [Point; 4], t: f32) -> Point {
    let u = 1.0 - t;
    Point::new(
        u.powi(3) * points[0].x
            + 3.0 * u.powi(2) * t * points[1].x
            + 3.0 * u * t.powi(2) * points[2].x
            + t.powi(3) * points[3].x,
        u.powi(3) * points[0].y
            + 3.0 * u.powi(2) * t * points[1].y
            + 3.0 * u * t.powi(2) * points[2].y
            + t.powi(3) * points[3].y,
    )
}
fn line_distance(point: Point, from: Point, to: Point) -> f32 {
    let delta = to - from;
    let length = delta.x.powi(2) + delta.y.powi(2);
    let t = if length > 0.0 {
        (((point.x - from.x) * delta.x + (point.y - from.y) * delta.y) / length).clamp(0.0, 1.0)
    } else {
        0.0
    };
    let projected = from + delta * t;
    ((point.x - projected.x).powi(2) + (point.y - projected.y).powi(2)).sqrt()
}
fn legend(index: usize) -> Rectangle {
    Rectangle::new(
        Point::new(12.0 + index as f32 * LEGEND_WIDTH, 8.0),
        Size::new(LEGEND_WIDTH - 8.0, 36.0),
    )
}
fn hit_grid_event(paths: &[EventPath], selected: Option<i64>, world: [f32; 2]) -> Option<i64> {
    let point = Point::new(world[0], world[1]);
    if let Some(event) = paths
        .iter()
        .enumerate()
        .find_map(|(index, event)| legend(index).contains(point).then_some(event.event_id))
    {
        return Some(event);
    }
    let mut ordered: Vec<_> = paths.iter().enumerate().collect();
    ordered.sort_by_key(|(_, event)| Some(event.event_id) != selected);
    for (index, event) in ordered {
        if event.points.iter().any(|(row, column)| {
            let node = center(*row, *column);
            (point.x - node.x).powi(2) + (point.y - node.y).powi(2) <= (RADIUS + 5.0).powi(2)
        }) {
            return Some(event.event_id);
        }
        for pair in event.points.windows(2) {
            let curve = curve_points(
                center(pair[0].0, pair[0].1),
                center(pair[1].0, pair[1].1),
                event_offset(index, paths.len()),
            );
            let mut previous = curve[0];
            for sample in 1..=32 {
                let next = curve_at(curve, sample as f32 / 32.0);
                if line_distance(point, previous, next) <= 6.0 {
                    return Some(event.event_id);
                }
                previous = next;
            }
        }
    }
    None
}
fn zoom_at(view: &mut BoardView, factor: f32, cursor: [f32; 2]) {
    if !factor.is_finite() || factor <= 0.0 || !view.zoom.is_finite() || view.zoom <= 0.0 {
        return;
    }
    let target = (view.zoom * factor).clamp(0.5, 2.0);
    view.zoom_at(target / view.zoom, cursor);
}
fn parse_color(value: &str) -> Color {
    let hex = value.trim().trim_start_matches('#');
    let hex = if hex.len() == 3 {
        hex.chars().flat_map(|c| [c, c]).collect::<String>()
    } else {
        hex.to_owned()
    };
    if hex.len() == 6
        && let Ok(value) = u32::from_str_radix(&hex, 16)
    {
        return Color::from_rgb8((value >> 16) as u8, (value >> 8) as u8, value as u8);
    }
    ui::INDIGO
}
fn alpha(mut color: Color, opacity: f32) -> Color {
    color.a = opacity;
    color
}
fn text(
    frame: &mut Frame,
    content: impl Into<String>,
    position: Point,
    size: f32,
    color: Color,
    max_width: f32,
) {
    frame.fill_text(Text {
        content: content.into(),
        position,
        size: size.into(),
        color,
        max_width,
        font: iced::Font::with_name(if cfg!(target_os = "macos") {
            "PingFang SC"
        } else {
            "Microsoft YaHei"
        }),
        ..Text::default()
    });
}

impl canvas::Program<MatrixMessage> for Matrix<'_> {
    type State = Interaction;
    fn update(
        &self,
        state: &mut Interaction,
        event: &Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Option<Action<MatrixMessage>> {
        match event {
            Event::Mouse(mouse::Event::ButtonReleased(
                mouse::Button::Left | mouse::Button::Middle,
            )) => state
                .drag
                .take()
                .map(|_| Action::request_redraw().and_capture()),
            Event::Mouse(mouse::Event::ButtonPressed(
                button @ (mouse::Button::Left | mouse::Button::Middle),
            )) => {
                let cursor = cursor.position_in(bounds)?;
                if *button == mouse::Button::Left
                    && let Some(event_id) = hit_grid_event(
                        &event_paths(self.events, &episode_grid(self.episodes, self.scenes)),
                        self.selected,
                        self.view.world([cursor.x, cursor.y]),
                    )
                {
                    state.drag = None;
                    return Some(Action::publish(MatrixMessage::Select(event_id)).and_capture());
                }
                state.drag = Some(cursor);
                Some(Action::request_redraw().and_capture())
            }
            Event::Mouse(mouse::Event::CursorMoved { .. }) => {
                let previous = state.drag?;
                let current = cursor.position_in(bounds)?;
                let delta = current - previous;
                let mut view = self.view.clone();
                view.pan[0] += delta.x;
                view.pan[1] += delta.y;
                state.drag = Some(current);
                Some(Action::publish(MatrixMessage::View(view)).and_capture())
            }
            Event::Mouse(mouse::Event::WheelScrolled { delta }) => {
                let cursor = cursor.position_in(bounds)?;
                let amount = match delta {
                    mouse::ScrollDelta::Lines { y, .. } => *y * 0.12,
                    mouse::ScrollDelta::Pixels { y, .. } => *y * 0.002,
                };
                let mut view = self.view.clone();
                zoom_at(&mut view, amount.exp(), [cursor.x, cursor.y]);
                Some(Action::publish(MatrixMessage::View(view)).and_capture())
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
        cursor: mouse::Cursor,
    ) -> Vec<Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        frame.fill_rectangle(Point::ORIGIN, bounds.size(), ui::INK);
        let grid = episode_grid(self.episodes, self.scenes);
        let paths = event_paths(self.events, &grid);
        let selected = self
            .selected
            .filter(|id| paths.iter().any(|path| path.event_id == *id));
        let hovered = cursor
            .position_in(bounds)
            .and_then(|p| hit_grid_event(&paths, selected, self.view.world([p.x, p.y])));
        let width =
            (LEFT + COLUMN * grid.max_columns as f32).max(12.0 + paths.len() as f32 * LEGEND_WIDTH);
        let height = TOP + ROW * grid.rows.len().max(1) as f32;
        frame.with_save(|frame| {
            frame.translate(Vector::new(self.view.pan[0], self.view.pan[1]));
            frame.scale(self.view.zoom);
            frame.fill_rectangle(Point::ORIGIN, Size::new(width, TOP), ui::SURFACE);
            for (index, path) in paths.iter().enumerate() {
                let bounds = legend(index);
                let active = selected == Some(path.event_id) || hovered == Some(path.event_id);
                frame.fill_rectangle(
                    bounds.position(),
                    bounds.size(),
                    alpha(path.color, if active { 0.23 } else { 0.08 }),
                );
                frame.fill(
                    &Path::circle(bounds.position() + Vector::new(13.0, 18.0), 4.0),
                    path.color,
                );
                text(
                    frame,
                    path.name.chars().take(11).collect::<String>(),
                    bounds.position() + Vector::new(25.0, 9.0),
                    12.0,
                    if active {
                        Color::WHITE
                    } else {
                        ui::TEXT_SECONDARY
                    },
                    LEGEND_WIDTH - 42.0,
                );
            }
            text(
                frame,
                "分集 / 场景",
                Point::new(18.0, 66.0),
                15.0,
                ui::TEXT,
                LEFT - 32.0,
            );
            for column in 0..grid.max_columns {
                let x = LEFT + column as f32 * COLUMN;
                text(
                    frame,
                    format!("scene {}", column + 1),
                    Point::new(x + 40.0, 67.0),
                    12.0,
                    ui::TEXT_MUTED,
                    COLUMN - 22.0,
                );
                frame.stroke(
                    &Path::line(Point::new(x, TOP), Point::new(x, height)),
                    Stroke::default().with_color(ui::LINE).with_width(1.0),
                );
            }
            for (row_index, row) in grid.rows.iter().enumerate() {
                let y = TOP + row_index as f32 * ROW;
                if row_index % 2 == 0 {
                    frame.fill_rectangle(
                        Point::new(0.0, y),
                        Size::new(width, ROW),
                        Color::from_rgba8(255, 255, 255, 0.018),
                    );
                }
                text(
                    frame,
                    format!("EP{:02}", row.order + 1),
                    Point::new(20.0, y + 17.0),
                    15.0,
                    ui::TEXT,
                    LEFT - 30.0,
                );
                text(
                    frame,
                    row.title.chars().take(11).collect::<String>(),
                    Point::new(20.0, y + 44.0),
                    11.0,
                    ui::TEXT_MUTED,
                    LEFT - 30.0,
                );
                if row.scenes.is_empty() {
                    text(
                        frame,
                        "暂无场景",
                        Point::new(LEFT + 24.0, y + 30.0),
                        12.0,
                        ui::TEXT_MUTED,
                        COLUMN - 24.0,
                    );
                }
                frame.stroke(
                    &Path::line(Point::new(0.0, y + ROW), Point::new(width, y + ROW)),
                    Stroke::default().with_color(ui::LINE).with_width(1.0),
                );
            }
            let mut ordered: Vec<_> = paths.iter().enumerate().collect();
            ordered.sort_by_key(|(_, path)| Some(path.event_id) == selected);
            for (index, path) in ordered {
                let opacity = if selected == Some(path.event_id) {
                    1.0
                } else if selected.is_some() {
                    0.27
                } else {
                    0.66
                };
                for pair in path.points.windows(2) {
                    let points = curve_points(
                        center(pair[0].0, pair[0].1),
                        center(pair[1].0, pair[1].1),
                        event_offset(index, paths.len()),
                    );
                    frame.stroke(
                        &Path::new(|builder| {
                            builder.move_to(points[0]);
                            builder.bezier_curve_to(points[1], points[2], points[3]);
                        }),
                        Stroke::default()
                            .with_color(alpha(path.color, opacity))
                            .with_width(if selected == Some(path.event_id) {
                                3.0
                            } else {
                                2.0
                            }),
                    );
                }
            }
            for (row_index, row) in grid.rows.iter().enumerate() {
                for (column, scene) in row.scenes.iter().enumerate() {
                    let node = center(row_index, column);
                    let linked: Vec<_> = paths
                        .iter()
                        .filter(|path| path.points.contains(&(row_index, column)))
                        .collect();
                    let primary = linked
                        .iter()
                        .copied()
                        .find(|path| Some(path.event_id) == selected)
                        .or_else(|| linked.first().copied());
                    let color = primary.map(|path| path.color).unwrap_or(ui::INDIGO);
                    let active = primary.is_some_and(|path| {
                        Some(path.event_id) == selected || Some(path.event_id) == hovered
                    });
                    if active {
                        frame.fill(&Path::circle(node, RADIUS + 6.0), alpha(color, 0.13));
                    }
                    frame.fill(&Path::circle(node, RADIUS), ui::SURFACE_SOFT);
                    frame.stroke(
                        &Path::circle(node, RADIUS),
                        Stroke::default().with_color(color).with_width(if active {
                            3.0
                        } else {
                            1.5
                        }),
                    );
                    text(
                        frame,
                        (column + 1).to_string(),
                        node + Vector::new(-4.0, -8.0),
                        12.0,
                        if primary.is_some() {
                            Color::WHITE
                        } else {
                            ui::TEXT_MUTED
                        },
                        24.0,
                    );
                    for (index, path) in linked.iter().take(4).enumerate() {
                        frame.fill(
                            &Path::circle(node + Vector::new(-9.0 + index as f32 * 6.0, 20.0), 2.3),
                            path.color,
                        );
                    }
                    text(
                        frame,
                        label(scene, "title").chars().take(10).collect::<String>(),
                        node + Vector::new(-54.0, 28.0),
                        10.5,
                        ui::TEXT_SECONDARY,
                        COLUMN - 24.0,
                    );
                }
            }
            if grid.rows.is_empty() {
                text(
                    frame,
                    "创建分集并拆分场景后，事件将在对应场景间连线。",
                    Point::new(24.0, TOP + 24.0),
                    14.0,
                    ui::TEXT_MUTED,
                    520.0,
                );
            }
        });
        frame.fill_rectangle(
            Point::new(0.0, bounds.height - 34.0),
            Size::new(bounds.width, 34.0),
            Color::from_rgba8(12, 20, 35, 0.95),
        );
        let selected_name = paths
            .iter()
            .find(|path| Some(path.event_id) == hovered.or(selected))
            .map(|path| format!(" · {}", path.name))
            .unwrap_or_default();
        text(
            &mut frame,
            format!(
                "{}% · 分集行 × 集内场景列 · 滚轮缩放 · 拖动空白平移{selected_name}",
                (self.view.zoom * 100.0).round()
            ),
            Point::new(14.0, bounds.height - 26.0),
            11.0,
            ui::TEXT_MUTED,
            (bounds.width - 28.0).max(0.0),
        );
        vec![frame.into_geometry()]
    }
    fn mouse_interaction(
        &self,
        state: &Interaction,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> mouse::Interaction {
        if state.drag.is_some() {
            return mouse::Interaction::Grabbing;
        }
        if cursor
            .position_in(bounds)
            .and_then(|p| {
                hit_grid_event(
                    &event_paths(self.events, &episode_grid(self.episodes, self.scenes)),
                    self.selected,
                    self.view.world([p.x, p.y]),
                )
            })
            .is_some()
        {
            mouse::Interaction::Pointer
        } else {
            mouse::Interaction::default()
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn grid_preserves_episode_order_empty_rows_and_sorted_local_scene_columns() {
        let episodes = vec![
            json!({"id":20,"order":2,"title":"空集"}),
            json!({"id":30,"order":3}),
            json!({"id":10,"order":1}),
        ];
        let scenes = vec![
            json!({"id":301,"episode_id":30,"sequence_number":2}),
            json!({"id":100,"episode_id":10,"sequence_number":4}),
            json!({"id":300,"episode_id":30,"sequence_number":1}),
            json!({"id":101,"episode_id":10,"sequence_number":1}),
        ];
        let grid = episode_grid(&episodes, &scenes);
        assert_eq!(
            grid.rows.iter().map(|r| r.episode_id).collect::<Vec<_>>(),
            vec![10, 20, 30]
        );
        assert!(grid.rows[1].scenes.is_empty());
        assert_eq!(grid.positions.get(&101), Some(&(0, 0)));
        assert_eq!(grid.positions.get(&100), Some(&(0, 1)));
        assert_eq!(grid.positions.get(&300), Some(&(2, 0)));
        assert_eq!(grid.positions.get(&301), Some(&(2, 1)));
    }
    #[test]
    fn event_path_crosses_episode_rows_in_story_order_without_duplicate_nodes() {
        let episodes = vec![json!({"id":1,"order":1}), json!({"id":2,"order":2})];
        let scenes = vec![
            json!({"id":11,"episode_id":1,"sequence_number":1}),
            json!({"id":12,"episode_id":1,"sequence_number":2}),
            json!({"id":21,"episode_id":2,"sequence_number":1}),
        ];
        let events = vec![
            json!({"id":7,"nodes":[{"target_type":"scene","target_id":21},{"target_type":"scene","target_id":12},{"target_type":"scene","target_id":11},{"target_type":"scene","target_id":21},{"target_type":"shot","target_id":11},{"target_type":"scene","target_id":999}]}),
        ];
        let paths = event_paths(&events, &episode_grid(&episodes, &scenes));
        assert_eq!(paths.len(), 1);
        assert_eq!(paths[0].points, vec![(0, 0), (0, 1), (1, 0)]);
    }
    #[test]
    fn grid_selection_hits_event_legend_and_nodes_but_not_empty_scene_cells() {
        let episodes = vec![json!({"id":1,"order":1})];
        let scenes = vec![
            json!({"id":11,"episode_id":1,"sequence_number":1}),
            json!({"id":12,"episode_id":1,"sequence_number":2}),
        ];
        let events = vec![
            json!({"id":7,"name":"事件甲","nodes":[{"target_type":"scene","target_id":11}]}),
            json!({"id":8,"name":"事件乙","nodes":[{"target_type":"scene","target_id":11}]}),
        ];
        let paths = event_paths(&events, &episode_grid(&episodes, &scenes));
        assert_eq!(hit_grid_event(&paths, None, [30.0, 20.0]), Some(7));
        assert_eq!(hit_grid_event(&paths, None, [210.0, 20.0]), Some(8));
        assert_eq!(hit_grid_event(&paths, Some(8), [275.0, 144.0]), Some(8));
        assert_eq!(hit_grid_event(&paths, None, [425.0, 144.0]), None);
    }

    #[test]
    fn zoom_is_limited_to_half_to_double_and_keeps_the_cursor_world_position() {
        let mut view = BoardView::default();
        let cursor = [420.0, 330.0];
        let initial = view.world(cursor);
        zoom_at(&mut view, 100.0, cursor);
        assert_eq!(view.zoom, 2.0);
        assert_eq!(view.world(cursor), initial);
        zoom_at(&mut view, 0.001, cursor);
        assert_eq!(view.zoom, 0.5);
        assert_eq!(view.world(cursor), initial);
    }
}
