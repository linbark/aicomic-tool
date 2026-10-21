use crate::Options;
use crate::board::{Board, BoardCard, BoardView};
use crate::event_matrix::{self, MatrixMessage};
use crate::session::{EditorSession, EpisodeKey};
use aicomic_ai::{
    AiProvider, AiSettings, ContextStore, ExecutionInput, PromptRegistry, ReviewDecision,
    WorkflowEngine, WorkflowResult,
};
use aicomic_core::{AssetItemUpdate, AssetTarget, EpisodeUpdate, EventUpdate, Store};
use aicomic_memory::{EmbeddingSettings, MemoryQuery, MemoryService, MemoryStore};
use iced::widget::{
    button, column, container, image, pick_list, row, scrollable, space, text, text_editor,
    text_input,
};
use iced::{Color, Element, Fill, Length, Subscription, Task, Theme};
use serde::Serialize;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::fmt;
use std::path::PathBuf;
use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Page {
    Script,
    Events,
    Assets,
    Context,
    Runs,
    Memory,
    Ai,
    Prompts,
}
impl Page {
    const ALL: [Page; 8] = [
        Self::Script,
        Self::Events,
        Self::Assets,
        Self::Context,
        Self::Runs,
        Self::Memory,
        Self::Ai,
        Self::Prompts,
    ];
    fn label(self) -> &'static str {
        match self {
            Self::Script => "剧本",
            Self::Events => "事件",
            Self::Assets => "资产",
            Self::Context => "项目上下文",
            Self::Runs => "运行记录",
            Self::Memory => "记忆",
            Self::Ai => "AI 设置",
            Self::Prompts => "Prompt 模板",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Choice {
    pub id: i64,
    pub name: String,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct AssetCategory(&'static str, &'static str);
impl fmt::Display for AssetCategory {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.1.fmt(f)
    }
}
const ASSET_CATEGORIES: [AssetCategory; 12] = [
    AssetCategory("persona_visual", "人物形象"),
    AssetCategory("persona_voice", "人物声音"),
    AssetCategory("background", "场景背景"),
    AssetCategory("element", "画面元素"),
    AssetCategory("prop", "道具"),
    AssetCategory("pose", "姿势"),
    AssetCategory("vfx", "视觉特效"),
    AssetCategory("layout", "画面布局"),
    AssetCategory("audio_music", "音乐"),
    AssetCategory("audio_sfx", "音效"),
    AssetCategory("branding", "品牌素材"),
    AssetCategory("ai_preset", "AI 预设"),
];
#[derive(Clone)]
struct PendingCommit {
    run_id: String,
    workflow_status: String,
    artifacts: Value,
}
impl fmt::Display for Choice {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.name.fmt(f)
    }
}

#[derive(Debug, Clone)]
pub enum Message {
    Page(Page),
    Project(Choice),
    NewName(String),
    NewDescription(String),
    CreateProject,
    DeleteProject,
    CreateEpisode,
    Episode(i64, u8),
    DeleteEpisode(i64),
    Title(String),
    Edit(text_editor::Action),
    Save,
    Run(String),
    RunComplete(EpisodeKey, String, Result<WorkflowResult, String>),
    Cancel,
    Review(bool),
    Board(BoardView),
    Tick,
    Snapshot(iced::window::Screenshot),
    Close,
    Provider(AiProvider),
    Setting(&'static str, String),
    SaveSettings,
    TestAi,
    AiTest(Result<String, String>),
    SelectPrompt(String),
    PromptField(&'static str, String),
    SavePrompt,
    ResetPrompt,
    DeletePrompt,
    ContextKind(String),
    SaveContext,
    SelectRun(String),
    SelectStage(String),
    SelectItem(i64),
    Category(String),
    CreateItem,
    DeleteItem(i64),
    ImportMedia,
    Imported(Result<(), String>),
    OpenMedia(PathBuf),
    DeleteAsset(i64),
    CreateEvent,
    DeleteEvent(i64),
    SelectEvent(i64),
    ToggleScene(i64),
    SaveEvent,
    MemorySearch(String),
    Refresh,
    ReviewChangeset(String, bool),
    ResolveConflict(String),
    ConfirmDelete(bool),
    ApplyScript,
    ApplyArtifacts,
    Embedding(&'static str, String),
    SaveEmbedding,
    RebuildMemory,
    MemoryRebuilt(Result<usize, String>),
    SemanticSearch,
    MemoryFound(i64, u64, Result<Vec<Value>, String>),
    Matrix(MatrixMessage),
    ContextSummary(text_editor::Action),
    ContextField(&'static str, String),
    ContextVisual(Choice),
    ItemField(&'static str, String),
    ItemDescription(text_editor::Action),
    ItemPrompt(text_editor::Action),
    SaveItem,
    VisualAsset(i64),
    EventField(&'static str, String),
    EventFilter(String),
    EventFocus(i64),
    EventSort(String),
    SaveEventMetadata,
    PromptSearch(String),
    Inspect(text_editor::Action),
}

pub struct App {
    pending_commits: HashMap<EpisodeKey, PendingCommit>,
    store: Store,
    options: Options,
    page: Page,
    status: String,
    projects: Vec<Choice>,
    project: Option<Choice>,
    episodes: Vec<Value>,
    title: String,
    new_name: String,
    new_description: String,
    session: EditorSession,
    editor: text_editor::Content,
    step: u8,
    outputs: HashMap<EpisodeKey, Value>,
    board: BoardView,
    busy: Option<(EpisodeKey, String, iced::task::Handle)>,
    paused: Option<(EpisodeKey, String)>,
    settings: AiSettings,
    settings_tokens: String,
    settings_timeout: String,
    settings_temperature: String,
    templates: Vec<Value>,
    prompt_key: String,
    prompt_title: String,
    prompt_category: String,
    context_kind: String,
    run_list: Vec<Value>,
    selected_run: String,
    selected_stage: String,
    stages: Vec<String>,
    items: Vec<Value>,
    selected_item: Option<i64>,
    category: String,
    media: Vec<Value>,
    events: Vec<Value>,
    selected_event: Option<i64>,
    scenes: Vec<Value>,
    linked_scenes: Vec<i64>,
    memory_records: Vec<Value>,
    changesets: Vec<Value>,
    conflicts: Vec<Value>,
    memory_search: String,
    embedding: EmbeddingSettings,
    embedding_max: String,
    memory_service: MemoryService,
    matrix_view: BoardView,
    memory_request: u64,
    context_summary: text_editor::Content,
    context_instruction: String,
    context_episodes: String,
    item_name: String,
    item_description: text_editor::Content,
    item_prompt: text_editor::Content,
    selected_visual_asset: Option<i64>,
    event_name: String,
    event_description: String,
    event_color: String,
    event_search: String,
    event_sort: String,
    event_focus: Vec<i64>,
    prompt_search: String,
    item_category: String,
    matrix_events: Vec<Value>,
    ticks: u32,
    pending_delete: Option<(bool, i64)>,
}

fn stringify<T: Serialize>(rows: impl IntoIterator<Item = T>) -> Vec<Value> {
    rows.into_iter()
        .filter_map(|row| serde_json::to_value(row).ok())
        .collect()
}
fn id(value: &Value) -> i64 {
    value["id"].as_i64().unwrap_or(0)
}
fn label<'a>(value: &'a Value, key: &str) -> &'a str {
    value[key].as_str().unwrap_or("")
}
fn pretty(value: &Value) -> String {
    serde_json::to_string_pretty(value).unwrap_or_default()
}
fn small<'a>(value: impl Into<String>) -> iced::widget::Text<'a> {
    text(value.into())
        .size(12)
        .color(Color::from_rgb8(148, 163, 184))
}

impl App {
    pub fn new(store: Store, options: Options) -> Self {
        let settings = AiSettings::load(&options.root).unwrap_or_default();
        let embedding = EmbeddingSettings::load(&options.root).unwrap_or_default();
        let memory_service = MemoryService::new(&options.root, embedding.clone());
        let mut app = Self {
            pending_commits: HashMap::new(),
            store,
            options,
            page: Page::Script,
            status: "就绪 · 本地保存".into(),
            projects: vec![],
            project: None,
            episodes: vec![],
            title: String::new(),
            new_name: String::new(),
            new_description: String::new(),
            session: EditorSession::default(),
            editor: text_editor::Content::new(),
            step: 0,
            outputs: HashMap::new(),
            board: BoardView::default(),
            busy: None,
            paused: None,
            settings_tokens: settings
                .max_tokens
                .map(|n| n.to_string())
                .unwrap_or_default(),
            settings_timeout: settings.timeout_seconds.to_string(),
            settings_temperature: settings.temperature.to_string(),
            settings,
            templates: vec![],
            prompt_key: String::new(),
            prompt_title: String::new(),
            prompt_category: "custom".into(),
            context_kind: "series_bible".into(),
            run_list: vec![],
            selected_run: String::new(),
            selected_stage: String::new(),
            stages: vec![],
            items: vec![],
            selected_item: None,
            category: "persona_visual".into(),
            media: vec![],
            events: vec![],
            selected_event: None,
            scenes: vec![],
            linked_scenes: vec![],
            memory_records: vec![],
            changesets: vec![],
            conflicts: vec![],
            memory_search: String::new(),
            ticks: 0,
            pending_delete: None,
            embedding_max: embedding.max_tokens.to_string(),
            embedding,
            memory_service,
            matrix_view: BoardView::default(),
            memory_request: 0,
            context_summary: text_editor::Content::new(),
            context_instruction: String::new(),
            context_episodes: "12".into(),
            item_name: String::new(),
            item_description: text_editor::Content::new(),
            item_prompt: text_editor::Content::new(),
            selected_visual_asset: None,
            event_name: String::new(),
            event_description: String::new(),
            event_color: "#6366F1".into(),
            event_search: String::new(),
            event_sort: "名称".into(),
            event_focus: vec![],
            prompt_search: String::new(),
            item_category: "persona_visual".into(),
            matrix_events: vec![],
        };
        if app.options.demo
            && app.store.projects().map(|r| r.is_empty()).unwrap_or(false)
            && let Ok(project) = app
                .store
                .create_project("星海来信", Some("从文字到画面的创作旅程"))
        {
            let _ = app.store.create_episode(project.id, "第一集：雨夜重逢", Some("第 1 场 · 旧车站 · 夜\n\n雨水从屋檐滴落。林夏握着一封泛黄的信，站在月台尽头。\n\n林夏：你说过，最后一班车开走前，你一定会回来。\n\n远处传来脚步声。她抬起头，一道熟悉的身影从雾中走来。\n\n陈默：这一次，我没有迟到。"));
            let _ = app.store.create_episode(
                project.id,
                "第二集：未寄出的信",
                Some("黎明时，两人走进了多年未开的邮局。"),
            );
        }
        app.reload_projects();
        if let Some(choice) = app.projects.first().cloned() {
            app.select_project(choice);
        }
        app
    }

    pub fn theme(&self) -> Theme {
        Theme::custom(
            "星晴",
            iced::theme::Palette {
                background: Color::from_rgb8(11, 18, 32),
                text: Color::from_rgb8(229, 231, 235),
                primary: Color::from_rgb8(99, 102, 241),
                success: Color::from_rgb8(52, 211, 153),
                danger: Color::from_rgb8(248, 113, 113),
                warning: Color::from_rgb8(251, 191, 36),
            },
        )
    }
    pub fn subscription(&self) -> Subscription<Message> {
        Subscription::batch([
            if self.options.smoke || self.busy.is_some() {
                iced::time::every(Duration::from_millis(700)).map(|_| Message::Tick)
            } else {
                Subscription::none()
            },
            iced::window::close_requests().map(|_| Message::Close),
        ])
    }

    fn root(&self) -> PathBuf {
        self.options.root.clone()
    }
    fn pid(&self) -> Option<i64> {
        self.project.as_ref().map(|p| p.id)
    }
    fn fail(&mut self, error: impl fmt::Display) {
        self.status = format!("操作失败：{error}");
    }
    fn reload_projects(&mut self) {
        match self.store.projects() {
            Ok(rows) => {
                self.projects = rows
                    .into_iter()
                    .map(|p| Choice {
                        id: p.id,
                        name: p.name,
                    })
                    .collect()
            }
            Err(e) => self.fail(e),
        }
    }
    fn select_project(&mut self, choice: Choice) {
        self.sync_editor();
        self.project = Some(choice);
        self.session.current = None;
        self.editor = text_editor::Content::new();
        self.selected_item = None;
        self.selected_event = None;
        self.media.clear();
        self.paused = None;
        self.linked_scenes.clear();
        self.memory_request += 1;
        self.selected_run.clear();
        self.selected_stage.clear();
        self.stages.clear();
        self.event_focus.clear();
        self.matrix_view = BoardView::default();
        self.reload();
        if let Some(episode) = self.episodes.first().cloned() {
            self.select_episode(id(&episode), 0);
        }
        if self.page != Page::Script {
            self.load_document();
        }
    }
    fn reload(&mut self) {
        let Some(pid) = self.pid() else {
            return;
        };
        match self.store.episodes(pid) {
            Ok(rows) => self.episodes = stringify(rows),
            Err(e) => self.fail(e),
        }
        for episode in &self.episodes {
            if episode["exec_artifacts"].is_object() {
                self.outputs
                    .entry(EpisodeKey {
                        project: pid,
                        episode: id(episode),
                    })
                    .or_insert_with(|| episode["exec_artifacts"].clone());
            }
        }
        match self.store.asset_items(pid) {
            Ok(rows) => self.items = stringify(rows),
            Err(e) => self.fail(e),
        }
        match self.store.event_matrix(pid) {
            Ok(matrix) => {
                self.events = stringify(matrix.events);
                let nodes = stringify(matrix.nodes);
                for event in &mut self.events {
                    event["nodes"] = json!(
                        nodes
                            .iter()
                            .filter(|n| n["event_id"].as_i64() == Some(id(event)))
                            .collect::<Vec<_>>()
                    );
                }
            }
            Err(e) => self.fail(e),
        }
        self.filter_events();
        self.scenes.clear();
        for episode in &self.episodes {
            if let Ok(rows) = self.store.scenes(id(episode)) {
                self.scenes
                    .extend(stringify(rows).into_iter().map(|mut scene| {
                        scene["episode_order"] = episode["order"].clone();
                        scene["episode_title"] = episode["title"].clone();
                        scene
                    }));
            }
        }
        let contexts = ContextStore::new(self.root());
        if let Ok(runs) = contexts.list_runs(&pid.to_string()) {
            self.run_list = stringify(runs);
        }
        for run in &self.run_list {
            let run_id = label(run, "run_id");
            if let Ok(Some(stage)) =
                contexts.read_stage(&pid.to_string(), run_id, "desktop.materialization")
                && stage["status"] == "error"
                && stage["artifacts"].is_object()
            {
                let key = EpisodeKey {
                    project: pid,
                    episode: stage["episode_id"].as_i64().unwrap_or(0),
                };
                if key.episode < 0
                    || (key.episode > 0
                        && !self
                            .episodes
                            .iter()
                            .any(|episode| id(episode) == key.episode))
                {
                    continue;
                }
                self.pending_commits
                    .entry(key)
                    .or_insert_with(|| PendingCommit {
                        run_id: run_id.into(),
                        workflow_status: label(&stage, "workflow_status").into(),
                        artifacts: stage["artifacts"].clone(),
                    });
            }
        }
        if let Ok(memory) = MemoryStore::open_root(&self.root()) {
            let mut query = MemoryQuery::new(pid);
            query.top_k = 100;
            if !self.memory_search.is_empty() {
                query.query_text = Some(self.memory_search.clone());
            }
            if let Ok(rows) = memory.list_records(&query) {
                self.memory_records = stringify(rows);
            }
            if let Ok(rows) = memory.list_changesets(pid, None, 100) {
                self.changesets = rows;
            }
            if let Ok(rows) = memory.list_conflicts(pid, None, 100) {
                self.conflicts = rows;
            }
        }
    }
    fn sync_editor(&mut self) {
        if self.page == Page::Script && self.step == 0 {
            self.session.edit(self.editor.text());
        }
    }
    fn select_episode(&mut self, episode_id: i64, step: u8) {
        self.sync_editor();
        self.paused = None;
        let Some(pid) = self.pid() else {
            return;
        };
        if let Some(episode) = self.episodes.iter().find(|ep| id(ep) == episode_id) {
            self.title = label(episode, "title").into();
            self.session.open(
                EpisodeKey {
                    project: pid,
                    episode: episode_id,
                },
                label(episode, "description"),
            );
            if let Some(title) = self.session.draft().and_then(|d| d.title.clone()) {
                self.title = title;
            }
            self.step = step;
            self.editor = text_editor::Content::with_text(
                if step == 0 {
                    self.session
                        .draft()
                        .map(|d| d.text.clone())
                        .unwrap_or_default()
                } else {
                    self.output_text()
                }
                .as_str(),
            );
            self.board = BoardView::default();
            let run = label(episode, "last_exec_run_id");
            if !run.is_empty()
                && self.busy.is_none()
                && let Ok(Some(stage)) =
                    ContextStore::new(self.root()).read_stage(&pid.to_string(), run, "chat.status")
            {
                self.paused = (stage["status"] == "paused").then(|| {
                    (
                        EpisodeKey {
                            project: pid,
                            episode: episode_id,
                        },
                        run.to_owned(),
                    )
                });
            }
        }
    }
    fn output_text(&self) -> String {
        self.session
            .current
            .and_then(|key| self.outputs.get(&key))
            .map(pretty)
            .unwrap_or_else(|| "尚无生成结果。选择 AI 动作后开始生成。".into())
    }
    fn save_script(&mut self) -> bool {
        self.sync_editor();
        let Some(key) = self.session.current else {
            return false;
        };
        let Some(draft) = self.session.draft() else {
            return false;
        };
        let (script, revision) = (draft.text.clone(), draft.revision);
        match self
            .store
            .update_episode(key.episode, &self.title, Some(&script))
        {
            Ok(_) => {
                self.session.saved(key, revision);
                self.status = "已保存到本地".into();
                self.reload();
                true
            }
            Err(e) => {
                self.fail(e);
                false
            }
        }
    }
    fn load_document(&mut self) {
        if self.page == Page::Context {
            if let Some(pid) = self.pid() {
                let result = ContextStore::new(self.root()).get_context(
                    &pid.to_string(),
                    &self.context_kind,
                    "v1",
                );
                match result {
                    Ok(value) => {
                        self.editor = text_editor::Content::with_text(&pretty(
                            &value.unwrap_or_else(|| json!({})),
                        ))
                    }
                    Err(e) => self.fail(e),
                }
            }
        } else if self.page == Page::Prompts {
            match PromptRegistry::new(self.root()).list() {
                Ok(rows) => self.templates = stringify(rows),
                Err(e) => self.fail(e),
            }
            if self.prompt_key.is_empty() {
                self.editor = text_editor::Content::new();
            } else {
                match PromptRegistry::new(self.root()).get(&self.prompt_key) {
                    Ok(template) => self.editor = text_editor::Content::with_text(&template.prompt),
                    Err(e) => self.fail(e),
                }
            }
        } else if self.page == Page::Runs {
            self.editor = text_editor::Content::new();
        }
    }
    fn prepare_input(&mut self, action: String) -> Result<(EpisodeKey, ExecutionInput), String> {
        let project = self.pid().ok_or("请先选择项目")?;
        let independent = action.starts_with("project_outline_") || action == "visual_dna_ingest";
        let key = if independent {
            EpisodeKey {
                project,
                episode: 0,
            }
        } else {
            if !self.save_script() {
                return Err("请先选择并保存一集剧本".into());
            }
            self.session.current.ok_or("请先选择一集剧本")?
        };
        let contexts = ContextStore::new(self.root());
        let mut context = json!({"series_bible":contexts.get_context(&project.to_string(),"series_bible","v1").map_err(|e|e.to_string())?,"artifacts":aicomic_ai::reusable_artifacts(&self.outputs.get(&key).cloned().unwrap_or_else(||json!({})))});
        let script = match action.as_str() {
            "project_outline_generate" => {
                let count: u64 = self
                    .context_episodes
                    .parse()
                    .map_err(|_| "预计集数应为1–500的整数")?;
                if !(1..=500).contains(&count) {
                    return Err("预计集数应为1–500的整数".into());
                }
                context["num_episodes"] = json!(count);
                self.context_summary.text()
            }
            "project_outline_optimize" => {
                context["current_outline"] =
                    serde_json::from_str(&self.editor.text()).unwrap_or_else(|_| json!({}));
                self.context_instruction.clone()
            }
            "visual_dna_ingest" => {
                let item = self.selected_item.ok_or("请先选择资产条目")?;
                let asset = self
                    .selected_visual_asset
                    .and_then(|asset_id| self.media.iter().find(|a| id(a) == asset_id))
                    .ok_or("请先选择图片素材")?;
                context["item_id"] = json!(item);
                context["asset_file_path"] = asset["file_path"].clone();
                format!(
                    "{}\n{}\n{}",
                    self.item_name,
                    self.item_description.text(),
                    self.item_prompt.text()
                )
            }
            _ => self
                .session
                .draft()
                .map(|d| d.text.clone())
                .unwrap_or_default(),
        };
        if script.trim().is_empty() {
            return Err("请填写故事概述、优化指令或剧本内容".into());
        }
        Ok((
            key,
            ExecutionInput {
                project_id: project.to_string(),
                episode_id: (key.episode > 0).then_some(key.episode),
                script,
                context,
                action,
                run_id: uuid::Uuid::new_v4().simple().to_string(),
            },
        ))
    }
    fn filter_events(&mut self) {
        if self.event_sort == "关联数量" {
            self.events.sort_by(|a, b| {
                b["nodes"]
                    .as_array()
                    .map_or(0, Vec::len)
                    .cmp(&a["nodes"].as_array().map_or(0, Vec::len))
                    .then_with(|| label(a, "name").cmp(label(b, "name")))
            });
        } else {
            self.events
                .sort_by(|a, b| label(a, "name").cmp(label(b, "name")));
        }
        let search = self.event_search.to_lowercase();
        self.matrix_events = self
            .events
            .iter()
            .filter(|event| {
                (label(event, "name").to_lowercase().contains(&search)
                    || label(event, "description").to_lowercase().contains(&search))
                    && (self.event_focus.is_empty() || self.event_focus.contains(&id(event)))
            })
            .cloned()
            .collect();
    }
    fn refresh_runs(&mut self) {
        let Some(pid) = self.pid() else {
            return;
        };
        let contexts = ContextStore::new(self.root());
        if let Ok(runs) = contexts.list_runs(&pid.to_string()) {
            self.run_list = stringify(runs);
        }
        if !self.selected_run.is_empty() {
            if let Ok(stages) = contexts.list_stages(&pid.to_string(), &self.selected_run) {
                self.stages = stages.into_iter().map(|stage| stage.name).collect();
            }
            if self.page == Page::Runs {
                let value = if self.selected_stage.is_empty() {
                    contexts.read_run(&pid.to_string(), &self.selected_run)
                } else {
                    contexts.read_stage(&pid.to_string(), &self.selected_run, &self.selected_stage)
                };
                if let Ok(Some(value)) = value {
                    let body = pretty(&value);
                    if self.editor.text() != body {
                        self.editor = text_editor::Content::with_text(&body);
                    }
                }
            }
        }
    }
    fn materialize(&self, key: EpisodeKey, pending: &PendingCommit) -> Result<(), String> {
        if key.episode > 0 {
            self.store
                .apply_workflow_artifacts(
                    key.project,
                    key.episode,
                    &pending.run_id,
                    &pending.artifacts,
                )
                .map_err(|e| e.to_string())?;
        }
        crate::integration::apply_memory(
            &self.store,
            key,
            &pending.run_id,
            &pending.artifacts,
            &pending.workflow_status,
        )?;
        if key.episode > 0 {
            self.store
                .patch_episode(
                    key.episode,
                    EpisodeUpdate {
                        last_exec_run_id: Some(pending.run_id.clone()),
                        exec_status: Some(pending.workflow_status.clone()),
                        exec_artifacts: Some(pending.artifacts.clone()),
                        ..EpisodeUpdate::default()
                    },
                )
                .map_err(|e| e.to_string())?;
        }
        ContextStore::new(self.root()).write_stage(&key.project.to_string(),&pending.run_id,"desktop.materialization",&json!({"status":"done","episode_id":key.episode,"workflow_status":pending.workflow_status})).map_err(|e|e.to_string())
    }
    fn remember_failed_commit(&mut self, key: EpisodeKey, pending: PendingCommit, error: &str) {
        let _=ContextStore::new(self.root()).write_stage(&key.project.to_string(),&pending.run_id,"desktop.materialization",&json!({"status":"error","episode_id":key.episode,"workflow_status":pending.workflow_status,"artifacts":pending.artifacts,"error":error}));
        if key.episode > 0 {
            let _ = self.store.patch_episode(
                key.episode,
                EpisodeUpdate {
                    last_exec_run_id: Some(pending.run_id.clone()),
                    exec_status: Some("apply_failed".into()),
                    exec_artifacts: Some(pending.artifacts.clone()),
                    ..EpisodeUpdate::default()
                },
            );
        }
        self.pending_commits.insert(key, pending);
    }
    fn commit_key(&self) -> Option<EpisodeKey> {
        if self.page == Page::Script {
            self.session.current
        } else {
            self.pid().map(|project| EpisodeKey {
                project,
                episode: 0,
            })
        }
    }

    pub fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Page(page) => {
                self.sync_editor();
                if page == Page::Script {
                    self.editor = text_editor::Content::with_text(&if self.step == 0 {
                        self.session
                            .draft()
                            .map(|d| d.text.clone())
                            .unwrap_or_default()
                    } else {
                        self.output_text()
                    });
                }
                self.page = page;
                self.load_document();
            }
            Message::Project(choice) => self.select_project(choice),
            Message::NewName(name) => self.new_name = name,
            Message::NewDescription(description) => self.new_description = description,
            Message::CreateProject => {
                if !self.new_name.trim().is_empty() {
                    match self.store.create_project(
                        self.new_name.trim(),
                        Some(self.new_description.trim()).filter(|s| !s.is_empty()),
                    ) {
                        Ok(project) => {
                            self.new_name.clear();
                            self.new_description.clear();
                            self.reload_projects();
                            self.select_project(Choice {
                                id: project.id,
                                name: project.name,
                            });
                        }
                        Err(e) => self.fail(e),
                    }
                }
            }
            Message::DeleteProject => self.pending_delete = self.pid().map(|id| (true, id)),
            Message::CreateEpisode => {
                if let Some(pid) = self.pid() {
                    match self.store.create_episode(
                        pid,
                        &format!("第 {} 集", self.episodes.len() + 1),
                        None,
                    ) {
                        Ok(episode) => {
                            self.reload();
                            self.select_episode(episode.id, 0);
                        }
                        Err(e) => self.fail(e),
                    }
                }
            }
            Message::DeleteEpisode(id) => self.pending_delete = Some((false, id)),
            Message::ConfirmDelete(confirm) => {
                if let Some((project, target)) = self.pending_delete.take()
                    && confirm
                {
                    if self.busy.as_ref().is_some_and(|(key, _, _)| {
                        if project {
                            key.project == target
                        } else {
                            key.episode == target
                        }
                    }) {
                        let _ = self.update(Message::Cancel);
                    }
                    let result = if project {
                        self.store.delete_project(target)
                    } else {
                        self.store.delete_episode(target)
                    };
                    match result {
                        Ok(_) => {
                            self.session.drafts.retain(|key, _| {
                                if project {
                                    key.project != target
                                } else {
                                    key.episode != target
                                }
                            });
                            self.outputs.retain(|key, _| {
                                if project {
                                    key.project != target
                                } else {
                                    key.episode != target
                                }
                            });
                            self.pending_commits.retain(|key, _| {
                                if project {
                                    key.project != target
                                } else {
                                    key.episode != target
                                }
                            });
                            self.paused = None;
                            self.session.current = None;
                            self.media.clear();
                            self.memory_records.clear();
                            self.changesets.clear();
                            self.conflicts.clear();
                            self.events.clear();
                            self.matrix_events.clear();
                            self.items.clear();
                            self.scenes.clear();
                            self.reload_projects();
                            if let Some(choice) = self.projects.first().cloned() {
                                self.select_project(choice);
                            } else {
                                self.project = None;
                                self.episodes.clear();
                            }
                        }
                        Err(e) => self.fail(e),
                    }
                }
            }
            Message::Episode(id, step) => self.select_episode(id, step),
            Message::Title(title) => {
                self.session.rename(title.clone());
                self.title = title;
            }
            Message::Edit(action) => {
                self.editor.perform(action);
                self.sync_editor();
            }
            Message::Save => match self.page {
                Page::Script => {
                    self.save_script();
                }
                Page::Context => return self.update(Message::SaveContext),
                Page::Prompts => return self.update(Message::SavePrompt),
                Page::Assets => return self.update(Message::SaveItem),
                Page::Events => return self.update(Message::SaveEventMetadata),
                _ => {}
            },
            Message::Run(action) => {
                if self.busy.is_some() {
                    return Task::none();
                }
                let (key, mut input) = match self.prepare_input(action) {
                    Ok(input) => input,
                    Err(e) => {
                        self.fail(e);
                        return Task::none();
                    }
                };
                if self.pending_commits.contains_key(&key) {
                    self.fail("请先重试写入当前运行结果");
                    return Task::none();
                }
                let run_id = input.run_id.clone();
                let engine = WorkflowEngine::new(self.root(), self.settings.clone());
                let task_id = run_id.clone();
                if key.episode > 0
                    && let Err(e) = self.store.patch_episode(
                        key.episode,
                        EpisodeUpdate {
                            last_exec_run_id: Some(run_id.clone()),
                            exec_status: Some("running".into()),
                            ..EpisodeUpdate::default()
                        },
                    )
                {
                    self.fail(e);
                    return Task::none();
                }
                let memory = self.memory_service.clone();
                let evidence_store = self.store.clone();
                let evidence_text = input.script.clone();
                let (task, handle) = Task::perform(
                    async move {
                        let evidence = tokio::task::spawn_blocking(move || {
                            let ids = crate::integration::prepare_memory(
                                &evidence_store,
                                key,
                                &evidence_text,
                            )?;
                            let quotes =
                                crate::integration::source_evidences(&evidence_store, key, &ids)?;
                            Ok::<_, String>((ids, quotes))
                        })
                        .await
                        .map_err(|e| e.to_string())??;
                        input.context["source_evidence_ids"] = json!(evidence.0);
                        input.context["source_evidences"] = evidence.1;
                        let retrieved = memory
                            .task_context(key.project, input.script.clone(), None)
                            .await
                            .map_err(|e| e.to_string())?;
                        input.context["retrieved_memory"] =
                            serde_json::to_value(retrieved).map_err(|e| e.to_string())?;
                        engine.execute(input).await.map_err(|e| e.to_string())
                    },
                    move |result| Message::RunComplete(key, task_id.clone(), result),
                )
                .abortable();
                self.status = "AI 正在生成，运行过程会保存在本地…".into();
                self.busy = Some((key, run_id, handle));
                return task;
            }
            Message::RunComplete(key, run_id, result) => {
                if !self.busy.as_ref().is_some_and(|(active_key, running, _)| {
                    *active_key == key && running == &run_id
                }) {
                    return Task::none();
                }
                self.busy = None;
                match result {
                    Ok(result) => {
                        self.outputs.insert(key, result.artifacts.clone());
                        let pending = PendingCommit {
                            run_id: run_id.clone(),
                            workflow_status: result.status.clone(),
                            artifacts: result.artifacts.clone(),
                        };
                        let applied = self.materialize(key, &pending);
                        if result.status == "paused" {
                            self.paused = Some((key, run_id.clone()));
                            self.status =
                                format!("等待确认：{}", result.waiting_for.unwrap_or_default());
                        } else {
                            self.paused = None;
                            self.status = if result.status == "rejected" {
                                "本次结果已拒绝，运行记录已保存".into()
                            } else {
                                "生成完成，原始输出与阶段记录已保存".into()
                            };
                        }
                        if let Err(e) = applied {
                            self.fail(format!("生成结果已保留，写入失败，可重试写入：{e}"));
                            self.remember_failed_commit(key, pending, &e);
                        } else {
                            self.pending_commits.remove(&key);
                        }
                        if self.session.current == Some(key) && self.page == Page::Script {
                            self.editor =
                                text_editor::Content::with_text(&pretty(&result.artifacts));
                            self.step = if result.artifacts["assets_visual_dna"].is_object() {
                                2
                            } else {
                                1
                            };
                        }
                        self.reload();
                        if self.pid() == Some(key.project) && self.page == Page::Context {
                            if result.artifacts["project_outline"].is_object() {
                                self.context_kind = "project_outline".into();
                            }
                            self.load_document();
                        }
                    }
                    Err(error) => {
                        let _ = self.store.patch_episode(
                            key.episode,
                            EpisodeUpdate {
                                exec_status: Some("error".into()),
                                ..EpisodeUpdate::default()
                            },
                        );
                        self.fail(error);
                    }
                }
                self.refresh_runs();
            }
            Message::Cancel => {
                self.paused = None;
                if let Some((key, run, handle)) = self.busy.take() {
                    handle.abort();
                    let _ = ContextStore::new(self.root()).write_stage(
                        &key.project.to_string(),
                        &run,
                        "chat.status",
                        &json!({"status":"cancelled"}),
                    );
                    let _ = self.store.patch_episode(
                        key.episode,
                        EpisodeUpdate {
                            exec_status: Some("cancelled".into()),
                            ..EpisodeUpdate::default()
                        },
                    );
                    self.status = "任务已取消；已保存阶段仍可查看".into();
                }
                self.refresh_runs();
            }
            Message::Review(approve) => {
                if self.busy.is_some() {
                    return Task::none();
                }
                if let Some((key, run)) = self.paused.clone() {
                    if self.pending_commits.contains_key(&key) {
                        self.fail("请先重试写入已生成结果，再继续审核");
                        return Task::none();
                    }
                    if self.session.current != Some(key) {
                        return Task::none();
                    }
                    let engine = WorkflowEngine::new(self.root(), self.settings.clone());
                    let task_run = run.clone();
                    let (task, handle) = Task::perform(
                        async move {
                            engine
                                .resume(
                                    &key.project.to_string(),
                                    &run,
                                    if approve {
                                        ReviewDecision::Approve
                                    } else {
                                        ReviewDecision::Reject
                                    },
                                    None,
                                )
                                .await
                                .map_err(|e| e.to_string())
                        },
                        move |result| Message::RunComplete(key, task_run.clone(), result),
                    )
                    .abortable();
                    self.busy = Some((key, self.paused.as_ref().unwrap().1.clone(), handle));
                    return task;
                }
            }
            Message::Board(view) => self.board = view,
            Message::Tick => {
                self.ticks += 1;
                if self.busy.is_some() {
                    self.refresh_runs();
                }
                if self.options.smoke && self.ticks == 2 {
                    eprintln!(
                        "smoke: editor_chars={}, draft_chars={}",
                        self.editor.text().chars().count(),
                        self.session
                            .draft()
                            .map(|d| d.text.chars().count())
                            .unwrap_or(0)
                    );
                }
                if let Some((key, run, _)) = &self.busy
                    && let Ok(Some(stage)) = ContextStore::new(self.root()).read_stage(
                        &key.project.to_string(),
                        run,
                        "chat.status",
                    )
                {
                    self.status = format!("AI 执行中 · {}", label(&stage, "status"));
                }
                if self.options.smoke && self.ticks == 6 {
                    return iced::window::oldest()
                        .and_then(iced::window::screenshot)
                        .map(Message::Snapshot);
                }
            }
            Message::Snapshot(snapshot) => {
                if let Some(path) = &self.options.screenshot {
                    let result = (|| -> Result<(), Box<dyn std::error::Error>> {
                        let file = std::fs::File::create(path)?;
                        let mut encoder = png::Encoder::new(
                            std::io::BufWriter::new(file),
                            snapshot.size.width,
                            snapshot.size.height,
                        );
                        encoder.set_color(png::ColorType::Rgba);
                        encoder.set_depth(png::BitDepth::Eight);
                        encoder.write_header()?.write_image_data(&snapshot.rgba)?;
                        Ok(())
                    })();
                    if let Err(e) = result {
                        eprintln!("截图失败：{e}");
                    }
                }
                return iced::exit();
            }
            Message::Close => {
                if !self.save_all() {
                    return Task::none();
                }
                if self.busy.is_some() {
                    let _ = self.update(Message::Cancel);
                }
                return iced::exit();
            }
            Message::Provider(provider) => {
                if self.settings.provider != provider {
                    self.settings.provider = provider;
                    self.settings.base_url = provider.default_base_url().into();
                    self.settings.model = provider.default_model().into();
                    self.settings.api_key.clear();
                    self.status = format!("已切换至 {provider}，请重新输入 API Key 并保存设置");
                }
            }
            Message::Setting(field, value) => match field {
                "api_key" => self.settings.api_key = value,
                "base_url" => self.settings.base_url = value,
                "model" => self.settings.model = value,
                "tokens" => self.settings_tokens = value,
                "timeout" => self.settings_timeout = value,
                "temperature" => self.settings_temperature = value,
                _ => {}
            },
            Message::SaveSettings => {
                let result = (|| -> Result<(), String> {
                    self.settings.max_tokens = if self.settings_tokens.trim().is_empty() {
                        None
                    } else {
                        Some(
                            self.settings_tokens
                                .parse()
                                .map_err(|_| "输出上限应为正整数")?,
                        )
                    };
                    self.settings.timeout_seconds =
                        self.settings_timeout.parse().map_err(|_| "超时应为数字")?;
                    self.settings.temperature = self
                        .settings_temperature
                        .parse()
                        .map_err(|_| "温度应为数字")?;
                    self.settings.save(&self.root()).map_err(|e| e.to_string())
                })();
                match result {
                    Ok(_) => self.status = "AI 设置已保存".into(),
                    Err(e) => self.fail(e),
                }
            }
            Message::TestAi => {
                let engine = WorkflowEngine::new(self.root(), self.settings.clone());
                return Task::perform(
                    async move { engine.test_connection().await.map_err(|e| e.to_string()) },
                    Message::AiTest,
                );
            }
            Message::AiTest(result) => match result {
                Ok(detail) => self.status = detail,
                Err(e) => self.fail(e),
            },
            Message::SelectPrompt(key) => match PromptRegistry::new(self.root()).get(&key) {
                Ok(template) => {
                    let v = serde_json::to_value(template).unwrap_or_default();
                    self.prompt_key = key;
                    self.prompt_title = label(&v, "title").into();
                    self.prompt_category = label(&v, "category").into();
                    self.editor = text_editor::Content::with_text(label(&v, "prompt"));
                }
                Err(e) => self.fail(e),
            },
            Message::PromptField(field, value) => match field {
                "key" => self.prompt_key = value,
                "title" => self.prompt_title = value,
                "category" => self.prompt_category = value,
                _ => {}
            },
            Message::SavePrompt => {
                let result = PromptRegistry::new(self.root()).upsert(
                    &self.prompt_key,
                    &self.prompt_title,
                    &self.prompt_category,
                    &self.editor.text(),
                );
                match result {
                    Ok(_) => {
                        self.status = "模板已保存".into();
                        self.load_document();
                    }
                    Err(e) => self.fail(e),
                }
            }
            Message::ResetPrompt => {
                match PromptRegistry::new(self.root()).reset(&self.prompt_key) {
                    Ok(template) => {
                        self.editor = text_editor::Content::with_text(&template.prompt);
                        self.status = "已恢复内置模板".into();
                    }
                    Err(e) => self.fail(e),
                }
            }
            Message::DeletePrompt => {
                match PromptRegistry::new(self.root()).delete(&self.prompt_key) {
                    Ok(_) => {
                        self.prompt_key.clear();
                        self.editor = text_editor::Content::new();
                        self.load_document();
                    }
                    Err(e) => self.fail(e),
                }
            }
            Message::ContextKind(kind) => {
                self.context_kind = kind;
                self.load_document();
            }
            Message::SaveContext => {
                if let Some(pid) = self.pid() {
                    match serde_json::from_str::<Value>(&self.editor.text()) {
                        Ok(value) => match ContextStore::new(self.root()).put_context(
                            &pid.to_string(),
                            &self.context_kind,
                            "v1",
                            &value,
                        ) {
                            Ok(_) => self.status = "项目上下文已保存".into(),
                            Err(e) => self.fail(e),
                        },
                        Err(e) => self.fail(format!("JSON 格式错误：{e}")),
                    }
                }
            }
            Message::SelectRun(run) => {
                self.selected_run = run;
                self.selected_stage.clear();
                self.page = Page::Runs;
                self.refresh_runs();
            }
            Message::SelectStage(stage) => {
                self.selected_stage = stage.clone();
                if let Some(pid) = self.pid()
                    && let Ok(Some(value)) = ContextStore::new(self.root()).read_stage(
                        &pid.to_string(),
                        &self.selected_run,
                        &stage,
                    )
                {
                    self.editor = text_editor::Content::with_text(&pretty(&value));
                }
            }
            Message::SelectItem(item) => {
                self.selected_item = Some(item);
                self.selected_visual_asset = None;
                if let Some(value) = self.items.iter().find(|v| id(v) == item) {
                    self.item_name = label(value, "name").into();
                    self.item_category = label(value, "category").into();
                    self.item_description =
                        text_editor::Content::with_text(label(value, "description"));
                    self.item_prompt = text_editor::Content::with_text(label(value, "base_prompt"));
                }
                match self.store.assets_for_item(item) {
                    Ok(rows) => self.media = stringify(rows),
                    Err(e) => self.fail(e),
                }
            }
            Message::Category(category) => {
                self.category = category;
                self.selected_item = None;
                self.media.clear();
            }
            Message::CreateItem => {
                if let Some(pid) = self.pid()
                    && !self.new_name.trim().is_empty()
                {
                    match self
                        .store
                        .create_asset_item(pid, &self.new_name, &self.category, None)
                    {
                        Ok(item) => {
                            self.selected_item = Some(item.id);
                            self.new_name.clear();
                            self.reload();
                            return self.update(Message::SelectItem(item.id));
                        }
                        Err(e) => self.fail(e),
                    }
                }
            }
            Message::DeleteItem(item) => {
                if self.pid().is_some_and(|pid| {
                    self.busy
                        .as_ref()
                        .is_some_and(|(key, _, _)| key.project == pid && key.episode == 0)
                        || self.pending_commits.contains_key(&EpisodeKey {
                            project: pid,
                            episode: 0,
                        })
                }) {
                    self.fail("项目任务执行中或结果待写入，请先完成或取消任务后删除资产");
                    return Task::none();
                }
                match self.store.delete_asset_item(item) {
                    Ok(_) => {
                        self.selected_item = None;
                        self.media.clear();
                        self.reload();
                    }
                    Err(e) => self.fail(e),
                }
            }
            Message::ImportMedia => {
                if let Some(item) = self.selected_item {
                    let store = self.store.clone();
                    return Task::perform(
                        async move {
                            let paths = rfd::AsyncFileDialog::new()
                                .set_title("导入素材")
                                .pick_files()
                                .await;
                            if let Some(paths) = paths {
                                for path in paths {
                                    store
                                        .import_asset(path.path(), AssetTarget::Item(item))
                                        .map_err(|e| e.to_string())?;
                                }
                            }
                            Ok(())
                        },
                        Message::Imported,
                    );
                }
            }
            Message::Imported(result) => match result {
                Ok(_) => {
                    self.status = "素材已导入".into();
                    if let Some(item) = self.selected_item {
                        return self.update(Message::SelectItem(item));
                    }
                }
                Err(e) => self.fail(e),
            },
            Message::OpenMedia(path) => {
                if let Err(e) = open::that_detached(path) {
                    self.fail(e);
                }
            }
            Message::DeleteAsset(asset) => match self.store.delete_asset(asset) {
                Ok(_) => {
                    if let Some(item) = self.selected_item {
                        return self.update(Message::SelectItem(item));
                    }
                }
                Err(e) => self.fail(e),
            },
            Message::CreateEvent => {
                if let Some(pid) = self.pid()
                    && !self.new_name.trim().is_empty()
                {
                    match self.store.create_event(pid, &self.new_name, "#6366F1") {
                        Ok(_) => {
                            self.new_name.clear();
                            self.reload();
                        }
                        Err(e) => self.fail(e),
                    }
                }
            }
            Message::DeleteEvent(event) => match self.store.delete_event(event) {
                Ok(_) => {
                    self.selected_event = None;
                    self.reload();
                }
                Err(e) => self.fail(e),
            },
            Message::SelectEvent(event) => {
                self.selected_event = Some(event);
                if let Some(value) = self.events.iter().find(|v| id(v) == event) {
                    self.event_name = label(value, "name").into();
                    self.event_description = label(value, "description").into();
                    self.event_color = label(value, "color").into();
                }
                self.linked_scenes = self
                    .events
                    .iter()
                    .find(|e| id(e) == event)
                    .and_then(|e| e["nodes"].as_array())
                    .map(|nodes| {
                        nodes
                            .iter()
                            .filter(|n| label(n, "target_type") == "scene")
                            .filter_map(|n| n["target_id"].as_i64())
                            .collect()
                    })
                    .unwrap_or_default();
            }
            Message::ToggleScene(scene) => {
                if self.linked_scenes.contains(&scene) {
                    self.linked_scenes.retain(|s| s != &scene);
                } else {
                    self.linked_scenes.push(scene);
                }
            }
            Message::SaveEvent => {
                if let Some(event) = self.selected_event {
                    let to_remove = self
                        .events
                        .iter()
                        .find(|e| id(e) == event)
                        .and_then(|e| e["nodes"].as_array())
                        .map(|nodes| {
                            nodes
                                .iter()
                                .filter(|n| {
                                    label(n, "target_type") == "scene"
                                        && !self
                                            .linked_scenes
                                            .contains(&n["target_id"].as_i64().unwrap_or(0))
                                })
                                .map(id)
                                .collect::<Vec<_>>()
                        })
                        .unwrap_or_default();
                    for node in to_remove {
                        if let Err(e) = self.store.delete_event_node(node) {
                            self.fail(e);
                            return Task::none();
                        }
                    }
                    let linked = self.linked_scenes.clone();
                    for scene in linked {
                        if let Err(e) = self.store.upsert_event_node(event, "scene", scene, "") {
                            self.fail(e);
                            return Task::none();
                        }
                    }
                    self.status = "事件场景关联已保存".into();
                    self.reload();
                }
            }
            Message::MemorySearch(query) => {
                self.memory_request += 1;
                self.memory_search = query;
                self.reload();
            }
            Message::Refresh => self.reload(),
            Message::ReviewChangeset(changeset, approve) => {
                match MemoryStore::open_root(&self.root()) {
                    Ok(memory) => {
                        let result = if approve {
                            memory.apply_changeset(&changeset, "desktop", None)
                        } else {
                            memory.reject_changeset(&changeset, "desktop", None)
                        };
                        match result {
                            Ok(_) => {
                                self.status = "审核已保存".into();
                                self.reload();
                            }
                            Err(e) => self.fail(e),
                        }
                    }
                    Err(e) => self.fail(e),
                }
            }
            Message::ResolveConflict(conflict) => match MemoryStore::open_root(&self.root()) {
                Ok(memory) => {
                    match memory.resolve_conflict(&conflict, "desktop", Some("用户在桌面端裁决"))
                    {
                        Ok(_) => self.reload(),
                        Err(e) => self.fail(e),
                    }
                }
                Err(e) => self.fail(e),
            },
            Message::ApplyScript => {
                if let Some(script) = self
                    .session
                    .current
                    .and_then(|key| self.outputs.get(&key))
                    .and_then(|a| a["script"].as_str())
                    .map(str::to_owned)
                {
                    self.session.edit(script.clone());
                    self.step = 0;
                    self.editor = text_editor::Content::with_text(&script);
                    self.status = "生成剧本已放入编辑器，请检查并保存".into();
                }
            }
            Message::ApplyArtifacts => {
                if self.busy.is_some() {
                    return Task::none();
                }
                if let Some(key) = self.commit_key()
                    && let Some(pending) = self.pending_commits.get(&key).cloned()
                {
                    match self.materialize(key, &pending) {
                        Ok(()) => {
                            self.pending_commits.remove(&key);
                            self.status = "运行结果已完整写入".into();
                            self.reload();
                        }
                        Err(e) => {
                            self.remember_failed_commit(key, pending, &e);
                            self.fail(e);
                        }
                    }
                }
            }
            Message::Embedding(field, value) => match field {
                "mode" => self.embedding.mode = value,
                "dir" => self.embedding.model_dir = value,
                "url" => self.embedding.base_url = value,
                "key" => self.embedding.api_key = value,
                "model" => self.embedding.model = value,
                "max" => self.embedding_max = value,
                _ => {}
            },
            Message::SaveEmbedding => {
                match self.embedding_max.parse() {
                    Ok(n) => self.embedding.max_tokens = n,
                    Err(_) => {
                        self.fail("输入长度应为正整数");
                        return Task::none();
                    }
                }
                match self.embedding.save(&self.root()) {
                    Ok(()) => {
                        self.memory_service =
                            MemoryService::new(self.root(), self.embedding.clone());
                        self.status = "记忆检索设置已保存".into();
                    }
                    Err(e) => self.fail(e),
                }
            }
            Message::RebuildMemory => {
                if let Some(pid) = self.pid() {
                    let service = self.memory_service.clone();
                    self.status = "正在重建语义索引…".into();
                    return Task::perform(
                        async move { service.rebuild(pid).await.map_err(|e| e.to_string()) },
                        Message::MemoryRebuilt,
                    );
                }
            }
            Message::MemoryRebuilt(result) => match result {
                Ok(count) => self.status = format!("索引已重建，共 {count} 条记忆"),
                Err(e) => self.fail(e),
            },
            Message::SemanticSearch => {
                if let Some(pid) = self.pid() {
                    let service = self.memory_service.clone();
                    let query = self.memory_search.clone();
                    self.memory_request += 1;
                    let request = self.memory_request;
                    return Task::perform(
                        async move {
                            service
                                .semantic_search(pid, query, 30)
                                .await
                                .map(stringify)
                                .map_err(|e| e.to_string())
                        },
                        move |result| Message::MemoryFound(pid, request, result),
                    );
                }
            }
            Message::MemoryFound(pid, request, result) => {
                if self.pid() != Some(pid) || self.memory_request != request {
                    return Task::none();
                }
                match result {
                    Ok(rows) => {
                        self.memory_records = rows;
                        self.status = "语义检索完成".into();
                    }
                    Err(e) => self.fail(e),
                }
            }
            Message::Matrix(message) => match message {
                MatrixMessage::View(view) => self.matrix_view = view,
                MatrixMessage::Select(event) => return self.update(Message::SelectEvent(event)),
            },
            Message::ContextSummary(action) => self.context_summary.perform(action),
            Message::ContextField(field, value) => match field {
                "instruction" => self.context_instruction = value,
                "episodes" => self.context_episodes = value,
                _ => {}
            },
            Message::ContextVisual(item) => {
                self.context_kind = format!("visual_dna.asset_item_{}", item.id);
                self.load_document();
            }
            Message::ItemField(field, value) => match field {
                "name" => self.item_name = value,
                "category" => self.item_category = value,
                _ => {}
            },
            Message::ItemDescription(action) => self.item_description.perform(action),
            Message::ItemPrompt(action) => self.item_prompt.perform(action),
            Message::SaveItem => {
                if let Some(item) = self.selected_item {
                    match self.store.update_asset_item(
                        item,
                        AssetItemUpdate {
                            name: Some(self.item_name.clone()),
                            description: Some(self.item_description.text()),
                            base_prompt: Some(self.item_prompt.text()),
                            category: Some(self.item_category.clone()),
                            ..AssetItemUpdate::default()
                        },
                    ) {
                        Ok(_) => {
                            self.reload();
                            self.status = "资产条目已保存".into();
                        }
                        Err(e) => self.fail(e),
                    }
                }
            }
            Message::VisualAsset(asset) => {
                self.selected_visual_asset = Some(asset);
                return self.update(Message::Run("visual_dna_ingest".into()));
            }
            Message::EventField(field, value) => match field {
                "name" => self.event_name = value,
                "description" => self.event_description = value,
                "color" => self.event_color = value,
                _ => {}
            },
            Message::EventFilter(value) => {
                self.event_search = value;
                self.filter_events();
            }
            Message::EventFocus(event) => {
                if self.event_focus.contains(&event) {
                    self.event_focus.retain(|id| *id != event);
                } else {
                    self.event_focus.push(event);
                }
                self.filter_events();
            }
            Message::EventSort(sort) => {
                self.event_sort = sort;
                self.filter_events();
            }
            Message::SaveEventMetadata => {
                if let Some(event) = self.selected_event {
                    match self.store.update_event(
                        event,
                        EventUpdate {
                            name: Some(self.event_name.clone()),
                            description: Some(self.event_description.clone()),
                            color: Some(self.event_color.clone()),
                            ..EventUpdate::default()
                        },
                    ) {
                        Ok(_) => {
                            self.reload();
                            self.status = "事件已保存".into();
                        }
                        Err(e) => self.fail(e),
                    }
                }
            }
            Message::PromptSearch(value) => self.prompt_search = value,
            Message::Inspect(action) => {
                if !action.is_edit() {
                    self.editor.perform(action);
                }
            }
        }
        Task::none()
    }

    fn save_all(&mut self) -> bool {
        self.sync_editor();
        let drafts: Vec<_> = self
            .session
            .drafts
            .iter()
            .filter(|(_, d)| d.dirty)
            .map(|(k, d)| (*k, d.clone()))
            .collect();
        for (key, draft) in drafts {
            let title = draft.title.clone().unwrap_or_else(|| {
                if self.session.current == Some(key) {
                    self.title.clone()
                } else {
                    self.store
                        .episodes(key.project)
                        .ok()
                        .and_then(|eps| eps.into_iter().find(|e| e.id == key.episode))
                        .map(|e| e.title)
                        .unwrap_or_default()
                }
            });
            if let Err(e) = self
                .store
                .update_episode(key.episode, &title, Some(&draft.text))
            {
                self.fail(e);
                return false;
            }
            self.session.saved(key, draft.revision);
        }
        true
    }

    pub fn view(&self) -> Element<'_, Message> {
        let mut nav = row![
            text("星晴").size(24),
            small("AI 漫剧创作"),
            space::horizontal()
        ]
        .spacing(14)
        .align_y(iced::Center);
        for page in Page::ALL {
            nav = nav.push(
                button(text(page.label()).size(13))
                    .style(if page == self.page {
                        button::primary
                    } else {
                        button::text
                    })
                    .on_press(Message::Page(page)),
            );
        }
        let header = container(nav)
            .padding([15, 24])
            .style(container::bordered_box);
        let content: Element<'_, Message> = match self.page {
            Page::Script => self.script_view(),
            Page::Ai => self.settings_view(),
            Page::Prompts => self.prompts_view(),
            Page::Context => self.context_view(),
            Page::Runs => self.runs_view(),
            Page::Assets => self.assets_view(),
            Page::Events => self.events_view(),
            Page::Memory => self.memory_view(),
        };
        let mut footer = row![
            small(self.status.clone()),
            space::horizontal(),
            small("Rust · 本地工作区")
        ]
        .spacing(12);
        if self.busy.is_none()
            && self
                .commit_key()
                .is_some_and(|key| self.pending_commits.contains_key(&key))
        {
            footer = footer.push(button("重试写入运行结果").on_press(Message::ApplyArtifacts));
        }
        let mut body = column![
            header,
            container(content).height(Fill).padding(16),
            container(footer).padding([8, 24])
        ]
        .spacing(0);
        if let Some((project, _)) = self.pending_delete {
            body = body.push(
                container(
                    row![
                        text(if project {
                            "确认删除此项目及其内容？"
                        } else {
                            "确认删除本集及其内容？"
                        }),
                        button("取消").on_press(Message::ConfirmDelete(false)),
                        button("确认删除")
                            .style(button::danger)
                            .on_press(Message::ConfirmDelete(true))
                    ]
                    .spacing(12),
                )
                .padding(16)
                .style(container::bordered_box),
            );
        }
        crate::shortcuts::save(body, Message::Save)
    }
    fn project_picker(&self) -> Element<'_, Message> {
        pick_list(
            self.projects.as_slice(),
            self.project.clone(),
            Message::Project,
        )
        .placeholder("选择项目")
        .width(Fill)
        .into()
    }
    fn script_view(&self) -> Element<'_, Message> {
        let mut tree = column![
            small("创作空间"),
            self.project_picker(),
            row![
                text_input("新项目名称", &self.new_name).on_input(Message::NewName),
                button("+").on_press(Message::CreateProject)
            ]
            .spacing(5),
            text_input("项目简介（选填）", &self.new_description).on_input(Message::NewDescription),
            row![
                button("新增集").on_press(Message::CreateEpisode),
                button("删除项目")
                    .style(button::text)
                    .on_press(Message::DeleteProject)
            ]
            .spacing(8),
            small("剧本目录")
        ]
        .spacing(14);
        for episode in &self.episodes {
            let episode_id = id(episode);
            let mut entry = column![row![
                button(
                    text(format!(
                        "EP{:02}  {}",
                        episode["order"].as_i64().unwrap_or(0),
                        label(episode, "title")
                    ))
                    .size(13)
                )
                .style(button::text)
                .width(Fill)
                .on_press(Message::Episode(episode_id, 0)),
                button("×")
                    .style(button::text)
                    .on_press(Message::DeleteEpisode(episode_id))
            ]]
            .spacing(3);
            if self
                .session
                .current
                .is_some_and(|key| key.episode == episode_id)
            {
                for (step, label) in [(0, "原始剧本"), (1, "结构化拆解"), (2, "资产提取")]
                {
                    entry = entry.push(
                        button(text(format!("  {}  {label}", step)).size(12))
                            .style(if self.step == step {
                                button::primary
                            } else {
                                button::text
                            })
                            .width(Fill)
                            .on_press(Message::Episode(episode_id, step)),
                    );
                }
            }
            tree = tree.push(entry);
        }
        let sidebar = container(scrollable(tree))
            .width(Length::Fixed(260.0))
            .height(Fill)
            .padding(16)
            .style(container::bordered_box);
        if self.session.current.is_none() {
            return row![
                sidebar,
                container(
                    column![
                        text("让故事，从这里开始").size(30),
                        text("创建项目和集数，写下你的第一段剧本。"),
                        small("原始剧本  →  结构化拆解  →  资产提取")
                    ]
                    .spacing(20)
                )
                .center(Fill)
            ]
            .spacing(16)
            .into();
        }
        let tabs = row![
            button("01  原始剧本")
                .style(if self.step == 0 {
                    button::primary
                } else {
                    button::secondary
                })
                .on_press(Message::Episode(self.session.current.unwrap().episode, 0)),
            button("02  结构化拆解")
                .on_press(Message::Episode(self.session.current.unwrap().episode, 1)),
            button("03  资产提取")
                .on_press(Message::Episode(self.session.current.unwrap().episode, 2)),
            space::horizontal(),
            button("保存").on_press(Message::Save)
        ]
        .spacing(8);
        let mut actions = row![
            small(if self.busy.is_some() {
                "正在执行…"
            } else {
                "选择创作动作"
            }),
            space::horizontal()
        ]
        .spacing(8);
        if self.busy.is_some() {
            actions = actions.push(
                button("取消执行")
                    .style(button::danger)
                    .on_press(Message::Cancel),
            );
            if let Some((key, run, _)) = &self.busy
                && self.pid() == Some(key.project)
            {
                actions =
                    actions.push(button("查看运行过程").on_press(Message::SelectRun(run.clone())));
            }
        } else {
            actions = actions
                .push(button("优化剧本").on_press(Message::Run("script_optimize".into())))
                .push(button("结构拆解").on_press(Message::Run("episode_outline_generate".into())))
                .push(button("提取资产").on_press(Message::Run("episode_assets_visual_dna".into())))
                .push(button("完整执行 →").on_press(Message::Run("episode_execute".into())));
        }
        let mut work = column![
            row![
                text_input("集标题", &self.title)
                    .on_input(Message::Title)
                    .size(22),
                small(if self.session.draft().is_some_and(|d| d.dirty) {
                    "未保存"
                } else {
                    "已保存"
                })
            ]
            .spacing(12)
            .align_y(iced::Center),
            tabs,
            actions
        ]
        .spacing(16);
        if self
            .paused
            .as_ref()
            .is_some_and(|(key, _)| self.session.current == Some(*key))
            && self.busy.is_none()
        {
            work = work.push(
                row![
                    text("生成已暂停，请审核后继续"),
                    button("确认继续").on_press(Message::Review(true)),
                    button("拒绝").on_press(Message::Review(false))
                ]
                .spacing(12),
            );
        }
        if self
            .session
            .current
            .and_then(|k| self.outputs.get(&k))
            .is_some_and(|a| a["script"].is_string())
        {
            work = work.push(button("应用生成剧本").on_press(Message::ApplyScript));
        }
        if self.step == 0 {
            work = work.push(
                text_editor(&self.editor)
                    .placeholder("在此输入原始剧本…")
                    .height(Fill)
                    .on_action(Message::Edit)
                    .size(15)
                    .padding(20),
            );
        } else {
            let cards = self.cards();
            work = work.push(
                row![
                    iced::widget::canvas(Board {
                        cards,
                        view: self.board.clone()
                    })
                    .width(Fill)
                    .height(Fill),
                    container(
                        text_editor(&self.editor)
                            .size(12)
                            .height(Fill)
                            .on_action(Message::Inspect)
                    )
                    .width(Length::Fixed(330.0))
                    .height(Fill)
                    .padding(14)
                    .style(container::bordered_box)
                ]
                .spacing(12),
            );
        }
        row![
            sidebar,
            container(work)
                .width(Fill)
                .height(Fill)
                .padding(20)
                .style(container::bordered_box)
        ]
        .spacing(16)
        .into()
    }
    fn cards(&self) -> Vec<BoardCard> {
        let Some(artifacts) = self.session.current.and_then(|k| self.outputs.get(&k)) else {
            return vec![];
        };
        let value = if self.step == 2 && artifacts["assets_visual_dna"].is_object() {
            &artifacts["assets_visual_dna"]
        } else if self.step == 1 && !artifacts["outline"].is_null() {
            &artifacts["outline"]
        } else {
            artifacts
        };
        let mut cards = vec![];
        fn visit(value: &Value, heading: &str, cards: &mut Vec<BoardCard>) {
            if cards.len() >= 100 {
                return;
            }
            match value {
                Value::Array(items) => {
                    for (i, item) in items.iter().enumerate() {
                        match item {
                            Value::Object(_) => cards.push(BoardCard {
                                title: item["title"]
                                    .as_str()
                                    .or(item["name"].as_str())
                                    .map(str::to_owned)
                                    .unwrap_or_else(|| format!("{heading} {}", i + 1)),
                                body: pretty(item),
                            }),
                            _ => visit(item, heading, cards),
                        }
                    }
                }
                Value::Object(fields) => {
                    for (key, item) in fields {
                        visit(item, key, cards);
                    }
                }
                _ => {}
            }
        }
        if self.step == 2 && value["series_style"].is_object() {
            cards.push(BoardCard {
                title: "全局视觉风格".into(),
                body: pretty(&value["series_style"]),
            });
        }
        visit(value, "内容", &mut cards);
        if cards.is_empty() {
            cards.push(BoardCard {
                title: "生成结果".into(),
                body: pretty(value),
            });
        }
        cards
    }
    fn settings_view(&self) -> Element<'_, Message> {
        let field = |name: &'static str, value: &str, key: &'static str| {
            column![
                small(name),
                text_input(name, value).on_input(move |v| Message::Setting(key, v))
            ]
            .spacing(6)
        };
        let provider_hint = match self.settings.provider {
            AiProvider::OpenAi => "使用 OpenAI 兼容接口，可填写兼容服务的地址与模型。",
            AiProvider::Google => {
                "使用 Google 原生 generateContent 接口，默认模型 gemini-flash-latest，可修改模型。"
            }
        };
        let mut settings = column![
            text("AI 设置").size(26),
            column![
                small("模型服务"),
                pick_list(
                    AiProvider::ALL,
                    Some(self.settings.provider),
                    Message::Provider
                )
                .width(Fill),
                small(provider_hint)
            ]
            .spacing(6),
            column![
                small("API Key"),
                text_input("输入 API Key", &self.settings.api_key)
                    .secure(true)
                    .on_input(|v| Message::Setting("api_key", v))
            ]
            .spacing(6),
            field("服务地址", &self.settings.base_url, "base_url"),
            field("模型", &self.settings.model, "model"),
            row![
                field("温度", &self.settings_temperature, "temperature"),
                field(
                    "最大输出 Tokens（留空为 8192，上限 8192）",
                    &self.settings_tokens,
                    "tokens"
                ),
                field("超时（秒）", &self.settings_timeout, "timeout")
            ]
            .spacing(16),
            row![
                button("保存设置").on_press(Message::SaveSettings),
                button("测试连接").on_press(Message::TestAi)
            ]
            .spacing(10),
            text("记忆检索").size(22),
            row![
                button("仅事实检索").on_press(Message::Embedding("mode", "disabled".into())),
                button("本地 BGE-M3").on_press(Message::Embedding("mode", "local".into())),
                button("兼容 Embedding API").on_press(Message::Embedding("mode", "http".into()))
            ]
            .spacing(10)
        ]
        .spacing(22);
        if self.embedding.mode == "local" {
            settings = settings
                .push(small(
                    "选择含 config.json、tokenizer.json、model.safetensors 的模型目录",
                ))
                .push(
                    text_input("模型目录", &self.embedding.model_dir)
                        .on_input(|v| Message::Embedding("dir", v)),
                )
                .push(
                    text_input("最大输入长度", &self.embedding_max)
                        .on_input(|v| Message::Embedding("max", v)),
                );
        }
        if self.embedding.mode == "http" {
            settings = settings
                .push(
                    text_input("Embedding 服务地址", &self.embedding.base_url)
                        .on_input(|v| Message::Embedding("url", v)),
                )
                .push(
                    text_input("Embedding 模型", &self.embedding.model)
                        .on_input(|v| Message::Embedding("model", v)),
                )
                .push(
                    text_input("Embedding API Key", &self.embedding.api_key)
                        .secure(true)
                        .on_input(|v| Message::Embedding("key", v)),
                );
        }
        settings = settings.push(button("保存记忆检索设置").on_press(Message::SaveEmbedding));
        scrollable(
            container(settings)
                .max_width(920)
                .padding(28)
                .style(container::bordered_box),
        )
        .into()
    }
    fn prompts_view(&self) -> Element<'_, Message> {
        let mut list =
            column![text_input("搜索模板", &self.prompt_search).on_input(Message::PromptSearch)]
                .spacing(6);
        for template in self.templates.iter().filter(|t| {
            format!("{} {}", label(t, "key"), label(t, "title"))
                .to_lowercase()
                .contains(&self.prompt_search.to_lowercase())
        }) {
            let key = label(template, "key");
            list = list.push(
                button(text(format!("{}\n{}", label(template, "title"), key)).size(12))
                    .width(Fill)
                    .style(button::text)
                    .on_press(Message::SelectPrompt(key.into())),
            );
        }
        row![
            container(scrollable(list))
                .width(280)
                .height(Fill)
                .padding(12)
                .style(container::bordered_box),
            column![
                text("Prompt 模板").size(26),
                row![
                    text_input("模板 Key", &self.prompt_key)
                        .on_input(|v| Message::PromptField("key", v)),
                    text_input("名称", &self.prompt_title)
                        .on_input(|v| Message::PromptField("title", v))
                ]
                .spacing(10),
                text_input("分类", &self.prompt_category)
                    .on_input(|v| Message::PromptField("category", v)),
                text_editor(&self.editor)
                    .height(Fill)
                    .on_action(Message::Edit),
                row![
                    button("保存模板").on_press(Message::SavePrompt),
                    button("恢复内置").on_press(Message::ResetPrompt),
                    button("删除自定义")
                        .style(button::danger)
                        .on_press(Message::DeletePrompt)
                ]
                .spacing(10)
            ]
            .spacing(14)
        ]
        .spacing(20)
        .into()
    }
    fn context_view(&self) -> Element<'_, Message> {
        let items = self
            .items
            .iter()
            .map(|item| Choice {
                id: id(item),
                name: label(item, "name").into(),
            })
            .collect::<Vec<_>>();
        column![
            row![
                text("项目上下文").size(26),
                space::horizontal(),
                container(self.project_picker()).width(280)
            ]
            .spacing(12),
            row![
                button("Series Bible").on_press(Message::ContextKind("series_bible".into())),
                button("项目大纲").on_press(Message::ContextKind("project_outline".into())),
                pick_list(items, None::<Choice>, Message::ContextVisual)
                    .placeholder("选择资产 Visual DNA"),
                space::horizontal(),
                button("保存 JSON").on_press(Message::SaveContext)
            ]
            .spacing(10),
            row![
                column![
                    small("故事概述"),
                    text_editor(&self.context_summary)
                        .placeholder("输入故事背景与主要情节…")
                        .height(105)
                        .on_action(Message::ContextSummary)
                ]
                .spacing(6),
                column![
                    small("预计集数"),
                    text_input("12", &self.context_episodes)
                        .on_input(|v| Message::ContextField("episodes", v)),
                    button("生成项目大纲")
                        .on_press(Message::Run("project_outline_generate".into()))
                ]
                .spacing(8)
                .width(180)
            ]
            .spacing(14),
            row![
                text_input("优化指令", &self.context_instruction)
                    .on_input(|v| Message::ContextField("instruction", v)),
                button("优化项目大纲").on_press(Message::Run("project_outline_optimize".into()))
            ]
            .spacing(10),
            small(self.context_kind.clone()),
            text_editor(&self.editor)
                .height(Fill)
                .on_action(Message::Edit)
        ]
        .spacing(14)
        .into()
    }
    fn runs_view(&self) -> Element<'_, Message> {
        let mut list = column![
            text("运行记录").size(22),
            self.project_picker(),
            small(self.status.clone())
        ]
        .spacing(12);
        for run in &self.run_list {
            let run_id = label(run, "run_id");
            list = list.push(
                button(text(run_id.to_string()).size(12))
                    .on_press(Message::SelectRun(run_id.into())),
            );
        }
        for stage in &self.stages {
            list = list.push(
                button(text(stage.clone()).size(11))
                    .style(button::text)
                    .on_press(Message::SelectStage(stage.clone())),
            );
        }
        row![
            container(scrollable(list))
                .width(320)
                .height(Fill)
                .padding(14)
                .style(container::bordered_box),
            container(
                text_editor(&self.editor)
                    .size(13)
                    .on_action(Message::Inspect)
                    .height(Fill)
            )
            .width(Fill)
            .height(Fill)
            .padding(20)
            .style(container::bordered_box)
        ]
        .spacing(16)
        .into()
    }
    fn assets_view(&self) -> Element<'_, Message> {
        let mut list = column![
            self.project_picker(),
            pick_list(
                ASSET_CATEGORIES,
                ASSET_CATEGORIES
                    .iter()
                    .find(|c| c.0 == self.category)
                    .copied(),
                |s| { Message::Category(s.0.into()) }
            ),
            text_input("新资产条目", &self.new_name).on_input(Message::NewName),
            button("创建条目").on_press(Message::CreateItem)
        ]
        .spacing(12);
        for item in self
            .items
            .iter()
            .filter(|item| label(item, "category") == self.category)
        {
            list = list.push(
                row![
                    button(text(label(item, "name").to_string()))
                        .width(Fill)
                        .on_press(Message::SelectItem(id(item))),
                    button("×")
                        .style(button::text)
                        .on_press(Message::DeleteItem(id(item)))
                ]
                .spacing(4),
            );
        }
        let mut gallery = column![
            row![
                text("资产库").size(26),
                space::horizontal(),
                button("导入素材").on_press(Message::ImportMedia)
            ]
            .spacing(12)
        ]
        .spacing(20);
        if self.selected_item.is_some() {
            gallery = gallery.push(
                container(
                    column![
                        row![
                            text_input("资产名称", &self.item_name)
                                .on_input(|v| Message::ItemField("name", v)),
                            pick_list(
                                ASSET_CATEGORIES,
                                ASSET_CATEGORIES
                                    .iter()
                                    .find(|c| c.0 == self.item_category)
                                    .copied(),
                                |v| { Message::ItemField("category", v.0.into()) }
                            ),
                            button("保存条目").on_press(Message::SaveItem)
                        ]
                        .spacing(10),
                        small("描述"),
                        text_editor(&self.item_description)
                            .height(100)
                            .on_action(Message::ItemDescription),
                        small("基础 Prompt"),
                        text_editor(&self.item_prompt)
                            .height(100)
                            .on_action(Message::ItemPrompt)
                    ]
                    .spacing(10),
                )
                .padding(16)
                .style(container::bordered_box),
            );
        }
        for asset in &self.media {
            let path = match self.store.asset_path(label(asset, "file_path")) {
                Ok(path) => path,
                Err(_) => {
                    gallery = gallery.push(text("此素材路径不可用，请检查原始工作区"));
                    continue;
                }
            };
            let mut card = column![
                row![
                    text(label(asset, "file_path").to_string()).size(13),
                    space::horizontal(),
                    button("预览 / 打开").on_press(Message::OpenMedia(path.clone())),
                    button("删除")
                        .style(button::danger)
                        .on_press(Message::DeleteAsset(id(asset)))
                ]
                .spacing(8)
            ]
            .spacing(8);
            if label(asset, "file_type") == "image" {
                card = card
                    .push(
                        image(path)
                            .height(240)
                            .content_fit(iced::ContentFit::Contain),
                    )
                    .push(
                        button("生成 Visual DNA（根据文件名与描述推断）")
                            .on_press(Message::VisualAsset(id(asset))),
                    );
            }
            gallery = gallery.push(container(card).padding(16).style(container::bordered_box));
        }
        row![
            container(scrollable(list))
                .width(280)
                .height(Fill)
                .padding(16)
                .style(container::bordered_box),
            scrollable(gallery).width(Fill)
        ]
        .spacing(18)
        .into()
    }
    fn events_view(&self) -> Element<'_, Message> {
        let mut list = column![
            self.project_picker(),
            text("事件").size(24),
            text_input("搜索事件", &self.event_search).on_input(Message::EventFilter),
            pick_list(
                ["名称", "关联数量"],
                Some(self.event_sort.as_str()),
                |sort| Message::EventSort(sort.into())
            ),
            row![
                text_input("新事件", &self.new_name).on_input(Message::NewName),
                button("+").on_press(Message::CreateEvent)
            ]
            .spacing(8)
        ]
        .spacing(14);
        for event in self.events.iter().filter(|e| {
            label(e, "name").contains(&self.event_search)
                || label(e, "description").contains(&self.event_search)
        }) {
            list = list.push(
                row![
                    button(if self.event_focus.contains(&id(event)) {
                        "●"
                    } else {
                        "○"
                    })
                    .style(button::text)
                    .on_press(Message::EventFocus(id(event))),
                    button(text(label(event, "name").to_string()))
                        .width(Fill)
                        .on_press(Message::SelectEvent(id(event))),
                    button("×")
                        .style(button::text)
                        .on_press(Message::DeleteEvent(id(event)))
                ]
                .spacing(4),
            );
        }
        let mut detail = column![
            row![
                text("事件与场景").size(24),
                space::horizontal(),
                button("保存关联").on_press(Message::SaveEvent)
            ]
            .spacing(12),
            small("选择事件后，将它关联到相应场景")
        ]
        .spacing(16);
        if self.selected_event.is_some() {
            detail = detail
                .push(
                    row![
                        text_input("事件名称", &self.event_name)
                            .on_input(|v| Message::EventField("name", v)),
                        text_input("颜色 #RRGGBB", &self.event_color)
                            .on_input(|v| Message::EventField("color", v)),
                        button("保存事件").on_press(Message::SaveEventMetadata)
                    ]
                    .spacing(10),
                )
                .push(
                    text_input("事件描述", &self.event_description)
                        .on_input(|v| Message::EventField("description", v)),
                );
        }
        for scene in &self.scenes {
            let checked = self.linked_scenes.contains(&id(scene));
            detail = detail.push(
                button(text(format!(
                    "{}  {}",
                    if checked { "●" } else { "○" },
                    label(scene, "title")
                )))
                .style(if checked {
                    button::primary
                } else {
                    button::secondary
                })
                .on_press(Message::ToggleScene(id(scene))),
            );
        }
        row![
            container(scrollable(list))
                .width(280)
                .height(Fill)
                .padding(16)
                .style(container::bordered_box),
            column![
                event_matrix::matrix(
                    &self.matrix_events,
                    &self.episodes,
                    &self.scenes,
                    self.selected_event,
                    &self.matrix_view
                )
                .map(Message::Matrix),
                container(scrollable(detail))
                    .height(280)
                    .width(Fill)
                    .padding(16)
                    .style(container::bordered_box)
            ]
            .spacing(14)
        ]
        .spacing(18)
        .into()
    }
    fn memory_view(&self) -> Element<'_, Message> {
        let mut records = column![
            row![
                text("项目记忆").size(26),
                space::horizontal(),
                container(self.project_picker()).width(260),
                button("刷新").on_press(Message::Refresh),
                button("重建语义索引").on_press(Message::RebuildMemory)
            ]
            .spacing(14),
            row![
                text_input("搜索事实原文", &self.memory_search).on_input(Message::MemorySearch),
                button("语义检索").on_press(Message::SemanticSearch)
            ]
            .spacing(10)
        ]
        .spacing(16);
        for changeset in &self.changesets {
            let change_id = label(changeset, "changeset_id");
            records = records.push(
                container(
                    column![
                        text(format!("变更集 · {}", label(changeset, "review_status"))),
                        text(pretty(changeset)).size(12),
                        row![
                            button("批准")
                                .on_press(Message::ReviewChangeset(change_id.into(), true)),
                            button("拒绝")
                                .on_press(Message::ReviewChangeset(change_id.into(), false))
                        ]
                        .spacing(8)
                    ]
                    .spacing(10),
                )
                .padding(16)
                .style(container::bordered_box),
            );
        }
        for conflict in &self.conflicts {
            let conflict_id = label(conflict, "conflict_id");
            records = records.push(
                container(
                    column![
                        text("待裁决冲突"),
                        text(pretty(conflict)).size(12),
                        button("标记已裁决").on_press(Message::ResolveConflict(conflict_id.into()))
                    ]
                    .spacing(10),
                )
                .padding(16)
                .style(container::bordered_box),
            );
        }
        for record in &self.memory_records {
            records = records.push(
                container(text(pretty(record)).size(12))
                    .padding(14)
                    .style(container::bordered_box),
            );
        }
        scrollable(records).into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn switching_ai_provider_clears_credentials_preserves_parameters_and_saves_selection() {
        let root = tempfile::tempdir().unwrap();
        let store = Store::open(root.path()).unwrap();
        let mut app = App::new(
            store,
            Options {
                root: root.path().into(),
                ..Options::default()
            },
        );
        app.settings.api_key = "old-provider-test-key".into();
        app.settings.base_url = "https://old.example/api".into();
        app.settings.model = "custom-old-model".into();
        let _ = app.update(Message::Setting("temperature", "0.7".into()));
        let _ = app.update(Message::Setting("tokens", "4096".into()));
        let _ = app.update(Message::Setting("timeout", "90".into()));

        let _ = app.update(Message::Provider(AiProvider::Google));
        assert_eq!(app.settings.provider, AiProvider::Google);
        assert!(app.settings.api_key.is_empty());
        assert_eq!(app.settings.base_url, AiProvider::Google.default_base_url());
        assert_eq!(app.settings.model, AiProvider::Google.default_model());
        assert!(app.status.contains("重新输入"));
        assert_eq!(app.settings_temperature, "0.7");
        assert_eq!(app.settings_tokens, "4096");
        assert_eq!(app.settings_timeout, "90");

        let _ = app.update(Message::Setting("api_key", "new-provider-test-key".into()));
        let _ = app.update(Message::Setting("model", "gemini-custom".into()));
        let _ = app.update(Message::SaveSettings);
        let saved = AiSettings::load(root.path()).unwrap();
        assert_eq!(saved.provider, AiProvider::Google);
        assert_eq!(saved.api_key, "new-provider-test-key");
        assert_eq!(saved.model, "gemini-custom");
        assert_eq!(saved.temperature, 0.7);
        assert_eq!(saved.max_tokens, Some(4096));
        assert_eq!(saved.timeout_seconds, 90.0);

        let _ = app.update(Message::Provider(AiProvider::OpenAi));
        assert!(app.settings.api_key.is_empty());
        assert_eq!(app.settings.base_url, AiProvider::OpenAi.default_base_url());
        assert_eq!(app.settings.model, AiProvider::OpenAi.default_model());
    }

    #[test]
    fn selecting_the_current_ai_provider_keeps_custom_settings_and_credentials() {
        let root = tempfile::tempdir().unwrap();
        let store = Store::open(root.path()).unwrap();
        let mut app = App::new(
            store,
            Options {
                root: root.path().into(),
                ..Options::default()
            },
        );
        app.settings.api_key = "current-provider-test-key".into();
        app.settings.base_url = "https://custom.example/api".into();
        app.settings.model = "custom-model".into();
        let _ = app.update(Message::Provider(app.settings.provider));
        assert_eq!(app.settings.api_key, "current-provider-test-key");
        assert_eq!(app.settings.base_url, "https://custom.example/api");
        assert_eq!(app.settings.model, "custom-model");
    }

    #[test]
    fn cancelled_or_stale_run_completion_cannot_change_the_editor() {
        let root = tempfile::tempdir().unwrap();
        let store = Store::open(root.path()).unwrap();
        let mut app = App::new(
            store,
            Options {
                root: root.path().into(),
                demo: true,
                ..Options::default()
            },
        );
        let key = app.session.current.unwrap();
        let before = app.editor.text();
        let _ = app.update(Message::RunComplete(
            key,
            "cancelled-run".into(),
            Ok(WorkflowResult {
                run_id: "cancelled-run".into(),
                status: "done".into(),
                artifacts: json!({"outline":"不能覆盖的过期结果"}),
                output_text: String::new(),
                waiting_for: None,
            }),
        ));
        assert!(app.outputs.is_empty(), "cancelled run must not be accepted");
        assert_eq!(app.editor.text(), before);
    }
    #[test]
    fn switching_from_context_back_to_script_never_replaces_script_with_json() {
        let root = tempfile::tempdir().unwrap();
        let store = Store::open(root.path()).unwrap();
        let mut app = App::new(
            store,
            Options {
                root: root.path().into(),
                demo: true,
                ..Options::default()
            },
        );
        let before = app.editor.text();
        let _ = app.update(Message::Page(Page::Context));
        assert_ne!(app.editor.text(), before);
        let _ = app.update(Message::Page(Page::Script));
        assert_eq!(app.session.draft().unwrap().text, before);
    }
    #[test]
    fn context_project_switch_loads_that_projects_context() {
        let root = tempfile::tempdir().unwrap();
        let store = Store::open(root.path()).unwrap();
        let first = store.create_project("A", None).unwrap();
        let second = store.create_project("B", None).unwrap();
        store
            .create_episode(second.id, "B1", Some("{\"wrong\":\"script\"}"))
            .unwrap();
        let ctx = ContextStore::new(root.path().into());
        ctx.put_context(
            &first.id.to_string(),
            "series_bible",
            "v1",
            &json!({"world":"A"}),
        )
        .unwrap();
        ctx.put_context(
            &second.id.to_string(),
            "series_bible",
            "v1",
            &json!({"world":"B"}),
        )
        .unwrap();
        let mut app = App::new(
            store,
            Options {
                root: root.path().into(),
                ..Options::default()
            },
        );
        let _ = app.update(Message::Page(Page::Context));
        let _ = app.update(Message::Project(Choice {
            id: second.id,
            name: second.name,
        }));
        assert_eq!(
            serde_json::from_str::<Value>(&app.editor.text()).unwrap(),
            json!({"world":"B"})
        );
    }
    #[test]
    fn returning_to_prompt_restores_its_body_before_save() {
        let root = tempfile::tempdir().unwrap();
        let store = Store::open(root.path()).unwrap();
        let registry = PromptRegistry::new(root.path().into());
        registry
            .upsert("test_prompt", "测试", "custom", "原提示词")
            .unwrap();
        let mut app = App::new(
            store,
            Options {
                root: root.path().into(),
                demo: true,
                ..Options::default()
            },
        );
        let _ = app.update(Message::Page(Page::Prompts));
        let _ = app.update(Message::SelectPrompt("test_prompt".into()));
        let _ = app.update(Message::Page(Page::Script));
        let _ = app.update(Message::Page(Page::Prompts));
        assert_eq!(app.editor.text(), "原提示词");
    }
    #[test]
    fn unchecking_event_scene_removes_the_old_relation() {
        let root = tempfile::tempdir().unwrap();
        let store = Store::open(root.path()).unwrap();
        let project = store.create_project("A", None).unwrap();
        let ep = store.create_episode(project.id, "第一集", None).unwrap();
        let scene = store.create_scene(ep.id, "场景", 1).unwrap();
        let event = store.create_event(project.id, "事件", "#6366F1").unwrap();
        store
            .upsert_event_node(event.id, "scene", scene.id, "")
            .unwrap();
        let mut app = App::new(
            store.clone(),
            Options {
                root: root.path().into(),
                ..Options::default()
            },
        );
        let _ = app.update(Message::SelectEvent(event.id));
        assert_eq!(app.linked_scenes, vec![scene.id]);
        let _ = app.update(Message::ToggleScene(scene.id));
        let _ = app.update(Message::SaveEvent);
        assert!(store.event_matrix(project.id).unwrap().nodes.is_empty());
    }
    #[test]
    fn empty_project_outline_uses_summary_count_and_instruction_without_an_episode() {
        let root = tempfile::tempdir().unwrap();
        let store = Store::open(root.path()).unwrap();
        store.create_project("独立项目", None).unwrap();
        let mut app = App::new(
            store,
            Options {
                root: root.path().into(),
                ..Options::default()
            },
        );
        let _ = app.update(Message::Page(Page::Context));
        app.context_summary = text_editor::Content::with_text("海岛上的少女收到来自未来的信");
        app.context_episodes = "18".into();
        app.context_instruction = "增加悬念".into();
        let (key, input) = app
            .prepare_input("project_outline_generate".into())
            .expect("项目大纲不应依赖集数已建立");
        assert_eq!(key.episode, 0);
        assert_eq!(input.episode_id, None);
        assert_eq!(input.context["num_episodes"], 18);
        assert_eq!(input.script, "海岛上的少女收到来自未来的信");
        let (_, input) = app
            .prepare_input("project_outline_optimize".into())
            .unwrap();
        assert_eq!(input.script, "增加悬念");
    }
    #[test]
    fn creating_an_item_does_not_reuse_the_previous_items_editor_or_media() {
        let root = tempfile::tempdir().unwrap();
        let store = Store::open(root.path()).unwrap();
        let mut app = App::new(
            store.clone(),
            Options {
                root: root.path().into(),
                demo: true,
                ..Options::default()
            },
        );
        let pid = app.pid().unwrap();
        let item = store
            .create_asset_item(pid, "A", "persona_visual", Some("A描述"))
            .unwrap();
        app.reload();
        let _ = app.update(Message::SelectItem(item.id));
        app.media = vec![json!({"id":999,"file_path":"old.png"})];
        app.new_name = "B".into();
        let _ = app.update(Message::CreateItem);
        assert_eq!(app.item_name, "B");
        assert!(app.media.is_empty());
        let _ = app.update(Message::SaveItem);
        assert_eq!(
            store.asset_item(app.selected_item.unwrap()).unwrap().name,
            "B"
        );
    }
    #[test]
    fn materialization_failure_preserves_review_state_and_retries_the_same_run() {
        let root = tempfile::tempdir().unwrap();
        let store = Store::open(root.path()).unwrap();
        let mut app = App::new(
            store.clone(),
            Options {
                root: root.path().into(),
                demo: true,
                ..Options::default()
            },
        );
        let key = app.session.current.unwrap();
        let run = "retry-original-run";
        let contexts = ContextStore::new(root.path().into());
        contexts
            .write_stage(
                &key.project.to_string(),
                run,
                "chat.status",
                &json!({"status":"paused","waiting_for":"confirm_ingest"}),
            )
            .unwrap();
        contexts
            .snapshot_run(
                &key.project.to_string(),
                run,
                &json!({"project_id":key.project.to_string(),"episode_id":key.episode}),
                &json!({"status":"paused","artifacts":{}}),
                &json!({}),
            )
            .unwrap();
        std::fs::remove_file(root.path().join("memory_store.db")).unwrap();
        std::fs::create_dir(root.path().join("memory_store.db")).unwrap();
        let (_, handle) = Task::<Message>::none().abortable();
        app.busy = Some((key, run.into(), handle));
        let _ = app.update(Message::RunComplete(
            key,
            run.into(),
            Ok(WorkflowResult {
                run_id: run.into(),
                status: "paused".into(),
                artifacts: json!({}),
                output_text: String::new(),
                waiting_for: Some("confirm_ingest".into()),
            }),
        ));
        assert_eq!(
            contexts
                .read_stage(&key.project.to_string(), run, "chat.status")
                .unwrap()
                .unwrap()["status"],
            "paused"
        );
        std::fs::remove_dir(root.path().join("memory_store.db")).unwrap();
        let _ = app.update(Message::ApplyArtifacts);
        assert_eq!(store.episode(key.episode).unwrap().exec_status, "paused");
        assert_eq!(
            store
                .episode(key.episode)
                .unwrap()
                .last_exec_run_id
                .as_deref(),
            Some(run)
        );
    }
    #[test]
    fn live_run_inspection_refreshes_the_selected_stage_without_changing_script() {
        let root = tempfile::tempdir().unwrap();
        let store = Store::open(root.path()).unwrap();
        let mut app = App::new(
            store,
            Options {
                root: root.path().into(),
                demo: true,
                ..Options::default()
            },
        );
        let key = app.session.current.unwrap();
        let draft = app.editor.text();
        let contexts = ContextStore::new(root.path().into());
        contexts
            .snapshot_run(
                &key.project.to_string(),
                "live",
                &json!({}),
                &json!({}),
                &json!({}),
            )
            .unwrap();
        contexts
            .write_stage(
                &key.project.to_string(),
                "live",
                "chat.status",
                &json!({"status":"running"}),
            )
            .unwrap();
        let _ = app.update(Message::SelectRun("live".into()));
        let _ = app.update(Message::SelectStage("chat.status".into()));
        contexts
            .write_stage(
                &key.project.to_string(),
                "live",
                "chat.status",
                &json!({"status":"paused"}),
            )
            .unwrap();
        let (_, handle) = Task::<Message>::none().abortable();
        app.busy = Some((key, "live".into(), handle));
        let _ = app.update(Message::Tick);
        assert!(app.editor.text().contains("paused"));
        let _ = app.update(Message::Cancel);
        assert!(app.editor.text().contains("cancelled"));
        let _ = app.update(Message::Page(Page::Script));
        assert_eq!(app.editor.text(), draft);
    }
    #[test]
    fn asset_deletion_is_blocked_while_project_run_or_materialization_needs_it() {
        let root = tempfile::tempdir().unwrap();
        let store = Store::open(root.path()).unwrap();
        let mut app = App::new(
            store.clone(),
            Options {
                root: root.path().into(),
                demo: true,
                ..Options::default()
            },
        );
        let key = EpisodeKey {
            project: app.pid().unwrap(),
            episode: 0,
        };
        let item = store
            .create_asset_item(key.project, "角色", "persona_visual", None)
            .unwrap();
        let (_, handle) = Task::<Message>::none().abortable();
        app.busy = Some((key, "visual".into(), handle));
        let _ = app.update(Message::DeleteItem(item.id));
        assert!(store.asset_item(item.id).is_ok());
        app.busy = None;
        app.pending_commits.insert(
            key,
            PendingCommit {
                run_id: "visual".into(),
                workflow_status: "done".into(),
                artifacts: json!({}),
            },
        );
        let _ = app.update(Message::DeleteItem(item.id));
        assert!(store.asset_item(item.id).is_ok());
        app.pending_commits.clear();
        let _ = app.update(Message::DeleteItem(item.id));
        assert!(store.asset_item(item.id).is_err());
    }
}
