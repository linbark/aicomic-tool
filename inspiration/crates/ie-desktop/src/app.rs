use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::time::Duration;

use iced::{Subscription, Task, keyboard, window};
use ie::studio::{StudioAction, StudioConfig};

use crate::data::{self, Snapshot};
use crate::options::{Options, Screen};
use crate::runtime::{self, Asset, Outcome};

pub const PAGE_SIZE: usize = 24;
#[derive(Clone, Debug)]
pub enum Dialog {
    Open,
    AssetRoot,
    Copy,
    New,
    Export(i64),
    Recipe,
}
#[derive(Clone, Debug)]
pub enum Message {
    Navigate(Screen),
    Search(String),
    ClearSearch,
    Refresh,
    ToggleInspector,
    ToggleList,
    ToggleSidebar,
    SelectWork(i64),
    SelectPill(i64),
    TogglePillRaw,
    SelectExperiment(i64),
    Page(usize),
    Ratio(f64),
    SelectRecipe(Option<usize>),
    Refine,
    Brew,
    BrewRecipe,
    Export,
    Dialog(Dialog),
    DialogDone {
        generation: u64,
        config: StudioConfig,
        kind: Dialog,
        result: Result<Option<PathBuf>, String>,
    },
    Loaded {
        generation: u64,
        config: StudioConfig,
        result: Result<Box<Outcome>, String>,
    },
    Images {
        generation: u64,
        result: Result<HashMap<PathBuf, Asset>, String>,
    },
    Person(String),
    Personal(bool),
    Preference(String),
    ConfirmPreference,
    Cancel,
    Window(window::Id, window::Event),
    Frame,
    CaptureTick,
    Captured(window::Screenshot),
    CaptureSaved(Result<PathBuf, String>),
    FocusSearch,
    FocusNext(bool),
    Key(keyboard::Key, keyboard::Modifiers),
}
#[derive(Clone, Debug)]
pub struct PendingPreference {
    pub action: StudioAction,
    pub summary: String,
}

pub struct Studio {
    pub options: Options,
    pub config: StudioConfig,
    pub data: Snapshot,
    pub screen: Screen,
    pub query: String,
    pub selection: Option<i64>,
    pub pill: Option<i64>,
    pub show_raw_pill: bool,
    pub experiment: Option<i64>,
    pub recipe: Option<usize>,
    pub custom_recipe: Option<PathBuf>,
    pub palette_ratio: f64,
    pub person: String,
    pub personal: bool,
    pub confirmation: Option<PendingPreference>,
    pub inspector: bool,
    pub sidebar: bool,
    pub list: bool,
    pub page: usize,
    pub status: String,
    pub error: bool,
    pub busy: Option<String>,
    pub writable: bool,
    pub loaded: bool,
    pub generation: u64,
    pub assets: HashMap<PathBuf, Asset>,
    pub pending_assets: HashSet<PathBuf>,
    pub window: Option<window::Id>,
    pub width: f32,
    pub capture_ticks: u8,
    pub rendered_frames: u8,
    pub capturing: bool,
}
impl Studio {
    pub fn new(options: Options) -> (Self, Task<Message>) {
        let config = options.config.clone();
        let mut app = Self {
            screen: options.screen,
            inspector: options.size.0 >= 1180.,
            width: options.size.0,
            options,
            config,
            data: Snapshot::default(),
            query: String::new(),
            selection: None,
            pill: None,
            show_raw_pill: false,
            experiment: None,
            recipe: None,
            custom_recipe: None,
            palette_ratio: 0.6,
            person: String::new(),
            personal: false,
            confirmation: None,
            sidebar: true,
            list: false,
            page: 0,
            status: "正在读取本地素材…".into(),
            error: false,
            busy: None,
            writable: false,
            loaded: false,
            generation: 0,
            assets: HashMap::new(),
            pending_assets: HashSet::new(),
            window: None,
            capture_ticks: 0,
            rendered_frames: 0,
            capturing: false,
        };
        let task = app.reload();
        (app, task)
    }
    pub fn effective_demo(&self) -> bool {
        self.options.demo || self.data.demo
    }
    pub fn theme(&self) -> Option<iced::Theme> {
        match self.options.appearance.as_str() {
            "light" => Some(iced::Theme::Light),
            "dark" => Some(iced::Theme::Dark),
            _ => None,
        }
    }
    pub fn can_write(&self) -> bool {
        self.writable
            && !self.data.needs_migration
            && self.data.schema.is_some()
            && self.busy.is_none()
            && self.confirmation.is_none()
    }
    pub fn work(&self) -> Option<&serde_json::Value> {
        self.data.work(self.selection)
    }
    pub fn active_experiment(&self) -> Option<&serde_json::Value> {
        let query = if self.screen == Screen::Preferences {
            self.query.as_str()
        } else {
            ""
        };
        self.data.visible_experiment(self.experiment, query)
    }
    fn sync_experiment_selection(&mut self) {
        let visible = self.active_experiment().map(data::id);
        if self.experiment != visible {
            self.confirmation = None;
        }
        self.experiment = visible;
    }
    pub fn can_rate(&self) -> bool {
        self.can_write()
            && self.ready_experiment()
            && (self.effective_demo() || !self.person.trim().is_empty())
    }
    pub fn ready_experiment(&self) -> bool {
        self.active_experiment().is_some_and(|e| {
            data::flag(e, "ready")
                && ["A", "B"].iter().all(|key| {
                    data::path(e, key)
                        .and_then(|p| self.assets.get(&p))
                        .is_some_and(|a| a.handle.is_some())
                })
        })
    }
    fn begin(&mut self, label: &str) -> (u64, StudioConfig) {
        self.generation = self.generation.wrapping_add(1);
        self.pending_assets.clear();
        self.busy = Some(label.into());
        self.error = false;
        self.status = label.into();
        self.capture_ticks = 0;
        self.rendered_frames = 0;
        (self.generation, self.config.clone())
    }
    fn reload(&mut self) -> Task<Message> {
        if self.busy.is_some() {
            return Task::none();
        }
        let (generation, config) = self.begin("正在读取本地数据…");
        let task_config = config.clone();
        Task::perform(
            async move {
                tokio::task::spawn_blocking(move || runtime::load(task_config).map(Box::new))
                    .await
                    .map_err(|e| format!("读取任务异常：{e}"))?
            },
            move |result| Message::Loaded {
                generation,
                config: config.clone(),
                result,
            },
        )
    }
    fn action(&mut self, action: StudioAction, label: &str, creates: bool) -> Task<Message> {
        if self.busy.is_some() {
            return Task::none();
        }
        if !creates && !self.can_write() {
            self.status = "当前数据库只读。请先建立工作副本或新库，再执行写入。".into();
            return Task::none();
        }
        let (generation, config) = self.begin(label);
        let task_config = config.clone();
        Task::perform(
            async move {
                tokio::task::spawn_blocking(move || runtime::run(task_config, action).map(Box::new))
                    .await
                    .map_err(|e| format!("后台任务异常：{e}"))?
            },
            move |result| Message::Loaded {
                generation,
                config: config.clone(),
                result,
            },
        )
    }
    fn choose(&mut self, kind: Dialog) -> Task<Message> {
        if self.busy.is_some() || self.confirmation.is_some() {
            return Task::none();
        }
        let generation = self.generation;
        let config = self.config.clone();
        self.busy = Some("等待系统文件对话框…".into());
        let for_task = kind.clone();
        let asset_root = self.config.root.clone();
        Task::perform(
            async move {
                tokio::task::spawn_blocking(move || {
                    let dialog = rfd::FileDialog::new();
                    match for_task {
                        Dialog::Open => dialog
                            .set_title("只读打开素材数据库")
                            .add_filter("SQLite 数据库", &["db", "sqlite", "sqlite3"])
                            .pick_file(),
                        Dialog::AssetRoot => dialog
                            .set_title("选择素材目录 · 不会修改数据库")
                            .set_directory(asset_root)
                            .pick_folder(),
                        Dialog::Copy => dialog
                            .set_title("建立工作副本 · 不会修改原数据库")
                            .set_file_name("炼丹炉-工作副本.db")
                            .add_filter("SQLite 数据库", &["db"])
                            .save_file(),
                        Dialog::New => dialog
                            .set_title("明确建立一个新的空数据库")
                            .set_file_name("炼丹炉-新库.db")
                            .add_filter("SQLite 数据库", &["db"])
                            .save_file(),
                        Dialog::Export(_) => dialog
                            .set_title("选择导出父目录 · 将新建独立 Brief 文件夹")
                            .pick_folder(),
                        Dialog::Recipe => dialog
                            .set_title("选择本地配方 · 不会发起模型请求")
                            .add_filter("JSON 配方", &["json"])
                            .pick_file(),
                    }
                })
                .await
                .map_err(|e| format!("文件对话框未完成：{e}"))
            },
            move |result| Message::DialogDone {
                generation,
                config: config.clone(),
                kind: kind.clone(),
                result,
            },
        )
    }
    pub fn visible_paths(&self) -> Vec<PathBuf> {
        let mut paths = Vec::new();
        if self.screen == Screen::Library {
            for work in self
                .data
                .filtered_works(&self.query)
                .into_iter()
                .skip(self.page * PAGE_SIZE)
                .take(PAGE_SIZE)
            {
                if let Some(path) = data::path(work, "image_path") {
                    paths.push(path);
                }
            }
        }
        if let Some(path) = self.work().and_then(|w| data::path(w, "image_path")) {
            paths.push(path);
        }
        if self.screen == Screen::Preferences
            && let Some(experiment) = self.active_experiment()
        {
            for key in ["A", "B"] {
                if let Some(path) = data::path(experiment, key) {
                    paths.push(path);
                }
            }
        }
        paths.sort();
        paths.dedup();
        paths
    }
    fn load_visible_images(&mut self) -> Task<Message> {
        let paths: Vec<_> = self
            .visible_paths()
            .into_iter()
            .filter(|p| !self.assets.contains_key(p) && !self.pending_assets.contains(p))
            .collect();
        if paths.is_empty() {
            return Task::none();
        }
        for path in &paths {
            self.pending_assets.insert(path.clone());
        }
        let generation = self.generation;
        self.capture_ticks = 0;
        self.rendered_frames = 0;
        Task::perform(
            async move {
                tokio::task::spawn_blocking(move || runtime::load_images(paths))
                    .await
                    .map_err(|e| format!("图片任务异常：{e}"))
            },
            move |result| Message::Images { generation, result },
        )
    }
    pub fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Navigate(screen) => {
                self.screen = screen;
                self.query.clear();
                self.page = 0;
                self.confirmation = None;
                self.sync_experiment_selection();
                self.capture_ticks = 0;
                return self.load_visible_images();
            }
            Message::Search(query) => {
                self.query = query;
                self.page = 0;
                self.confirmation = None;
                self.sync_experiment_selection();
                return self.load_visible_images();
            }
            Message::ClearSearch => {
                self.query.clear();
                self.page = 0;
                self.confirmation = None;
                self.sync_experiment_selection();
                return self.load_visible_images();
            }
            Message::Refresh => {
                if self.busy.is_some() {
                    return Task::none();
                }
                // Explicit refresh also re-probes files repaired or replaced at the same path.
                self.assets.clear();
                return self.reload();
            }
            Message::ToggleInspector => self.inspector = !self.inspector,
            Message::ToggleSidebar => self.sidebar = !self.sidebar,
            Message::ToggleList => self.list = !self.list,
            Message::SelectWork(id) => {
                self.selection = Some(id);
                self.inspector = true;
                return self.load_visible_images();
            }
            Message::SelectPill(id) => {
                if self.pill != Some(id) {
                    self.show_raw_pill = false;
                }
                self.pill = Some(id);
                self.inspector = true;
            }
            Message::TogglePillRaw => self.show_raw_pill = !self.show_raw_pill,
            Message::SelectExperiment(id) => {
                self.experiment = Some(id);
                self.confirmation = None;
                self.sync_experiment_selection();
                return self.load_visible_images();
            }
            Message::Page(page) => {
                let max = self
                    .data
                    .filtered_works(&self.query)
                    .len()
                    .saturating_sub(1)
                    / PAGE_SIZE;
                self.page = page.min(max);
                return self.load_visible_images();
            }
            Message::Ratio(ratio) => {
                if self.busy.is_none() {
                    self.palette_ratio = ratio.clamp(0., 1.);
                }
            }
            Message::SelectRecipe(index) => {
                self.recipe = index;
                self.custom_recipe = None;
            }
            Message::Refine => {
                if let Some(work) = self.selection {
                    return self.action(
                        StudioAction::RefineLocal { work },
                        "本地炼药中：palette + spacing，仅像素计算…",
                        false,
                    );
                }
            }
            Message::Brew => {
                if let Some(work) = self.selection {
                    return self.action(
                        StudioAction::BrewLocal {
                            work,
                            palette_ratio: self.palette_ratio,
                        },
                        "正在按当前比例炼制丹药…",
                        false,
                    );
                }
            }
            Message::BrewRecipe => {
                let path = self.custom_recipe.clone().or_else(|| {
                    self.recipe
                        .and_then(|index| self.data.recipes.get(index))
                        .and_then(|r| data::path(r, "path"))
                });
                if let Some(path) = path {
                    return self.action(
                        StudioAction::BrewRecipe { path },
                        "正在执行本地配方…",
                        false,
                    );
                }
            }
            Message::Export => {
                if let Some(pill) = self.pill {
                    return self.choose(Dialog::Export(pill));
                }
            }
            Message::Dialog(kind) => return self.choose(kind),
            Message::DialogDone {
                generation,
                config,
                kind,
                result,
            } => {
                if generation != self.generation || !runtime::same_config(&config, &self.config) {
                    return Task::none();
                }
                self.busy = None;
                match result {
                    Ok(Some(path)) => match kind {
                        Dialog::Open => {
                            self.config.root = path
                                .parent()
                                .unwrap_or(std::path::Path::new("/"))
                                .to_path_buf();
                            self.config.db = path;
                            self.options.root_explicit = false;
                            self.writable = false;
                            self.assets.clear();
                            self.data = Snapshot::default();
                            self.selection = None;
                            self.pill = None;
                            self.experiment = None;
                            return self.reload();
                        }
                        Dialog::AssetRoot => {
                            self.config.root = path;
                            self.options.root_explicit = true;
                            self.assets.clear();
                            return self.reload();
                        }
                        Dialog::Copy => {
                            return self.action(
                                StudioAction::CreateCopy { target: path },
                                "正在建立独立工作副本…",
                                true,
                            );
                        }
                        Dialog::New => {
                            return self.action(
                                StudioAction::CreateEmpty { target: path },
                                "正在建立明确指定的新库…",
                                true,
                            );
                        }
                        Dialog::Export(pill) => {
                            return self.action(
                                StudioAction::ExportBrief { pill, out: path },
                                "正在导出冻结规格 Brief…",
                                false,
                            );
                        }
                        Dialog::Recipe => {
                            self.custom_recipe = Some(path);
                            self.recipe = None;
                            self.status =
                                "已选择本地配方。点击「按配方炼制」执行；不会调用模型。".into();
                        }
                    },
                    Ok(None) => {
                        self.status = "已取消，没有写入或切换数据库。".into();
                    }
                    Err(error) => {
                        self.error = true;
                        self.status = error;
                    }
                }
            }
            Message::Loaded {
                generation,
                config,
                result,
            } => {
                if generation != self.generation || !runtime::same_config(&config, &self.config) {
                    return Task::none();
                }
                self.busy = None;
                self.loaded = true;
                match result {
                    Ok(outcome) => {
                        if outcome.switched {
                            self.config = outcome.config;
                            self.writable = true;
                            self.assets.clear();
                        }
                        if !self.options.root_explicit
                            && let Some(root) = outcome.snapshot.asset_root_hint.as_ref()
                            && self.config.root != *root
                        {
                            self.config.root = root.clone();
                            self.assets.clear();
                            self.pending_assets.clear();
                            // Re-resolve image paths under the hinted root in a new generation.
                            return self.reload();
                        }
                        self.data = outcome.snapshot;
                        self.status = outcome.note;
                        self.error = false;
                        if self.data.work(self.selection).is_none() {
                            self.selection = self.data.works.first().map(data::id);
                        }
                        if self.data.pill(self.pill).is_none() {
                            self.pill = self.data.pills.first().map(data::id);
                        }
                        if self.data.experiment(self.experiment).is_none() {
                            self.experiment = self
                                .data
                                .experiments
                                .iter()
                                .find(|e| data::flag(e, "ready"))
                                .or(self.data.experiments.first())
                                .map(data::id);
                        }
                        self.sync_experiment_selection();
                        self.page = self.page.min(
                            self.data
                                .filtered_works(&self.query)
                                .len()
                                .saturating_sub(1)
                                / PAGE_SIZE,
                        );
                        return self.load_visible_images();
                    }
                    Err(error) => {
                        self.error = true;
                        self.status = error;
                        // Keep the previous snapshot browsable even if its refresh failed.
                        return self.load_visible_images();
                    }
                }
            }
            Message::Images { generation, result } => {
                if generation != self.generation {
                    return Task::none();
                }
                match result {
                    Ok(images) => {
                        for path in images.keys() {
                            self.pending_assets.remove(path);
                        }
                        self.assets.extend(images);
                    }
                    Err(error) => {
                        self.pending_assets.clear();
                        self.error = true;
                        self.status = error;
                    }
                }
                if self.assets.len() > 96 {
                    let visible: HashSet<_> = self.visible_paths().into_iter().collect();
                    self.assets.retain(|path, _| visible.contains(path));
                }
                self.capture_ticks = 0;
                self.rendered_frames = 0;
            }
            Message::Person(person) => {
                self.confirmation = None;
                self.person = person;
                if self.person.trim().is_empty() {
                    self.personal = false;
                }
            }
            Message::Personal(value) => {
                self.confirmation = None;
                self.personal = value && !self.effective_demo() && !self.person.trim().is_empty();
            }
            Message::Preference(winner) => {
                if !self.can_rate() {
                    self.status = "记录需要工作副本、两张实际载入的 A/B 图；非体验模式还需填写稳定的本机 person 标识。未设置身份不会记录为 human。".into();
                    return Task::none();
                }
                if !["A", "B", "tie", "abstain"].contains(&winner.as_str()) {
                    return Task::none();
                }
                if let Some(experiment) = self.active_experiment() {
                    let person = if self.effective_demo() || self.person.trim().is_empty() {
                        None
                    } else {
                        Some(self.person.trim().to_owned())
                    };
                    let personal = self.personal && person.is_some() && !self.effective_demo();
                    let action = StudioAction::RecordPreference {
                        experiment: data::id(experiment),
                        manifest_hash: data::label(experiment, "manifest_hash").to_owned(),
                        winner: winner.clone(),
                        person,
                        personal,
                        demo: self.effective_demo(),
                    };
                    self.confirmation = Some(PendingPreference {
                        action,
                        summary: format!(
                            "比较 #{} · 记录 {} · source={} · 私有意图={}",
                            data::id(experiment),
                            match winner.as_str() {
                                "tie" => "平手",
                                "abstain" => "弃权",
                                value => value,
                            },
                            self.preference_source(),
                            if personal { "开启" } else { "关闭" }
                        ),
                    });
                }
            }
            Message::ConfirmPreference => {
                if let Some(pending) = self.confirmation.take() {
                    let still_visible = match &pending.action {
                        StudioAction::RecordPreference {
                            experiment,
                            manifest_hash,
                            ..
                        } => self.active_experiment().is_some_and(|visible| {
                            data::id(visible) == *experiment
                                && data::label(visible, "manifest_hash") == manifest_hash
                        }),
                        _ => false,
                    };
                    if !still_visible || !self.can_rate() {
                        self.status =
                            "比较对象或可评状态已变化，请依据当前可见 A/B 图重新选择。".into();
                        return Task::none();
                    }
                    return self.action(pending.action, "正在记录偏好来源与选择…", false);
                }
            }
            Message::Cancel => {
                self.confirmation = None;
            }
            Message::Window(id, event) => {
                self.window = Some(id);
                match event {
                    window::Event::Opened { size, .. } | window::Event::Resized(size) => {
                        self.width = size.width
                    }
                    window::Event::CloseRequested => return iced::exit(),
                    _ => {}
                }
            }
            Message::Frame => {
                if self.loaded && self.pending_assets.is_empty() && self.busy.is_none() {
                    self.rendered_frames = self.rendered_frames.saturating_add(1);
                }
            }
            Message::CaptureTick => {
                if self.loaded
                    && self.pending_assets.is_empty()
                    && self.busy.is_none()
                    && !self.capturing
                {
                    self.capture_ticks = self.capture_ticks.saturating_add(1);
                    if self.capture_ticks >= 4
                        && self.rendered_frames >= 2
                        && let Some(id) = self.window
                    {
                        self.capturing = true;
                        return window::screenshot(id).map(Message::Captured);
                    }
                }
            }
            Message::Captured(shot) => {
                if let Some(path) = self.options.screenshot.clone() {
                    return Task::perform(
                        async move {
                            tokio::task::spawn_blocking(move || {
                                runtime::save_screenshot(&path, shot)
                            })
                            .await
                            .map_err(|e| format!("截图任务异常：{e}"))?
                        },
                        Message::CaptureSaved,
                    );
                }
            }
            Message::CaptureSaved(result) => {
                self.options.screenshot = None;
                match result {
                    Ok(path) => {
                        self.status = format!("原生窗口截图已保存：{}", path.display());
                        eprintln!("{}", self.status);
                    }
                    Err(error) => {
                        self.error = true;
                        self.status = error;
                        eprintln!("{}", self.status);
                    }
                }
                if self.options.exit_after_capture {
                    return iced::exit();
                }
            }
            Message::FocusSearch => return iced::widget::operation::focus("studio-search"),
            Message::FocusNext(previous) => {
                return if previous {
                    iced::widget::operation::focus_previous()
                } else {
                    iced::widget::operation::focus_next()
                };
            }
            Message::Key(key, modifiers) => {
                use keyboard::key::Named;
                if key == keyboard::Key::Named(Named::Escape) {
                    return self.update(Message::Cancel);
                }
                if key == keyboard::Key::Named(Named::Tab) {
                    return self.update(Message::FocusNext(modifiers.shift()));
                }
                if modifiers.command()
                    && let keyboard::Key::Character(value) = key
                {
                    match value.as_str().to_lowercase().as_str() {
                        "f" => return self.update(Message::FocusSearch),
                        "r" => return self.update(Message::Refresh),
                        "1" => return self.update(Message::Navigate(Screen::Library)),
                        "2" => return self.update(Message::Navigate(Screen::Brew)),
                        "3" => return self.update(Message::Navigate(Screen::Pills)),
                        "4" => return self.update(Message::Navigate(Screen::Preferences)),
                        "o" => return self.update(Message::Dialog(Dialog::Open)),
                        _ => {}
                    }
                }
            }
        }
        Task::none()
    }
    pub fn preference_source(&self) -> &'static str {
        if self.effective_demo() {
            "demo（不记录真人）"
        } else if self.person.trim().is_empty() {
            "未设置（非 human）"
        } else {
            "human（显式身份）"
        }
    }
    pub fn subscription(&self) -> Subscription<Message> {
        let mut subscriptions = vec![
            window::events().filter_map(|(id, event)| {
                if matches!(
                    event,
                    window::Event::Opened { .. }
                        | window::Event::Resized(_)
                        | window::Event::CloseRequested
                ) {
                    Some(Message::Window(id, event))
                } else {
                    None
                }
            }),
            iced::event::listen_with(|event, status, _| {
                if let iced::Event::Keyboard(keyboard::Event::KeyPressed {
                    key, modifiers, ..
                }) = event
                {
                    if modifiers.command()
                        || status == iced::event::Status::Ignored
                        || key == keyboard::Key::Named(keyboard::key::Named::Escape)
                    {
                        Some(Message::Key(key, modifiers))
                    } else {
                        None
                    }
                } else {
                    None
                }
            }),
        ];
        if self.options.screenshot.is_some() && !self.capturing {
            subscriptions
                .push(iced::time::every(Duration::from_millis(120)).map(|_| Message::CaptureTick));
            subscriptions.push(window::frames().map(|_| Message::Frame));
        }
        Subscription::batch(subscriptions)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn app(demo: bool) -> Studio {
        let args: Vec<_> = if demo {
            vec!["--demo", "--db", "isolated.db"]
        } else {
            vec![]
        };
        let options = Options::parse(
            args.into_iter().map(str::to_owned),
            std::path::Path::new("/isolated"),
        )
        .unwrap();
        Studio::new(options).0
    }
    #[test]
    fn private_intent_defaults_off_and_needs_identity() {
        let mut a = app(false);
        assert!(!a.personal);
        let _ = a.update(Message::Personal(true));
        assert!(!a.personal);
        let _ = a.update(Message::Person("local-person".into()));
        let _ = a.update(Message::Personal(true));
        assert!(a.personal);
        let _ = a.update(Message::Person(String::new()));
        assert!(!a.personal);
        assert!(!a.preference_source().starts_with("human"));
    }
    #[test]
    fn demo_never_personal_or_human() {
        let mut a = app(true);
        let _ = a.update(Message::Person("person".into()));
        let _ = a.update(Message::Personal(true));
        assert!(!a.personal);
        assert!(a.preference_source().starts_with("demo"));
    }
    #[test]
    fn stale_snapshot_cannot_replace_current_database() {
        let mut a = app(false);
        let config = a.config.clone();
        let original = a.generation;
        a.config.db = "/isolated/new.db".into();
        let result = Outcome {
            config: config.clone(),
            snapshot: Snapshot::from_value(serde_json::json!({"works":[{"id":999}]})),
            switched: false,
            note: "stale".into(),
        };
        let _ = a.update(Message::Loaded {
            generation: original,
            config,
            result: Ok(Box::new(result)),
        });
        assert!(a.data.works.is_empty());
    }
    fn ready_app(demo_marker: bool) -> Studio {
        let mut a = app(false);
        a.busy = None;
        a.writable = true;
        a.data = Snapshot::from_value(
            serde_json::json!({"schema_version":25,"demo":demo_marker,"experiments":[{"id":7,"ready":true,"manifest_hash":"hash","A":"/isolated/A.png","B":"/isolated/B.png"}]}),
        );
        a.experiment = Some(7);
        for name in ["A", "B"] {
            a.assets.insert(
                PathBuf::from(format!("/isolated/{name}.png")),
                Asset {
                    handle: Some(iced::widget::image::Handle::from_rgba(
                        1,
                        1,
                        vec![0, 0, 0, 255],
                    )),
                    error: None,
                },
            );
        }
        a
    }
    #[test]
    fn database_demo_marker_forces_demo_rating_without_flag() {
        let mut a = ready_app(true);
        a.person = "must-not-be-human".into();
        a.personal = true;
        let _ = a.update(Message::Preference("A".into()));
        match &a.confirmation.unwrap().action {
            StudioAction::RecordPreference {
                demo,
                person,
                personal,
                ..
            } => {
                assert!(*demo);
                assert!(person.is_none());
                assert!(!personal);
            }
            _ => panic!("expected preference"),
        }
    }
    #[test]
    fn missing_identity_never_records_human() {
        let mut a = ready_app(false);
        assert!(!a.can_rate());
        let _ = a.update(Message::Preference("A".into()));
        assert!(a.confirmation.is_none());
        assert!(a.status.contains("person"));
    }
    #[test]
    fn stale_generation_images_and_snapshot_are_ignored() {
        let mut a = ready_app(false);
        a.generation = 4;
        let config = a.config.clone();
        let result = Outcome {
            config: config.clone(),
            snapshot: Snapshot::default(),
            switched: false,
            note: "stale".into(),
        };
        let _ = a.update(Message::Loaded {
            generation: 3,
            config,
            result: Ok(Box::new(result)),
        });
        assert_eq!(a.data.experiments.len(), 1);
        let images = HashMap::from([(
            PathBuf::from("/stale.png"),
            Asset {
                handle: None,
                error: Some("old".into()),
            },
        )]);
        let _ = a.update(Message::Images {
            generation: 3,
            result: Ok(images),
        });
        assert!(!a.assets.contains_key(&PathBuf::from("/stale.png")));
    }
    #[test]
    fn busy_blocks_writes_without_blocking_navigation() {
        let mut a = ready_app(true);
        a.busy = Some("working".into());
        assert!(!a.can_write());
        assert!(!a.can_rate());
        let _ = a.update(Message::Navigate(Screen::Pills));
        assert_eq!(a.screen, Screen::Pills);
    }
    #[test]
    fn preference_search_selects_visible_match_for_display_and_confirmation() {
        let mut a = ready_app(true);
        a.screen = Screen::Preferences;
        a.data.experiments.push(serde_json::json!({"id":8,"ready":true,"manifest_hash":"second-hash","content_summary":"quiet","A":"/isolated/A.png","B":"/isolated/B.png"}));
        let _ = a.update(Message::Search("quiet".into()));
        assert_eq!(a.experiment, Some(8));
        assert_eq!(data::id(a.active_experiment().unwrap()), 8);
        let _ = a.update(Message::Preference("B".into()));
        let pending = a.confirmation.as_ref().unwrap();
        assert!(pending.summary.contains("#8"));
        match &pending.action {
            StudioAction::RecordPreference {
                experiment,
                manifest_hash,
                ..
            } => {
                assert_eq!(*experiment, 8);
                assert_eq!(manifest_hash, "second-hash");
            }
            _ => panic!("expected preference"),
        }
        let _ = a.update(Message::Search("no-result".into()));
        assert!(a.confirmation.is_none());
        assert_eq!(a.experiment, None);
        assert!(a.active_experiment().is_none());
        assert!(!a.can_rate());
        let _ = a.update(Message::ClearSearch);
        assert_eq!(a.experiment, Some(7));
    }
    #[test]
    fn confirmation_rejects_an_experiment_hidden_by_search() {
        let mut a = ready_app(true);
        a.screen = Screen::Preferences;
        let _ = a.update(Message::Preference("A".into()));
        let old = a.confirmation.take().unwrap();
        a.query = "no-result".into();
        a.confirmation = Some(old);
        let _ = a.update(Message::ConfirmPreference);
        assert!(a.confirmation.is_none());
        assert!(a.busy.is_none());
        assert!(a.status.contains("比较对象"));
    }
    #[test]
    fn native_open_relocates_root_and_clears_cli_override() {
        let mut a = ready_app(false);
        a.options.root_explicit = true;
        let generation = a.generation;
        let config = a.config.clone();
        let _ = a.update(Message::DialogDone {
            generation,
            config,
            kind: Dialog::Open,
            result: Ok(Some("/selected/materials/library.db".into())),
        });
        assert_eq!(a.config.db, PathBuf::from("/selected/materials/library.db"));
        assert_eq!(a.config.root, PathBuf::from("/selected/materials"));
        assert!(!a.options.root_explicit);
        assert!(!a.writable);
        assert_eq!(a.generation, generation + 1);
    }
    #[test]
    fn reopened_copy_adopts_hint_in_a_new_generation() {
        let mut a = ready_app(false);
        let config = a.config.clone();
        let generation = a.generation;
        let outcome = Outcome {
            config: config.clone(),
            snapshot: Snapshot::from_value(
                serde_json::json!({"schema_version":25,"asset_root_hint":"/original/assets"}),
            ),
            switched: false,
            note: "reopened".into(),
        };
        let _ = a.update(Message::Loaded {
            generation,
            config,
            result: Ok(Box::new(outcome)),
        });
        assert_eq!(a.config.root, PathBuf::from("/original/assets"));
        assert_eq!(a.generation, generation + 1);
        assert!(a.busy.is_some());
    }
    #[test]
    fn explicit_cli_root_is_not_overridden_by_database_hint() {
        let mut a = ready_app(false);
        a.options.root_explicit = true;
        let config = a.config.clone();
        let generation = a.generation;
        let root = a.config.root.clone();
        let outcome = Outcome {
            config: config.clone(),
            snapshot: Snapshot::from_value(
                serde_json::json!({"schema_version":25,"asset_root_hint":"/not-my-explicit-root"}),
            ),
            switched: false,
            note: "loaded".into(),
        };
        let _ = a.update(Message::Loaded {
            generation,
            config,
            result: Ok(Box::new(outcome)),
        });
        assert_eq!(a.config.root, root);
        assert_eq!(a.generation, generation);
        assert!(a.busy.is_none());
    }
    #[test]
    fn copy_switch_does_not_relocate_asset_root_to_target_parent() {
        let mut a = ready_app(false);
        let config = a.config.clone();
        let root = config.root.clone();
        let mut next = config.clone();
        next.db = "/different/copy.db".into();
        let outcome = Outcome {
            config: next,
            snapshot: Snapshot::from_value(serde_json::json!({"schema_version":25})),
            switched: true,
            note: "copied".into(),
        };
        let _ = a.update(Message::Loaded {
            generation: a.generation,
            config,
            result: Ok(Box::new(outcome)),
        });
        assert_eq!(a.config.root, root);
        assert_eq!(a.config.db, PathBuf::from("/different/copy.db"));
    }
    #[test]
    fn raw_pill_data_defaults_collapsed_and_resets_on_selection() {
        let mut a = ready_app(false);
        assert!(!a.show_raw_pill);
        let _ = a.update(Message::TogglePillRaw);
        assert!(a.show_raw_pill);
        let _ = a.update(Message::SelectPill(42));
        assert!(!a.show_raw_pill);
    }
    #[test]
    fn explicit_refresh_retries_failed_image_at_the_same_path() {
        let mut a = app(false);
        let path = PathBuf::from("/isolated/repaired.png");
        let snapshot = Snapshot::from_value(serde_json::json!({
            "schema_version": 25,
            "works": [{"id": 1, "image_path": path}]
        }));
        let config = a.config.clone();
        let outcome = || Outcome {
            config: config.clone(),
            snapshot: snapshot.clone(),
            switched: false,
            note: "fixture".into(),
        };
        let _ = a.update(Message::Loaded {
            generation: a.generation,
            config: config.clone(),
            result: Ok(Box::new(outcome())),
        });
        let _ = a.update(Message::Images {
            generation: a.generation,
            result: Ok(HashMap::from([(
                path.clone(),
                Asset {
                    handle: None,
                    error: Some("fixture was missing".into()),
                },
            )])),
        });
        assert!(a.assets[&path].error.is_some());
        let _ = a.update(Message::Refresh);
        let _ = a.update(Message::Loaded {
            generation: a.generation,
            config: config.clone(),
            result: Ok(Box::new(outcome())),
        });
        assert!(a.pending_assets.contains(&path));
        assert!(!a.assets.contains_key(&path));
    }
    #[test]
    fn explicit_refresh_reprobes_successful_images_and_rejects_stale_results() {
        let mut a = ready_app(true);
        a.screen = Screen::Preferences;
        let generation = a.generation;
        let stale_images = a.assets.clone();
        let config = a.config.clone();
        let snapshot = a.data.clone();
        let _ = a.update(Message::Refresh);
        assert_eq!(a.generation, generation + 1);
        assert!(a.assets.is_empty());
        let _ = a.update(Message::Images {
            generation,
            result: Ok(stale_images),
        });
        assert!(a.assets.is_empty());
        let _ = a.update(Message::Loaded {
            generation: a.generation,
            config: config.clone(),
            result: Ok(Box::new(Outcome {
                config,
                snapshot,
                switched: false,
                note: "fixture".into(),
            })),
        });
        for name in ["A", "B"] {
            assert!(
                a.pending_assets
                    .contains(&PathBuf::from(format!("/isolated/{name}.png")))
            );
        }
        assert!(!a.ready_experiment());
    }
    #[test]
    fn refresh_while_busy_does_not_clear_images_or_pending_work() {
        let mut a = ready_app(false);
        let generation = a.generation;
        let pending = PathBuf::from("/isolated/pending.png");
        a.pending_assets.insert(pending.clone());
        a.busy = Some("fixture in progress".into());
        let _ = a.update(Message::Refresh);
        assert_eq!(a.generation, generation);
        assert_eq!(a.assets.len(), 2);
        assert!(a.pending_assets.contains(&pending));
        assert_eq!(a.busy.as_deref(), Some("fixture in progress"));
    }
    #[test]
    fn failed_refresh_reprobes_old_visible_images_instead_of_leaving_them_loading() {
        let mut a = ready_app(true);
        a.screen = Screen::Preferences;
        let config = a.config.clone();
        let _ = a.update(Message::Refresh);
        let _ = a.update(Message::Loaded {
            generation: a.generation,
            config,
            result: Err("fixture database could not be refreshed".into()),
        });
        assert!(a.busy.is_none());
        assert!(a.error);
        assert_eq!(a.status, "fixture database could not be refreshed");
        for path in a.visible_paths() {
            assert!(a.pending_assets.contains(&path));
        }
    }
    #[test]
    fn explicit_asset_directory_reloads_without_unlocking_or_switching_database() {
        let mut a = ready_app(false);
        a.writable = false;
        let generation = a.generation;
        let config = a.config.clone();
        let db = config.db.clone();
        let chosen = PathBuf::from("/chosen/materials");
        let _ = a.update(Message::DialogDone {
            generation,
            config,
            kind: Dialog::AssetRoot,
            result: Ok(Some(chosen.clone())),
        });
        assert_eq!(a.config.root, chosen);
        assert!(a.options.root_explicit);
        assert_eq!(a.config.db, db);
        assert!(!a.writable);
        assert!(a.assets.is_empty());
        assert!(a.pending_assets.is_empty());
        assert_eq!(a.generation, generation + 1);
        assert!(a.busy.is_some());

        let config = a.config.clone();
        let generation = a.generation;
        let _ = a.update(Message::Loaded {
            generation,
            config: config.clone(),
            result: Ok(Box::new(Outcome {
                config,
                snapshot: Snapshot::from_value(serde_json::json!({
                    "schema_version": 25,
                    "asset_root_hint": "/old/materials"
                })),
                switched: false,
                note: "fixture".into(),
            })),
        });
        assert_eq!(a.config.root, chosen);
        assert_eq!(a.generation, generation);
        assert!(a.busy.is_none());
        assert!(!a.writable);
    }
    #[test]
    fn asset_directory_dialog_is_blocked_while_busy_and_cancel_is_non_mutating() {
        let mut a = ready_app(false);
        let generation = a.generation;
        let config = a.config.clone();
        a.busy = Some("fixture in progress".into());
        let _ = a.update(Message::Dialog(Dialog::AssetRoot));
        assert_eq!(a.busy.as_deref(), Some("fixture in progress"));
        assert_eq!(a.generation, generation);
        assert!(runtime::same_config(&a.config, &config));

        a.busy = None;
        let _ = a.update(Message::Dialog(Dialog::AssetRoot));
        assert!(a.busy.is_some());
        let _ = a.update(Message::DialogDone {
            generation,
            config: config.clone(),
            kind: Dialog::AssetRoot,
            result: Ok(None),
        });
        assert!(a.busy.is_none());
        assert_eq!(a.generation, generation);
        assert!(runtime::same_config(&a.config, &config));
        assert_eq!(a.assets.len(), 2);
        assert!(!a.options.root_explicit);
        assert!(a.writable);
    }
    #[test]
    fn old_asset_directory_dialog_cannot_replace_root_or_release_reload() {
        let mut a = ready_app(false);
        let generation = a.generation;
        let config = a.config.clone();
        let _ = a.update(Message::DialogDone {
            generation,
            config: config.clone(),
            kind: Dialog::AssetRoot,
            result: Ok(Some("/chosen/materials".into())),
        });
        let current = a.config.clone();
        let current_generation = a.generation;
        for (generation, config) in [(generation, config.clone()), (a.generation, config)] {
            let _ = a.update(Message::DialogDone {
                generation,
                config,
                kind: Dialog::AssetRoot,
                result: Ok(Some("/stale/materials".into())),
            });
            assert!(runtime::same_config(&a.config, &current));
            assert_eq!(a.generation, current_generation);
            assert!(a.busy.is_some());
        }
    }
    #[test]
    fn startup_database_is_always_read_only() {
        let a = app(true);
        assert!(!a.writable);
        assert!(!a.can_write());
    }
}
