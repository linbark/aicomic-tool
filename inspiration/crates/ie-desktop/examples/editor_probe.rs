//! Isolated editor rendering diagnosis. No database, model, or production modules.
use iced::advanced::text::{self as native_text, Editor as _};
use iced::widget::{column, container, text, text_editor};
use iced::{Element, Fill, Font, Pixels, Size, Subscription, Task, window};
use std::{path::PathBuf, time::Duration};
const SAMPLE: &str = "ABC 123 中文 · persistent Content";
#[derive(Debug, Clone)]
enum Message {
    Tick,
    Window(window::Id),
    Edit(text_editor::Action),
    Shot(window::Screenshot),
    Saved(Result<PathBuf, String>),
}
struct Probe {
    content: text_editor::Content,
    ticks: u32,
    id: Option<window::Id>,
    out: PathBuf,
    capturing: bool,
}
impl Probe {
    fn new() -> (Self, Task<Message>) {
        type NativeEditor = <iced::Renderer as native_text::Renderer>::Editor;
        let mut e = NativeEditor::with_text(SAMPLE);
        let mut highlighter = native_text::highlighter::PlainText;
        e.update(
            Size::new(560., 156.),
            Font::with_name("PingFang SC"),
            Pixels(16.),
            native_text::LineHeight::Relative(1.3),
            native_text::Wrapping::Word,
            &mut highlighter,
        );
        let weak = e.downgrade();
        e.update(
            Size::new(560., 156.),
            Font::with_name("PingFang SC"),
            Pixels(16.),
            native_text::LineHeight::Relative(1.3),
            native_text::Wrapping::Word,
            &mut highlighter,
        );
        eprintln!(
            "[PROBE-ASSERT] noop_update_invalidates_previous_draw_weak={}",
            weak.upgrade().is_none()
        );
        assert!(
            weak.upgrade().is_none(),
            "Iced 0.14 editor update replaces Arc even for unchanged layout; screenshots must redraw before consuming old primitive Weak"
        );
        let b = e.buffer();
        eprintln!(
            "[PROBE-headless] size={:?} metrics={:?} scroll={:?} runs={} glyphs={}",
            b.size(),
            b.metrics(),
            b.scroll(),
            b.layout_runs().count(),
            b.layout_runs().map(|r| r.glyphs.len()).sum::<usize>()
        );
        for run in b.layout_runs() {
            eprintln!(
                "[PROBE-run] glyphs={} top={} y={} lineh={}",
                run.glyphs.len(),
                run.line_top,
                run.line_y,
                run.line_height
            );
        }
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis();
        let out = PathBuf::from("dist/editor-probe").join(format!("probe-{stamp}"));
        std::fs::create_dir_all(&out).unwrap();
        (
            Self {
                content: text_editor::Content::with_text(SAMPLE),
                ticks: 0,
                id: None,
                out,
                capturing: false,
            },
            Task::none(),
        )
    }
    fn update(&mut self, m: Message) -> Task<Message> {
        match m {
            Message::Window(id) => self.id = Some(id),
            Message::Edit(a) => self.content.perform(a),
            Message::Tick => {
                self.ticks += 1;
                eprintln!("[PROBE-ui] tick={} content={:?}", self.ticks, self.content);
                if self.ticks == 8 && !std::env::args().any(|a| a == "--freeze-layout") {
                    self.content
                        .perform(text_editor::Action::Edit(text_editor::Edit::Insert('A')));
                }
                if matches!(self.ticks, 4 | 16 | 40 | 80)
                    && let Some(id) = self.id
                {
                    if std::env::args().any(|a| a == "--sequence") {
                        self.capturing = true;
                        return Task::future(async {
                            tokio::time::sleep(Duration::from_millis(150)).await;
                        })
                        .then(move |_| window::screenshot(id).map(Message::Shot));
                    }
                    return window::screenshot(id).map(Message::Shot);
                }
                if self.ticks
                    >= if std::env::args().any(|a| a == "--freeze-layout") {
                        24
                    } else {
                        90
                    }
                {
                    return iced::exit();
                }
            }
            Message::Shot(shot) => {
                let path = self.out.join(format!("tick-{}.png", self.ticks));
                return Task::perform(
                    async move {
                        use image::ImageEncoder;
                        let file = std::fs::OpenOptions::new()
                            .write(true)
                            .create_new(true)
                            .open(&path)
                            .map_err(|e| e.to_string())?;
                        image::codecs::png::PngEncoder::new(file)
                            .write_image(
                                &shot.rgba,
                                shot.size.width,
                                shot.size.height,
                                image::ExtendedColorType::Rgba8,
                            )
                            .map_err(|e| e.to_string())?;
                        Ok(path)
                    },
                    Message::Saved,
                );
            }
            Message::Saved(result) => {
                eprintln!("[PROBE-screenshot] {result:?}");
                if std::env::args().any(|a| a == "--sequence") {
                    return iced::exit();
                }
            }
        }
        Task::none()
    }
    fn view(&self) -> Element<'_, Message> {
        container(trace(
            column![
                text(SAMPLE).size(16),
                text_editor(&self.content)
                    .font(Font::with_name("PingFang SC"))
                    .size(16)
                    .height(180)
                    .padding(12)
                    .on_action(Message::Edit)
                    .style(|_, _| text_editor::Style {
                        background: iced::Color::from_rgb8(247, 239, 223).into(),
                        border: iced::Border::default(),
                        placeholder: iced::Color::BLACK,
                        value: iced::Color::from_rgb8(41, 58, 49),
                        selection: iced::Color::from_rgb8(224, 185, 126)
                    }),
                text(format!(
                    "AFTER tick={} len={} lines={}",
                    self.ticks,
                    self.content.text().len(),
                    self.content.line_count()
                ))
                .color(iced::Color::BLACK)
            ]
            .spacing(12)
            .width(584)
            .into(),
        ))
        .padding(24)
        .width(Fill)
        .height(Fill)
        .into()
    }
    fn subscription(&self) -> Subscription<Message> {
        if self.capturing {
            return Subscription::none();
        }
        Subscription::batch([
            iced::time::every(Duration::from_millis(200)).map(|_| Message::Tick),
            window::events().filter_map(|(id, e)| {
                if matches!(
                    e,
                    window::Event::Opened { .. }
                        | window::Event::Resized(_)
                        | window::Event::CloseRequested
                ) {
                    Some(Message::Window(id))
                } else {
                    None
                }
            }),
        ])
    }
}
fn main() -> iced::Result {
    if std::env::args().any(|a| a == "--headless") {
        let _ = Probe::new();
        return Ok(());
    }
    iced::application(Probe::new, Probe::update, Probe::view)
        .title("Isolated native editor probe")
        .window(window::Settings {
            size: Size::new(650., 360.),
            ..Default::default()
        })
        .default_font(Font::with_name("PingFang SC"))
        .subscription(Probe::subscription)
        .run()
}

#[derive(Default)]
struct LayoutCache(Option<iced::advanced::layout::Node>);
struct Trace<'a> {
    inner: Element<'a, Message>,
}
fn trace(inner: Element<'_, Message>) -> Element<'_, Message> {
    Element::new(Trace { inner })
}
impl iced::advanced::Widget<Message, iced::Theme, iced::Renderer> for Trace<'_> {
    fn tag(&self) -> iced::advanced::widget::tree::Tag {
        iced::advanced::widget::tree::Tag::of::<LayoutCache>()
    }
    fn state(&self) -> iced::advanced::widget::tree::State {
        iced::advanced::widget::tree::State::new(LayoutCache::default())
    }
    fn size(&self) -> iced::Size<iced::Length> {
        self.inner.as_widget().size()
    }
    fn children(&self) -> Vec<iced::advanced::widget::Tree> {
        vec![iced::advanced::widget::Tree::new(&self.inner)]
    }
    fn diff(&self, t: &mut iced::advanced::widget::Tree) {
        t.diff_children(std::slice::from_ref(&self.inner));
    }
    fn layout(
        &mut self,
        t: &mut iced::advanced::widget::Tree,
        r: &iced::Renderer,
        l: &iced::advanced::layout::Limits,
    ) -> iced::advanced::layout::Node {
        if std::env::args().any(|a| a == "--freeze-layout")
            && let Some(n) = &t.state.downcast_ref::<LayoutCache>().0
        {
            return n.clone();
        }
        let n = self.inner.as_widget_mut().layout(&mut t.children[0], r, l);
        t.state.downcast_mut::<LayoutCache>().0 = Some(n.clone());
        fn dump(n: &iced::advanced::layout::Node, depth: usize) {
            eprintln!("[PROBE-layout] depth={depth} bounds={:?}", n.bounds());
            for c in n.children() {
                dump(c, depth + 1);
            }
        }
        dump(&n, 0);
        n
    }
    fn draw(
        &self,
        t: &iced::advanced::widget::Tree,
        r: &mut iced::Renderer,
        th: &iced::Theme,
        st: &iced::advanced::renderer::Style,
        l: iced::advanced::Layout<'_>,
        c: iced::mouse::Cursor,
        v: &iced::Rectangle,
    ) {
        eprintln!("[PROBE-draw] bounds={:?}", l.bounds());
        self.inner
            .as_widget()
            .draw(&t.children[0], r, th, st, l, c, v);
    }
    fn update(
        &mut self,
        t: &mut iced::advanced::widget::Tree,
        e: &iced::Event,
        l: iced::advanced::Layout<'_>,
        c: iced::mouse::Cursor,
        r: &iced::Renderer,
        cb: &mut dyn iced::advanced::Clipboard,
        sh: &mut iced::advanced::Shell<'_, Message>,
        v: &iced::Rectangle,
    ) {
        self.inner
            .as_widget_mut()
            .update(&mut t.children[0], e, l, c, r, cb, sh, v);
    }
}
