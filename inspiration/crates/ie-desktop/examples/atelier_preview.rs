//! Native component preview driven through the same production Studio/update/view.
//! Read-only isolated fixture only; does not add test-only behavior to the app binary.
//! Use --demo --db <atelier_acceptance fixture> --screen workshop|treasury ...
#![allow(dead_code)]

#[cfg(not(test))]
#[path = "../src/app.rs"]
mod app;
#[cfg(not(test))]
#[path = "../src/atelier.rs"]
mod atelier;
#[cfg(not(test))]
#[path = "../src/atelier_adapter.rs"]
mod atelier_adapter;
#[cfg(not(test))]
#[path = "../src/atelier_runtime.rs"]
mod atelier_runtime;
#[cfg(not(test))]
#[path = "../src/atelier_view.rs"]
mod atelier_view;
#[cfg(not(test))]
#[path = "../src/board.rs"]
mod board;
#[cfg(not(test))]
#[path = "../src/data.rs"]
mod data;
#[cfg(not(test))]
#[path = "../src/options.rs"]
mod options;
#[cfg(not(test))]
#[path = "../src/runtime.rs"]
mod runtime;
#[cfg(not(test))]
#[path = "../src/style.rs"]
mod style;
#[cfg(not(test))]
#[path = "../src/view.rs"]
mod view;

#[cfg(not(test))]
struct Preview {
    studio: app::Studio,
    screen: options::Screen,
    prepared: bool,
}
#[cfg(not(test))]
impl Preview {
    fn new(options: options::Options) -> (Self, iced::Task<app::Message>) {
        let screen = options.screen;
        let (studio, task) = app::Studio::new(options);
        (
            Self {
                studio,
                screen,
                prepared: false,
            },
            task,
        )
    }
    fn update(&mut self, message: app::Message) -> iced::Task<app::Message> {
        let task = self.studio.update(message);
        if self.studio.loaded && self.studio.busy.is_none() && !self.prepared {
            self.prepared = true;
            if let Err(error) = self.prepare() {
                self.studio.error = true;
                self.studio.status = format!("原生验收场景未就绪：{error}");
                eprintln!("{}", self.studio.status);
            }
        }
        task
    }
    fn view(&self) -> iced::Element<'_, app::Message> {
        self.studio.view()
    }
    fn send(&mut self, message: atelier::Message) {
        // These scenario actions are only navigation/selection/explicit memory binding.
        // Never dispatch Save/Run/Discover or unlock the fixture for writing.
        let _ = self.studio.update(app::Message::Atelier(message));
    }
    fn prepare(&mut self) -> Result<(), String> {
        if !self.studio.effective_demo() || self.studio.writable {
            return Err("只能预览明确标记的只读隔离数据库".into());
        }
        let id = atelier::AssetId {
            kind: if self.screen == options::Screen::Treasury {
                atelier::Kind::Recipe
            } else {
                atelier::Kind::Formula
            },
            key: if self.screen == options::Screen::Treasury {
                "acceptance.compose"
            } else {
                "acceptance.pipeline"
            }
            .into(),
        };
        let revision = self
            .studio
            .atelier
            .assets
            .get(&id)
            .and_then(|v| v.last())
            .cloned()
            .ok_or("请先生成 atelier_acceptance 隔离 fixture")?;
        if self.screen == options::Screen::Treasury {
            self.send(atelier::Message::Tab(id.kind));
            self.send(atelier::Message::Select(id));
        } else if self.screen == options::Screen::Workshop {
            self.send(atelier::Message::Bind(revision, false));
            self.send(atelier::Message::ConfirmFormula(true));
            self.send(atelier::Message::SelectNode(1));
            if self.studio.atelier.workshop.nodes.len() != 5 || self.studio.atelier.error {
                return Err("未通过生产路径载入验收 DAG".into());
            }
            self.studio.atelier.workshop.source = iced::widget::text_editor::Content::with_text(
                "隔离本地 HTTP 验收输入：青瓷色片、纸色背景与留白。不是实际模型推理或真人偏好。",
            );
        }
        self.studio.screen = self.screen;
        eprintln!(
            "Native preview prepared via production update path; read-only=true, inference=false"
        );
        Ok(())
    }
}

#[cfg(not(test))]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let options = options::Options::parse(std::env::args().skip(1), &std::env::current_dir()?)
        .map_err(std::io::Error::other)?;
    if !options.demo || options.config.db.as_os_str().is_empty() {
        return Err("需要 --demo --db 显式隔离 fixture，不使用用户默认数据库".into());
    }
    let size = options.size;
    iced::application(
        move || Preview::new(options.clone()),
        Preview::update,
        Preview::view,
    )
    .title(|preview: &Preview| {
        format!(
            "{} · 炼丹炉 {} · 原生隔离验收",
            preview.studio.screen.title(),
            env!("CARGO_PKG_VERSION")
        )
    })
    .window(iced::window::Settings {
        size: iced::Size::new(size.0, size.1),
        min_size: Some(iced::Size::new(1040., 720.)),
        ..Default::default()
    })
    .theme(|preview: &Preview| preview.studio.theme())
    .subscription(|preview: &Preview| preview.studio.subscription())
    .exit_on_close_request(false)
    .default_font(iced::Font::with_name(if cfg!(target_os = "macos") {
        "PingFang SC"
    } else {
        "Microsoft YaHei"
    }))
    .run()?;
    Ok(())
}

#[cfg(test)]
fn main() {}
