use crate::Options;
use crate::board::{Board, BoardCard, BoardView};
use crate::context_form::{ContextForm, ContextFormMessage};
use crate::editor::{ReadableEditor, dark_editor};
use crate::event_matrix::{self, MatrixMessage};
use crate::presentation::{field_label, readable, readable_text};
use crate::production::{Gate, ProductionState, StageState};
use crate::session::{EditorSession, EpisodeKey};
use crate::ui;
use aicomic_ai::{
    AiProvider, AiSettings, ContextStore, ExecutionInput, LlmClient, ModelInfo, PromptRegistry,
    ReviewDecision, WorkflowEngine, WorkflowResult,
};
use aicomic_core::{
    AssetItemUpdate, AssetTarget, EpisodeUpdate, EventUpdate, ExportSummary, Store, export_project,
    import_project,
};
use aicomic_memory::{EmbeddingSettings, MemoryQuery, MemoryService, MemoryStore};
use iced::widget::{
    button, column, combo_box, container, grid, image, pick_list, row, scrollable, space, stack,
    text, text_editor, text_input,
};
use iced::{Color, Element, Fill, Subscription, Task, Theme};
use serde::Serialize;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::fmt;
use std::path::PathBuf;
use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Page {
    Workbench,
    Script,
    Storyboard,
    Assets,
    Context,
    Events,
    Memory,
    Runs,
    Ai,
    Prompts,
}
impl Page {
    fn label(self) -> &'static str {
        match self {
            Self::Workbench => "制作台",
            Self::Script => "剧本工坊",
            Self::Storyboard => "分镜",
            Self::Assets => "资产库",
            Self::Context => "设定",
            Self::Events => "故事线",
            Self::Memory => "记忆",
            Self::Runs => "生成记录",
            Self::Ai => "AI 服务",
            Self::Prompts => "提示词",
        }
    }
    fn eyebrow(self) -> &'static str {
        match self {
            Self::Workbench => "PRODUCTION",
            Self::Script => "SCRIPT",
            Self::Storyboard => "STORYBOARD",
            Self::Assets => "ASSETS",
            Self::Context => "BIBLE",
            Self::Events => "STORYLINE",
            Self::Memory => "MEMORY",
            Self::Runs => "RUNS",
            Self::Ai => "SETTINGS",
            Self::Prompts => "PROMPTS",
        }
    }
    fn is_episode_scoped(self) -> bool {
        matches!(self, Self::Workbench | Self::Script | Self::Storyboard)
    }
    fn from_key(key: &str) -> Option<Self> {
        Some(match key {
            "workbench" => Self::Workbench,
            "script" => Self::Script,
            "storyboard" => Self::Storyboard,
            "assets" => Self::Assets,
            "context" => Self::Context,
            "events" => Self::Events,
            "memory" => Self::Memory,
            "runs" => Self::Runs,
            "ai" => Self::Ai,
            "prompts" => Self::Prompts,
            _ => return None,
        })
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
#[derive(Clone)]
struct PendingRun {
    key: EpisodeKey,
    input: ExecutionInput,
    action: String,
    input_chars: usize,
    context_chars: usize,
}
impl PendingRun {
    fn new(key: EpisodeKey, input: ExecutionInput) -> Self {
        let action = action_label(&input.action).to_owned();
        let input_chars = input
            .script
            .chars()
            .filter(|ch| !ch.is_whitespace())
            .count();
        let context_chars = serde_json::to_string(&input.context)
            .map(|value| value.chars().count())
            .unwrap_or(0);
        Self {
            key,
            input,
            action,
            input_chars,
            context_chars,
        }
    }
}
impl fmt::Display for Choice {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.name.fmt(f)
    }
}

#[derive(Clone)]
pub struct SettingValue(String);
impl fmt::Debug for SettingValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("<设置输入已隐藏>")
    }
}
impl From<String> for SettingValue {
    fn from(value: String) -> Self {
        Self(value)
    }
}
impl From<&str> for SettingValue {
    fn from(value: &str) -> Self {
        Self(value.into())
    }
}

#[derive(Debug, Clone)]
struct ModelChoice {
    info: ModelInfo,
    supported: bool,
}
impl fmt::Display for ModelChoice {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.info.fmt(f)?;
        if !self.supported {
            f.write_str(" · 不适用于当前创作")?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub enum Message {
    Page(Page),
    Project(Choice),
    NewName(String),
    ProjectName(String),
    NewProjectDescription(String),
    ToggleNewProject,
    CreateProject,
    ImportProject,
    ProjectImported(Result<Option<i64>, String>),
    ExportProject,
    ProjectExported(Result<Option<(PathBuf, ExportSummary)>, String>),
    DeleteProject,
    CreateEpisode,
    Episode(i64, u8),
    DeleteEpisode(i64),
    Title(String),
    Edit(text_editor::Action),
    Save,
    ScriptStep(u8),
    ToggleGate(Gate),
    ToggleScriptLock,
    StoryboardLimit(String),
    StoryboardAspect(String),
    StoryboardPromptStyle(String),
    CopyPrompt(String),
    Run(String),
    ConfirmRun,
    CancelRun,
    RunComplete(EpisodeKey, String, Result<WorkflowResult, String>),
    Cancel,
    Review(bool),
    Board(BoardView),
    Tick,
    Snapshot(iced::window::Screenshot),
    Close,
    Provider(AiProvider),
    Setting(&'static str, SettingValue),
    FetchModels,
    ModelsFetched(u64, Result<Vec<ModelInfo>, String>),
    SelectModel(String),
    SaveSettings,
    TestAi,
    AiTest(Result<String, String>),
    SelectPrompt(String),
    PromptField(&'static str, String),
    SavePrompt,
    ResetPrompt,
    DeletePrompt,
    ContextKind(String),
    ContextDocument(ContextFormMessage),
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
}

pub struct App {
    pending_commits: HashMap<EpisodeKey, PendingCommit>,
    pending_run: Option<PendingRun>,
    store: Store,
    options: Options,
    page: Page,
    status: String,
    projects: Vec<Choice>,
    project: Option<Choice>,
    episodes: Vec<Value>,
    title: String,
    new_name: String,
    project_name: String,
    project_description: String,
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
    model_catalog: combo_box::State<ModelChoice>,
    model_catalog_request: u64,
    model_catalog_loading: bool,
    model_catalog_loaded: bool,
    model_catalog_status: String,
    templates: Vec<Value>,
    prompt_key: String,
    prompt_title: String,
    prompt_category: String,
    context_kind: String,
    context_form: ContextForm,
    context_document_ready: bool,
    production: ProductionState,
    series_bible: Value,
    project_outline: Value,
    visual_dna_ids: Vec<i64>,
    new_project_open: bool,
    max_shots: String,
    aspect_ratio: String,
    prompt_style: String,
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
const STAGE_TITLES: [&str; 5] = ["故事基线", "本集剧本", "分集结构", "资产定型", "分镜"];

fn stage_state(done: bool, review: bool, active: bool) -> StageState {
    if done {
        StageState::Done
    } else if review {
        StageState::Review
    } else if active {
        StageState::Active
    } else {
        StageState::Empty
    }
}

fn short(text: &str, limit: usize) -> String {
    let mut value: String = text.chars().take(limit).collect();
    if text.chars().count() > limit {
        value.push('…');
    }
    value
}

fn export_file_stem(name: &str) -> String {
    let value = name
        .chars()
        .map(|ch| {
            if ch.is_control() || "/\\:*?\"<>|".contains(ch) {
                '_'
            } else {
                ch
            }
        })
        .collect::<String>();
    let value = value.trim().trim_end_matches('.').trim();
    let value = value.chars().take(80).collect::<String>();
    if value.is_empty() {
        "星晴项目".into()
    } else {
        value
    }
}

fn episode_number(episode: &Value) -> i64 {
    episode["order"].as_i64().unwrap_or(0) + 1
}

fn category_label(code: &str) -> &str {
    ASSET_CATEGORIES
        .iter()
        .find(|category| category.0 == code)
        .map(|category| category.1)
        .unwrap_or(code)
}

fn relative_time(milliseconds: u64) -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0);
    let seconds = now.saturating_sub(milliseconds) / 1000;
    match seconds {
        0..=59 => "刚刚".into(),
        60..=3599 => format!("{} 分钟前", seconds / 60),
        3600..=86399 => format!("{} 小时前", seconds / 3600),
        _ => format!("{} 天前", seconds / 86400),
    }
}

fn provider_label(provider: AiProvider) -> &'static str {
    match provider {
        AiProvider::OpenAi => "OpenAI 兼容",
        AiProvider::Google => "Google AI",
    }
}

fn stage_label(stage: &str) -> String {
    if stage.starts_with("log.") {
        return "日志".into();
    }
    match stage {
        "chat.status" => "运行状态".into(),
        "chat.plan" => "执行计划".into(),
        "storyboard.parsed" => "分镜解析".into(),
        "desktop.materialization" => "本地写入".into(),
        "architect.parsed" => "结构解析".into(),
        "writer.parsed" => "剧本写作".into(),
        "split_scenes.parsed" => "场次拆分".into(),
        "prompt_translate" => "提示词编译".into(),
        other if other.starts_with("qc.parsed.") => {
            format!("质检 {}", &other["qc.parsed.".len()..])
        }
        other => field_label(other),
    }
}

fn action_label(action: &str) -> &str {
    match action {
        "script_optimize" => "优化剧本",
        "workflow_script" => "完整改稿",
        "workflow_storyboard" => "生成分镜",
        "episode_outline_generate" => "结构拆解",
        "episode_assets_visual_dna" => "提取资产",
        "project_outline_generate" => "生成项目大纲",
        "project_outline_optimize" => "优化项目大纲",
        "visual_dna_ingest" => "生成视觉设定",
        "memory_extract_changeset" => "记忆抽取",
        "episode_execute" => "完整执行",
        _ => action,
    }
}

fn shot_size_label(code: &str) -> &str {
    match code {
        "ELS" => "大远景",
        "LS" => "远景",
        "MS" => "中景",
        "CU" => "特写",
        "ECU" => "大特写",
        "INSERT" => "插入",
        _ => code,
    }
}

fn camera_angle_label(code: &str) -> &str {
    match code {
        "EYE" => "平视",
        "LOW" => "仰视",
        "HIGH" => "俯视",
        "DUTCH" => "斜角",
        _ => code,
    }
}

fn lighting_label(code: &str) -> &str {
    match code {
        "SOFT" => "柔光",
        "HARD" => "硬光",
        "CHIAROSCURO" => "明暗对比",
        "RIM" => "轮廓光",
        "VOLUMETRIC" => "体积光",
        _ => code,
    }
}

fn run_status_tone(status: &str) -> ui::Tone {
    match status {
        "succeeded" | "done" | "completed" => ui::Tone::Mint,
        "running" | "paused" | "needs_clarification" => ui::Tone::Gold,
        "error" | "failed" | "apply_failed" => ui::Tone::Coral,
        "cancelled" | "rejected" => ui::Tone::Muted,
        _ => ui::Tone::Indigo,
    }
}

fn run_status_label(status: &str) -> &str {
    match status {
        "succeeded" | "done" | "completed" => "完成",
        "running" => "执行中",
        "paused" => "待审核",
        "needs_clarification" => "需补充",
        "error" | "failed" => "失败",
        "apply_failed" => "写入失败",
        "cancelled" => "已取消",
        "rejected" => "已拒绝",
        _ => status,
    }
}

impl App {
    pub fn new(store: Store, options: Options) -> Self {
        let settings = AiSettings::load(&options.root).unwrap_or_default();
        let embedding = EmbeddingSettings::load(&options.root).unwrap_or_default();
        let memory_service = MemoryService::new(&options.root, embedding.clone());
        let mut app = Self {
            pending_commits: HashMap::new(),
            pending_run: None,
            store,
            options,
            page: Page::Workbench,
            status: "就绪 · 本地保存".into(),
            projects: vec![],
            project: None,
            episodes: vec![],
            title: String::new(),
            new_name: String::new(),
            project_name: String::new(),
            project_description: String::new(),
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
            model_catalog: combo_box::State::new(vec![]),
            model_catalog_request: 0,
            model_catalog_loading: false,
            model_catalog_loaded: false,
            model_catalog_status: "填写服务地址与 API Key 后，获取模型列表并选择。".into(),
            templates: vec![],
            prompt_key: String::new(),
            prompt_title: String::new(),
            prompt_category: "custom".into(),
            context_kind: "series_bible".into(),
            context_form: ContextForm::new(json!({})),
            context_document_ready: false,
            production: ProductionState::default(),
            series_bible: json!({}),
            project_outline: json!({}),
            visual_dna_ids: vec![],
            new_project_open: false,
            max_shots: "24".into(),
            aspect_ratio: "9:16".into(),
            prompt_style: "通用提示词".into(),
            run_list: vec![],
            selected_run: String::new(),
            selected_stage: String::new(),
            stages: vec![],
            items: vec![],
            selected_item: None,
            category: "all".into(),
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
        if let Some(page) = app.options.page.as_deref().and_then(Page::from_key) {
            app.page = page;
            app.load_document();
            if page == Page::Script {
                let step = app.options.step.unwrap_or(0).min(2);
                if let Some(key) = app.session.current {
                    app.select_episode(key.episode, step);
                }
            }
        }
        if let Some(action) = app.options.preview.clone() {
            let _ = app.update(Message::Run(action));
        }
        app
    }

    pub fn theme(&self) -> Theme {
        ui::theme()
    }
    pub fn subscription(&self) -> Subscription<Message> {
        let mut subscriptions = vec![
            if self.options.smoke || self.busy.is_some() || self.options.scroll.is_some() {
                iced::time::every(Duration::from_millis(700)).map(|_| Message::Tick)
            } else {
                Subscription::none()
            },
            iced::window::close_requests().map(|_| Message::Close),
        ];
        if self.options.ime_log {
            subscriptions.push(iced::event::listen_with(|event, _, _| {
                if let iced::event::Event::InputMethod(event) = event {
                    eprintln!("ime: {event:?}");
                }
                None::<Message>
            }));
        }
        Subscription::batch(subscriptions)
    }

    fn root(&self) -> PathBuf {
        self.options.root.clone()
    }
    fn pid(&self) -> Option<i64> {
        self.project.as_ref().map(|p| p.id)
    }
    fn fail(&mut self, error: impl fmt::Display) {
        self.status = format!("操作失败：{}", readable_text(&error.to_string()));
    }
    fn current_episode(&self) -> Option<&Value> {
        self.session.current.and_then(|key| {
            self.episodes
                .iter()
                .find(|episode| id(episode) == key.episode)
        })
    }
    fn artifacts(&self) -> Option<&Value> {
        self.session.current.and_then(|key| self.outputs.get(&key))
    }
    fn current_script(&self) -> String {
        if self.page == Page::Script && self.step == 0 {
            self.editor.text()
        } else {
            self.session
                .draft()
                .map(|draft| draft.text.clone())
                .unwrap_or_default()
        }
    }
    fn script_chars(&self) -> usize {
        self.current_script()
            .chars()
            .filter(|ch| !ch.is_whitespace())
            .count()
    }
    fn estimated_seconds(&self) -> u64 {
        // Research baseline: Chinese speech is roughly 4.1 speakable characters
        // per second. Use it as a budget hint only, never as a hard cut.
        (self.script_chars() as f64 / 4.1).ceil() as u64
    }
    fn scene_count(&self) -> usize {
        self.current_script()
            .lines()
            .filter(|line| {
                let line = line.trim();
                (line.starts_with('第') && line.contains('场'))
                    || line.starts_with("INT.")
                    || line.starts_with("EXT.")
                    || line.starts_with("内景")
                    || line.starts_with("外景")
            })
            .count()
    }
    fn has_series_bible(&self) -> bool {
        self.series_bible
            .as_object()
            .is_some_and(|value| !value.is_empty())
    }
    fn has_outline(&self) -> bool {
        self.project_outline
            .as_object()
            .is_some_and(|value| !value.is_empty())
    }
    fn has_structure(&self) -> bool {
        self.artifacts().is_some_and(|artifacts| {
            artifacts["outline"].is_object()
                || artifacts["outline"].is_string()
                || artifacts["beat_sheet"]
                    .as_array()
                    .is_some_and(|beats| !beats.is_empty())
        })
    }
    fn has_assets(&self) -> bool {
        !self.items.is_empty()
    }
    fn shots(&self) -> Option<&Vec<Value>> {
        self.artifacts()
            .and_then(|artifacts| artifacts["shots"].as_array())
            .filter(|shots| !shots.is_empty())
    }
    fn has_shots(&self) -> bool {
        self.shots().is_some()
    }
    fn episode_locked(&self) -> bool {
        self.current_episode()
            .and_then(|episode| episode["script_locked"].as_bool())
            .unwrap_or(false)
    }
    fn stage_states(&self) -> [StageState; 5] {
        let episode = self.session.current.map(|key| key.episode).unwrap_or(0);
        let gates = self.production.episode(episode);
        [
            stage_state(
                self.production.outline_approved && self.has_outline(),
                self.has_outline() && !self.production.outline_approved,
                self.has_series_bible() || !self.has_outline(),
            ),
            stage_state(
                self.episode_locked() && !self.current_script().trim().is_empty(),
                false,
                !self.current_script().trim().is_empty(),
            ),
            stage_state(
                gates.structure_approved && self.has_structure(),
                self.has_structure() && !gates.structure_approved,
                self.episode_locked(),
            ),
            stage_state(
                self.production.assets_frozen && self.has_assets(),
                self.has_assets() && !self.production.assets_frozen,
                self.episode_locked(),
            ),
            stage_state(
                gates.storyboard_approved && self.has_shots(),
                self.has_shots() && !gates.storyboard_approved,
                self.production.assets_frozen,
            ),
        ]
    }
    fn invalidate_after_run(&mut self, key: EpisodeKey, artifacts: &Value) {
        let mut changed = false;
        if artifacts
            .get("project_outline")
            .is_some_and(Value::is_object)
        {
            self.production.invalidate(0, Gate::Outline);
            changed = true;
        }
        if artifacts.get("assets_visual_dna").is_some()
            || artifacts.get("asset_ingest_plan").is_some()
        {
            self.production.invalidate(0, Gate::Assets);
            changed = true;
        }
        if artifacts.get("outline").is_some() || artifacts.get("beat_sheet").is_some() {
            self.production.invalidate(key.episode, Gate::Structure);
            changed = true;
        }
        if artifacts.get("shots").is_some() {
            self.production.invalidate(key.episode, Gate::Storyboard);
            changed = true;
        }
        if changed {
            self.persist_production();
        }
    }
    fn persist_production(&mut self) {
        if let Some(pid) = self.pid()
            && let Err(error) = self.production.save(&ContextStore::new(self.root()), pid)
        {
            self.fail(error);
        }
    }
    fn toggle_gate(&mut self, gate: Gate) {
        let episode = self.session.current.map(|key| key.episode).unwrap_or(0);
        let confirmed = self.production.confirmed(episode, gate);
        if !confirmed {
            let ready = match gate {
                Gate::Outline => self.has_outline(),
                Gate::Assets => self.has_assets(),
                Gate::Structure => self.has_structure(),
                Gate::Storyboard => self.has_shots(),
            };
            if !ready {
                self.fail(format!("{} 还不能确认：缺少对应产物", gate.label()));
                return;
            }
        }
        self.production.set(episode, gate, !confirmed);
        self.persist_production();
        self.status = if confirmed {
            format!("{} 已撤销", gate.label())
        } else {
            format!("{} 已确认", gate.label())
        };
    }
    fn toggle_script_lock(&mut self) {
        let Some(key) = self.session.current else {
            self.fail("请先选择一集剧本");
            return;
        };
        let locked = self.episode_locked();
        if !locked {
            if !self.save_script() {
                return;
            }
            if self.current_script().trim().is_empty() {
                self.fail("剧本为空，不能锁定");
                return;
            }
        }
        match self.store.patch_episode(
            key.episode,
            EpisodeUpdate {
                script_locked: Some(!locked),
                ..EpisodeUpdate::default()
            },
        ) {
            Ok(_) => {
                self.status = if locked {
                    "剧本已解锁，可以继续修改".into()
                } else {
                    "剧本已锁定，可以进入结构拆解与分镜".into()
                };
                self.reload();
            }
            Err(error) => self.fail(error),
        }
    }
    fn episode_choices(&self) -> Vec<Choice> {
        self.episodes
            .iter()
            .map(|episode| Choice {
                id: id(episode),
                name: format!(
                    "EP{:02} · {}",
                    episode_number(episode),
                    label(episode, "title")
                ),
            })
            .collect()
    }
    fn selected_episode_choice(&self) -> Option<Choice> {
        self.session.current.and_then(|key| {
            self.episodes
                .iter()
                .find(|episode| id(episode) == key.episode)
                .map(|episode| Choice {
                    id: id(episode),
                    name: format!(
                        "EP{:02} · {}",
                        episode_number(episode),
                        label(episode, "title")
                    ),
                })
        })
    }
    fn invalidate_model_catalog(&mut self) {
        self.model_catalog_request = self.model_catalog_request.wrapping_add(1);
        self.model_catalog_loading = false;
        self.model_catalog_loaded = false;
        self.model_catalog = combo_box::State::new(vec![]);
        self.model_catalog_status = "服务配置已变更，请重新获取模型列表并选择。".into();
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
        self.pending_run = None;
        self.project = Some(choice);
        self.context_form.load(json!({}));
        self.context_document_ready = false;
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
        self.production = ProductionState::load(&contexts, pid);
        self.series_bible = contexts
            .get_context(&pid.to_string(), "series_bible", "v1")
            .ok()
            .flatten()
            .unwrap_or_else(|| json!({}));
        self.project_outline = contexts
            .get_context(&pid.to_string(), "project_outline", "v1")
            .ok()
            .flatten()
            .unwrap_or_else(|| json!({}));
        self.visual_dna_ids = self
            .items
            .iter()
            .map(id)
            .filter(|item| {
                contexts
                    .get_context(
                        &pid.to_string(),
                        &format!("visual_dna.asset_item_{item}"),
                        "v1",
                    )
                    .ok()
                    .flatten()
                    .is_some_and(|value| value.is_object())
            })
            .collect();
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
        if self
            .session
            .current
            .is_some_and(|key| key.episode != episode_id)
        {
            self.pending_run = None;
        }
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
            .map(readable)
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
        let previous = self.store.episode(key.episode).ok();
        let was_locked = previous
            .as_ref()
            .is_some_and(|episode| episode.script_locked);
        let previous_script = previous
            .as_ref()
            .and_then(|episode| episode.description.clone())
            .unwrap_or_default();
        match self
            .store
            .update_episode(key.episode, &self.title, Some(&script))
        {
            Ok(_) => {
                self.session.saved(key, revision);
                if was_locked && previous_script != script {
                    let _ = self.store.patch_episode(
                        key.episode,
                        EpisodeUpdate {
                            script_locked: Some(false),
                            ..EpisodeUpdate::default()
                        },
                    );
                    self.status = "已保存 · 剧本已修改，锁定解除".into();
                } else {
                    self.status = "已保存到本地".into();
                }
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
            self.context_document_ready = false;
            if let Some(pid) = self.pid() {
                let result = ContextStore::new(self.root()).get_context(
                    &pid.to_string(),
                    &self.context_kind,
                    "v1",
                );
                match result {
                    Ok(value) => {
                        self.context_form.load(value.unwrap_or_else(|| json!({})));
                        self.context_document_ready = true;
                    }
                    Err(e) => self.fail(e),
                }
            } else {
                self.context_form.load(json!({}));
            }
        } else if self.page == Page::Assets {
            if self.selected_item.is_none()
                && let Some(item) = self.items.first()
            {
                let _ = self.update(Message::SelectItem(id(item)));
            }
        } else if self.page == Page::Prompts {
            match PromptRegistry::new(self.root()).list() {
                Ok(rows) => self.templates = stringify(rows),
                Err(e) => self.fail(e),
            }
            if self.prompt_key.is_empty()
                && let Some(template) = self.templates.first()
            {
                self.prompt_key = label(template, "key").into();
                self.prompt_title = label(template, "title").into();
                self.prompt_category = label(template, "category").into();
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
            self.editor = text_editor::Content::with_text("选择运行记录或执行阶段，查看详细内容。");
            self.refresh_runs();
            if self.selected_run.is_empty()
                && let Some(run) = self.run_list.first()
            {
                self.selected_run = label(run, "run_id").into();
                self.selected_stage.clear();
                self.refresh_runs();
            }
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
                context["current_outline"] = if self.context_kind == "project_outline" {
                    if !self.context_document_ready {
                        return Err("请先载入需要优化的项目大纲".into());
                    }
                    self.context_form.value()?
                } else {
                    contexts
                        .get_context(&project.to_string(), "project_outline", "v1")
                        .map_err(|e| e.to_string())?
                        .unwrap_or_else(|| json!({}))
                };
                self.context_instruction.clone()
            }
            "workflow_storyboard" => {
                let max: u64 = self
                    .max_shots
                    .trim()
                    .parse()
                    .map_err(|_| "分镜上限应为 1–200 的整数")?;
                if !(1..=200).contains(&max) {
                    return Err("分镜上限应为 1–200 的整数".into());
                }
                let locked_visual_dna = self
                    .items
                    .iter()
                    .filter_map(|item| {
                        let item_id = id(item);
                        let visual_dna = contexts
                            .get_context(
                                &project.to_string(),
                                &format!("visual_dna.asset_item_{item_id}"),
                                "v1",
                            )
                            .ok()
                            .flatten()?;
                        Some(json!({
                            "name": label(item, "name"),
                            "category": label(item, "category"),
                            "visual_dna": visual_dna,
                        }))
                    })
                    .collect::<Vec<_>>();
                context["locked_visual_dna"] = json!(locked_visual_dna);
                context["max_shots"] = json!(max);
                context["aspect_ratio"] = json!(self.aspect_ratio);
                context["prompt_style"] = json!(if self.prompt_style.starts_with("Midjourney") {
                    "mj_v6"
                } else {
                    "generic"
                });
                self.session
                    .draft()
                    .map(|draft| draft.text.clone())
                    .unwrap_or_default()
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
    fn start_run(&mut self, pending: PendingRun) -> Task<Message> {
        let PendingRun {
            key,
            mut input,
            action,
            input_chars,
            context_chars,
        } = pending;
        if self.pending_commits.contains_key(&key) {
            self.fail("请先重试写入当前运行结果");
            return Task::none();
        }
        let confirmed_at_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|duration| duration.as_millis() as u64)
            .unwrap_or(0);
        input.context["preflight"] = json!({
            "action": input.action,
            "action_label": action,
            "target_episode": key.episode,
            "provider": self.settings.provider,
            "model": self.settings.model,
            "input_chars": input_chars,
            "context_chars": context_chars,
            "max_output_tokens": self.settings.max_tokens,
            "confirmed_at_ms": confirmed_at_ms
        });
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
                    let ids =
                        crate::integration::prepare_memory(&evidence_store, key, &evidence_text)?;
                    let quotes = crate::integration::source_evidences(&evidence_store, key, &ids)?;
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
        task
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
        self.production = ProductionState::load(&contexts, pid);
        self.series_bible = contexts
            .get_context(&pid.to_string(), "series_bible", "v1")
            .ok()
            .flatten()
            .unwrap_or_else(|| json!({}));
        self.project_outline = contexts
            .get_context(&pid.to_string(), "project_outline", "v1")
            .ok()
            .flatten()
            .unwrap_or_else(|| json!({}));
        self.visual_dna_ids = self
            .items
            .iter()
            .map(id)
            .filter(|item| {
                contexts
                    .get_context(
                        &pid.to_string(),
                        &format!("visual_dna.asset_item_{item}"),
                        "v1",
                    )
                    .ok()
                    .flatten()
                    .is_some_and(|value| value.is_object())
            })
            .collect();
        if let Ok(runs) = contexts.list_runs(&pid.to_string()) {
            self.run_list = stringify(runs);
        }
        if !self.selected_run.is_empty() {
            if let Ok(stages) = contexts.list_stages(&pid.to_string(), &self.selected_run) {
                self.stages = stages.into_iter().map(|stage| stage.name).collect();
            }
            if self.selected_stage.is_empty()
                && let Some(stage) = self.stages.last().cloned()
            {
                self.selected_stage = stage;
            }
            if self.page == Page::Runs {
                let value = if self.selected_stage.is_empty() {
                    contexts.read_run(&pid.to_string(), &self.selected_run)
                } else {
                    contexts.read_stage(&pid.to_string(), &self.selected_run, &self.selected_stage)
                };
                if let Ok(Some(value)) = value {
                    let body = readable(&value);
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
        if self.page.is_episode_scoped() {
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
            Message::ProjectName(name) => self.project_name = name,
            Message::NewProjectDescription(description) => self.project_description = description,
            Message::ToggleNewProject => {
                self.new_project_open = !self.new_project_open;
            }
            Message::CreateProject => {
                if !self.project_name.trim().is_empty() {
                    match self.store.create_project(
                        self.project_name.trim(),
                        Some(self.project_description.trim()).filter(|s| !s.is_empty()),
                    ) {
                        Ok(project) => {
                            self.project_name.clear();
                            self.project_description.clear();
                            self.new_project_open = false;
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
            Message::ImportProject => {
                let store = self.store.clone();
                return Task::perform(
                    async move {
                        let Some(file) = rfd::AsyncFileDialog::new()
                            .add_filter("星晴项目", &["xqproj"])
                            .pick_file()
                            .await
                        else {
                            return Ok(None);
                        };
                        let path = file.path().to_path_buf();
                        let summary = tokio::task::spawn_blocking(move || {
                            import_project(&store, &path).map_err(|error| error.to_string())
                        })
                        .await
                        .map_err(|error| error.to_string())??;
                        Ok(Some(summary.project_id))
                    },
                    Message::ProjectImported,
                );
            }
            Message::ProjectImported(result) => match result {
                Ok(Some(id)) => {
                    self.reload_projects();
                    if let Some(choice) =
                        self.projects.iter().find(|choice| choice.id == id).cloned()
                    {
                        self.select_project(choice);
                        self.status = "项目已导入，分集、资产、设定与事件已加入。".into();
                    } else {
                        self.status = "项目已导入，但刷新列表失败，请切换项目后重试。".into();
                    }
                }
                Ok(None) => self.status = "已取消导入。".into(),
                Err(error) => self.fail(error),
            },
            Message::ExportProject => {
                let Some(project_id) = self.pid() else {
                    self.status = "请先选择项目。".into();
                    return Task::none();
                };
                let Some(project) = self.project.clone() else {
                    return Task::none();
                };
                let store = self.store.clone();
                let file_name = format!("{}.xqproj", export_file_stem(&project.name));
                return Task::perform(
                    async move {
                        let Some(file) = rfd::AsyncFileDialog::new()
                            .set_file_name(&file_name)
                            .add_filter("星晴项目", &["xqproj"])
                            .save_file()
                            .await
                        else {
                            return Ok(None);
                        };
                        let path = file.path().to_path_buf();
                        let target = path.clone();
                        let summary = tokio::task::spawn_blocking(move || {
                            export_project(&store, project_id, &target)
                                .map_err(|error| format!("导出失败：{error}"))
                        })
                        .await
                        .map_err(|error| error.to_string())??;
                        Ok(Some((path, summary)))
                    },
                    Message::ProjectExported,
                );
            }
            Message::ProjectExported(result) => match result {
                Ok(Some((path, summary))) => {
                    let warnings = if summary.warnings.is_empty() {
                        String::new()
                    } else {
                        format!("，{} 条警告", summary.warnings.len())
                    };
                    self.status = format!(
                        "已导出「{}」：{} 集 / {} 资产条目 / {} 个媒体{} → {}",
                        summary.project_name,
                        summary.episodes,
                        summary.asset_items,
                        summary.media,
                        warnings,
                        path.display()
                    );
                }
                Ok(None) => self.status = "已取消导出。".into(),
                Err(error) => self.fail(error),
            },
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
                                self.context_form.load(json!({}));
                                self.context_document_ready = false;
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
                Page::Workbench | Page::Script | Page::Storyboard => {
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
                let (key, input) = match self.prepare_input(action) {
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
                self.status = format!("等待确认：{}", action_label(&input.action));
                self.pending_run = Some(PendingRun::new(key, input));
            }
            Message::ConfirmRun => {
                if let Some(pending) = self.pending_run.take() {
                    if self.settings.api_key.is_empty() {
                        self.pending_run = Some(pending);
                        self.fail("请先在 AI 服务中配置 API Key");
                        return Task::none();
                    }
                    // Rebuild the input and refuse to send something the user did
                    // not see. Editing the script or selecting another asset while
                    // the panel is open must force a fresh confirmation.
                    match self.prepare_input(pending.input.action.clone()) {
                        Ok((key, current))
                            if key == pending.key
                                && current.script == pending.input.script
                                && current.context == pending.input.context =>
                        {
                            return self.start_run(pending);
                        }
                        Ok(_) => {
                            self.fail("输入已发生变化，请重新点击生成并确认");
                        }
                        Err(error) => self.fail(error),
                    }
                }
            }
            Message::CancelRun => {
                if let Some(pending) = self.pending_run.take() {
                    self.status = format!("已取消：{}", pending.action);
                }
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
                        self.invalidate_after_run(key, &result.artifacts);
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
                                text_editor::Content::with_text(&readable(&result.artifacts));
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
                if let Some(offset) = self.options.scroll
                    && (2..=4).contains(&self.ticks)
                {
                    let offset = offset + (self.ticks - 2) as f32 * 220.0;
                    let target = match self.page {
                        Page::Prompts => Some("prompt-editor"),
                        Page::Script => Some("script-editor"),
                        _ => None,
                    };
                    if let Some(target) = target {
                        return iced::advanced::widget::operate(
                            iced::advanced::widget::operation::scrollable::scroll_to(
                                iced::advanced::widget::Id::new(target),
                                iced::advanced::widget::operation::scrollable::AbsoluteOffset {
                                    x: None,
                                    y: Some(offset),
                                },
                            ),
                        );
                    }
                }
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
                    self.invalidate_model_catalog();
                    self.settings.provider = provider;
                    self.settings.base_url = provider.default_base_url().into();
                    self.settings.model = provider.default_model().into();
                    self.settings.api_key.clear();
                    self.status = format!("已切换至 {provider}，请重新输入 API Key 并保存设置");
                }
            }
            Message::Setting(field, value) => {
                let value = value.0;
                match field {
                    "api_key" => {
                        if self.settings.api_key != value {
                            self.invalidate_model_catalog();
                            self.settings.api_key = value;
                        }
                    }
                    "base_url" => {
                        if self.settings.base_url != value {
                            self.invalidate_model_catalog();
                            self.settings.base_url = value;
                        }
                    }
                    "tokens" => self.settings_tokens = value,
                    "timeout" => self.settings_timeout = value,
                    "temperature" => self.settings_temperature = value,
                    _ => {}
                }
            }
            Message::FetchModels => {
                if self.model_catalog_loading {
                    return Task::none();
                }
                self.model_catalog_request = self.model_catalog_request.wrapping_add(1);
                let request = self.model_catalog_request;
                let settings = self.settings.clone();
                self.model_catalog_loading = true;
                self.model_catalog_status = "正在获取服务提供的全部模型…".into();
                return Task::perform(
                    async move {
                        LlmClient
                            .list_models(&settings)
                            .await
                            .map_err(|e| e.to_string())
                    },
                    move |result| Message::ModelsFetched(request, result),
                );
            }
            Message::ModelsFetched(request, result) => {
                if request != self.model_catalog_request || !self.model_catalog_loading {
                    return Task::none();
                }
                self.model_catalog_loading = false;
                match result {
                    Ok(models) => {
                        let choices: Vec<_> = models
                            .into_iter()
                            .map(|info| ModelChoice {
                                supported: info.supports_chat(self.settings.provider),
                                info,
                            })
                            .collect();
                        let supported = choices.iter().filter(|model| model.supported).count();
                        self.model_catalog_status = if choices.is_empty() {
                            "服务未返回模型，请检查服务配置后重新获取。".into()
                        } else if self.settings.provider == AiProvider::OpenAi {
                            format!(
                                "共 {} 个模型，均可选择；服务未提供统一能力信息，请选择聊天模型并保存设置。",
                                choices.len()
                            )
                        } else {
                            format!(
                                "共 {} 个模型，{} 个支持当前生成接口；选择后请保存设置。",
                                choices.len(),
                                supported
                            )
                        };
                        self.model_catalog = combo_box::State::new(choices);
                        self.model_catalog_loaded = true;
                    }
                    Err(error) => {
                        self.model_catalog_status =
                            format!("获取模型列表失败：{error}。已保留当前选择，请重试。");
                    }
                }
                self.status = self.model_catalog_status.clone();
            }
            Message::SelectModel(model_id) => {
                match self
                    .model_catalog
                    .options()
                    .iter()
                    .find(|model| model.info.id == model_id)
                {
                    Some(model) if model.supported => {
                        self.settings.model = model.info.id.clone();
                        self.status = format!("已选择 {}，请保存设置", self.settings.model);
                    }
                    Some(_) => self.status = "该模型不支持当前生成接口，请选择其他模型。".into(),
                    None => self.status = "模型列表已更新，请从当前列表重新选择。".into(),
                }
            }
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
            Message::ContextDocument(message) => {
                if self.context_document_ready {
                    self.context_form.update(message);
                }
            }
            Message::SaveContext => {
                if !self.context_document_ready {
                    self.fail("当前上下文尚未载入，请重新选择后再保存");
                    return Task::none();
                }
                if let Some(pid) = self.pid() {
                    match self.context_form.value() {
                        Ok(value) => match ContextStore::new(self.root()).put_context(
                            &pid.to_string(),
                            &self.context_kind,
                            "v1",
                            &value,
                        ) {
                            Ok(_) => {
                                if self.context_kind == "project_outline" {
                                    self.production.invalidate(0, Gate::Outline);
                                    self.persist_production();
                                }
                                self.reload();
                                self.status = "项目上下文已保存".into();
                            }
                            Err(e) => self.fail(e),
                        },
                        Err(e) => self.fail(e),
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
                    self.editor = text_editor::Content::with_text(&readable(&value));
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
                    match self.store.create_asset_item(
                        pid,
                        &self.new_name,
                        &self.item_category,
                        None,
                    ) {
                        Ok(item) => {
                            self.selected_item = Some(item.id);
                            self.new_name.clear();
                            self.production.invalidate(0, Gate::Assets);
                            self.persist_production();
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
                        self.production.invalidate(0, Gate::Assets);
                        self.persist_production();
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
                    self.production.invalidate(0, Gate::Assets);
                    self.persist_production();
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
                    self.production.invalidate(0, Gate::Assets);
                    self.persist_production();
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
            Message::ScriptStep(step) => {
                if let Some(key) = self.session.current {
                    self.select_episode(key.episode, step);
                    self.page = Page::Script;
                }
            }
            Message::ToggleGate(gate) => {
                self.toggle_gate(gate);
            }
            Message::ToggleScriptLock => {
                self.toggle_script_lock();
            }
            Message::StoryboardLimit(value) => self.max_shots = value,
            Message::StoryboardAspect(value) => self.aspect_ratio = value,
            Message::StoryboardPromptStyle(value) => self.prompt_style = value,
            Message::CopyPrompt(value) => {
                self.status = "提示词已复制到剪贴板".into();
                return iced::clipboard::write(value);
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
                            self.production.invalidate(0, Gate::Assets);
                            self.persist_production();
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
        if self.projects.is_empty() {
            return self.onboarding_view();
        }
        let page: Element<'_, Message> = match self.page {
            Page::Workbench => self.workbench_view(),
            Page::Script => self.script_view(),
            Page::Storyboard => self.storyboard_view(),
            Page::Assets => self.assets_view(),
            Page::Context => self.context_view(),
            Page::Events => self.events_view(),
            Page::Memory => self.memory_view(),
            Page::Runs => self.runs_view(),
            Page::Ai => self.settings_view(),
            Page::Prompts => self.prompts_view(),
        };
        let page: Element<'_, Message> = if self.new_project_open {
            column![
                self.new_project_panel(),
                container(page).height(Fill).width(Fill)
            ]
            .spacing(16)
            .into()
        } else {
            page
        };
        let page: Element<'_, Message> = if let Some((project, _)) = self.pending_delete {
            column![
                self.confirmation_bar(project),
                container(page).height(Fill).width(Fill)
            ]
            .spacing(12)
            .into()
        } else {
            page
        };
        let page: Element<'_, Message> = if let Some(preview) = self.run_preview_panel() {
            column![preview, container(page).height(Fill).width(Fill)]
                .spacing(12)
                .into()
        } else {
            page
        };
        let body = column![
            self.topbar(),
            row![
                self.rail(),
                container(page).width(Fill).height(Fill).padding([24, 28])
            ]
            .height(Fill),
            self.status_bar(),
        ];
        let shell = stack![
            container(space::horizontal())
                .width(Fill)
                .height(Fill)
                .style(ui::app),
            body,
        ]
        .width(Fill)
        .height(Fill);
        crate::shortcuts::save(shell, Message::Save)
    }

    fn current_stage(&self) -> (usize, StageState) {
        let states = self.stage_states();
        for (index, state) in states.iter().enumerate() {
            if *state != StageState::Done {
                return (index, *state);
            }
        }
        (4, StageState::Done)
    }

    fn onboarding_view(&self) -> Element<'_, Message> {
        let header = container(
            row![
                text("✦").size(18).color(ui::GOLD),
                text("星晴")
                    .size(19)
                    .font(ui::font(iced::font::Weight::Semibold))
                    .color(ui::TEXT),
                ui::eyebrow("AI 漫剧创作"),
                space::horizontal(),
                ui::chip("本地优先 · 不自动联网", ui::Tone::Muted),
            ]
            .spacing(10)
            .align_y(iced::Center),
        )
        .padding([16, 28])
        .style(ui::topbar);
        let features = [
            (
                "01",
                "故事基线",
                "先定世界观、人物与全剧大纲，避免边写边塌。",
            ),
            (
                "02",
                "剧本与资产",
                "文字、视觉设定与参考素材在同一条线上推进。",
            ),
            (
                "03",
                "分镜与生成",
                "镜头表是剧本通往画面的中枢，生成前必须确认。",
            ),
        ];
        let mut feature_list = column![].spacing(18);
        for (index, title, hint) in features {
            feature_list = feature_list.push(
                row![
                    text(index)
                        .size(11)
                        .font(ui::font(iced::font::Weight::Medium))
                        .color(ui::GOLD),
                    column![
                        text(title).size(15).color(ui::TEXT),
                        ui::muted(hint).size(12),
                    ]
                    .spacing(3)
                ]
                .spacing(14),
            );
        }
        let hero = column![
            ui::eyebrow("STARLIGHT STUDIO · 午夜片场"),
            ui::display("把一部小说，做成一部漫剧。"),
            text("星晴把故事、剧本、资产与分镜放在一条制作线上。每一步都有产物，每一道闸门都由你确认。")
                .size(15)
                .color(ui::TEXT_SECONDARY)
                .line_height(1.7),
            feature_list,
        ]
        .spacing(20)
        .width(Fill);
        let form = container(
            column![
                ui::eyebrow("NEW PROJECT"),
                ui::title("新建项目"),
                ui::muted("项目是设定的边界；分集是制作与运行的边界。"),
                column![
                    ui::eyebrow("项目名称"),
                    text_input("例如：星海来信", &self.project_name)
                        .on_input(Message::ProjectName)
                        .style(ui::field)
                        .padding([10, 12]),
                ]
                .spacing(6),
                column![
                    ui::eyebrow("一句话简介（选填）"),
                    text_input("这部片子讲什么？", &self.project_description)
                        .on_input(Message::NewProjectDescription)
                        .style(ui::field)
                        .padding([10, 12]),
                ]
                .spacing(6),
                button("创建项目")
                    .style(ui::primary)
                    .width(Fill)
                    .padding([11, 16])
                    .on_press_maybe(
                        (!self.project_name.trim().is_empty()).then_some(Message::CreateProject)
                    ),
                button("导入项目文件")
                    .style(ui::secondary)
                    .width(Fill)
                    .padding([10, 16])
                    .on_press(Message::ImportProject),
                ui::muted("创建后不会联网；已有 .xqproj 时可一步导入项目、分集、资产与设定。"),
            ]
            .spacing(16),
        )
        .padding(28)
        .width(420)
        .style(ui::panel);
        let body = container(
            row![hero, form]
                .spacing(72)
                .align_y(iced::Center)
                .padding([56, 72]),
        )
        .width(Fill)
        .height(Fill)
        .style(ui::app);
        stack![
            container(space::horizontal())
                .width(Fill)
                .height(Fill)
                .style(ui::app),
            column![header, body, self.status_bar()],
        ]
        .width(Fill)
        .height(Fill)
        .into()
    }

    fn brand(&self) -> Element<'_, Message> {
        row![
            text("✦").size(20).color(ui::GOLD),
            column![
                text("星晴")
                    .size(19)
                    .font(ui::font(iced::font::Weight::Semibold))
                    .color(ui::TEXT),
                ui::eyebrow("AI 漫剧创作"),
            ]
            .spacing(1),
        ]
        .spacing(10)
        .align_y(iced::Center)
        .into()
    }

    fn topbar(&self) -> Element<'_, Message> {
        let project = pick_list(
            self.projects.as_slice(),
            self.project.clone(),
            Message::Project,
        )
        .placeholder("选择项目")
        .width(200)
        .style(ui::pick);
        let mut pickers = row![column![ui::eyebrow("项目"), project].spacing(2)]
            .spacing(10)
            .align_y(iced::Center);
        if self.session.current.is_some() {
            let episodes = self.episode_choices();
            pickers = pickers.push(
                column![
                    ui::eyebrow("分集"),
                    pick_list(episodes, self.selected_episode_choice(), |choice| {
                        Message::Episode(choice.id, 0)
                    })
                    .width(250)
                    .style(ui::pick)
                ]
                .spacing(2),
            );
        }
        let (stage, state) = self.current_stage();
        let stage_chip = ui::chip(
            format!("{} · {}", STAGE_TITLES[stage], state.label()),
            state.tone(),
        );
        container(
            row![
                self.brand(),
                container(pickers).padding([0, 10]),
                space::horizontal(),
                stage_chip,
                button("导入项目")
                    .style(ui::secondary)
                    .padding([8, 14])
                    .on_press(Message::ImportProject),
                button("＋ 新建项目")
                    .style(ui::secondary)
                    .padding([8, 14])
                    .on_press(Message::ToggleNewProject),
            ]
            .spacing(14)
            .align_y(iced::Center),
        )
        .padding([0, 24])
        .height(64)
        .width(Fill)
        .style(ui::topbar)
        .into()
    }

    fn rail(&self) -> Element<'_, Message> {
        let groups: [(&str, &[Page]); 3] = [
            (
                "制作",
                &[
                    Page::Workbench,
                    Page::Script,
                    Page::Storyboard,
                    Page::Assets,
                ],
            ),
            ("知识", &[Page::Context, Page::Events, Page::Memory]),
            ("产出", &[Page::Runs]),
        ];
        let mut nav = column![].spacing(3);
        for (group, pages) in groups {
            nav = nav.push(container(ui::eyebrow(group)).padding([10, 10]).width(Fill));
            for page in pages {
                nav = nav.push(self.nav_item(*page));
            }
        }
        let settings = column![
            ui::divider(),
            container(ui::eyebrow("设置")).padding([10, 10]),
            self.nav_item(Page::Ai),
            self.nav_item(Page::Prompts),
            container(
                row![
                    container(text(""))
                        .width(6)
                        .height(6)
                        .style(|_: &Theme| container::Style {
                            background: Some(ui::MINT.into()),
                            border: iced::Border {
                                color: ui::MINT,
                                width: 0.0,
                                radius: 999.0.into(),
                            },
                            ..container::Style::default()
                        }),
                    ui::eyebrow("RUST · SQLITE · 本地"),
                ]
                .spacing(7)
                .align_y(iced::Center),
            )
            .padding([10, 10]),
        ]
        .spacing(3);
        container(column![
            scrollable(nav).height(Fill).style(ui::scrollable),
            settings
        ])
        .padding([12, 10])
        .width(228)
        .height(Fill)
        .style(ui::rail)
        .into()
    }

    fn nav_item(&self, page: Page) -> Element<'_, Message> {
        let active = self.page == page;
        button(
            row![
                text(if active { "▍" } else { " " })
                    .size(10)
                    .color(if active { ui::GOLD } else { Color::TRANSPARENT }),
                text(page.label()).size(13).color(if active {
                    ui::GOLD_BRIGHT
                } else {
                    ui::TEXT_SECONDARY
                }),
            ]
            .spacing(6)
            .align_y(iced::Center),
        )
        .width(Fill)
        .padding([9, 10])
        .style(ui::nav(active))
        .on_press(Message::Page(page))
        .into()
    }

    fn status_bar(&self) -> Element<'_, Message> {
        let mut right = row![ui::eyebrow("RUST · 本地工作区")]
            .spacing(10)
            .align_y(iced::Center);
        if let Some((_, run, _)) = &self.busy {
            right = right.push(ui::chip("生成中", ui::Tone::Gold));
            right = right.push(ui::muted(short(run, 12)).size(11));
        }
        container(
            row![
                container(text(""))
                    .width(6)
                    .height(6)
                    .style(|_: &Theme| container::Style {
                        background: Some(
                            if self.busy.is_some() {
                                ui::GOLD
                            } else {
                                ui::MINT
                            }
                            .into()
                        ),
                        border: iced::Border {
                            color: Color::TRANSPARENT,
                            width: 0.0,
                            radius: 999.0.into(),
                        },
                        ..container::Style::default()
                    }),
                text(short(&self.status, 96))
                    .size(12)
                    .color(ui::TEXT_SECONDARY),
                space::horizontal(),
                right,
            ]
            .spacing(8)
            .align_y(iced::Center),
        )
        .padding([0, 24])
        .height(30)
        .width(Fill)
        .style(ui::statusbar)
        .into()
    }

    fn page_heading<'a>(
        &'a self,
        title: impl Into<String>,
        subtitle: impl Into<String>,
        actions: Vec<Element<'a, Message>>,
    ) -> Element<'a, Message> {
        let mut action_row = row![].spacing(8).align_y(iced::Center);
        for action in actions {
            action_row = action_row.push(action);
        }
        row![
            column![
                ui::eyebrow(format!("{} · {}", self.page.eyebrow(), self.page.label())),
                ui::title(title),
                ui::muted(subtitle),
            ]
            .spacing(6),
            space::horizontal(),
            action_row,
        ]
        .align_y(iced::Center)
        .into()
    }

    fn run_preview_panel(&self) -> Option<Element<'_, Message>> {
        let pending = self.pending_run.as_ref()?;
        let project = self
            .project
            .as_ref()
            .map(|project| project.name.clone())
            .unwrap_or_else(|| "项目".into());
        let target = if pending.key.episode == 0 {
            format!("项目级 · {project}")
        } else {
            let episode = self
                .episodes
                .iter()
                .find(|episode| id(episode) == pending.key.episode);
            match episode {
                Some(episode) => format!(
                    "{project} · EP{:02} · {}",
                    episode_number(episode),
                    label(episode, "title")
                ),
                None => format!("{project} · EP{:02}", pending.key.episode),
            }
        };
        let max_tokens = self
            .settings
            .max_tokens
            .map(|tokens| format!("{tokens} tokens"))
            .unwrap_or_else(|| "默认".into());
        let preview_text = if pending.input.script.trim().is_empty() {
            "无正文输入（项目级动作）".into()
        } else {
            short(pending.input.script.trim(), 260)
        };
        let mut details = row![
            ui::chip("待确认生成", ui::Tone::Gold),
            ui::muted(target.clone()).size(12),
            ui::muted(format!(
                "{} · {}",
                provider_label(self.settings.provider),
                self.settings.model
            ))
            .size(12),
            ui::muted(format!("输入 {} 字", pending.input_chars)).size(12),
            ui::muted(format!("上下文 {} 字", pending.context_chars)).size(12),
            ui::muted(format!("最大输出 {max_tokens}")).size(12),
        ]
        .spacing(10)
        .align_y(iced::Center);
        if pending.input.action == "workflow_storyboard" {
            details = details
                .push(ui::muted(format!(
                    "最多 {} 镜 · {}",
                    pending.input.context["max_shots"].as_u64().unwrap_or(24),
                    pending.input.context["aspect_ratio"]
                        .as_str()
                        .unwrap_or("9:16")
                )))
                .spacing(10);
        }
        let mut actions = row![
            button("取消")
                .style(ui::secondary)
                .on_press(Message::CancelRun),
            button("确认生成")
                .style(ui::primary)
                .on_press_maybe((!self.settings.api_key.is_empty()).then_some(Message::ConfirmRun)),
            space::horizontal(),
            ui::muted(format!("Run {}", short(&pending.input.run_id, 12))),
        ]
        .spacing(10)
        .align_y(iced::Center);
        if self.settings.api_key.is_empty() {
            actions = row![
                button("取消")
                    .style(ui::secondary)
                    .on_press(Message::CancelRun),
                button("去配置 API Key")
                    .style(ui::primary)
                    .on_press(Message::Page(Page::Ai)),
                space::horizontal(),
                ui::muted("未配置密钥，确认生成不可用。"),
            ]
            .spacing(10)
            .align_y(iced::Center);
        }
        Some(
            container(
                column![
                    row![
                        column![
                            ui::eyebrow("GENERATION PREVIEW · 生成前确认"),
                            ui::heading(pending.action.clone()),
                        ]
                        .spacing(3),
                        space::horizontal(),
                        ui::chip("不确认不会调用", ui::Tone::Muted),
                    ]
                    .align_y(iced::Center),
                    details,
                    container(
                        text(preview_text)
                            .size(13)
                            .color(ui::TEXT_SECONDARY)
                            .line_height(1.65),
                    )
                    .padding(12)
                    .width(Fill)
                    .style(ui::card),
                    ui::muted("确认后才会调用模型；取消不会产生请求。费用以服务商账单为准，本次不做本地报价。"),
                    actions,
                ]
                .spacing(12),
            )
            .padding(16)
            .width(Fill)
            .style(ui::panel)
            .into(),
        )
    }

    fn confirmation_bar(&self, project: bool) -> Element<'_, Message> {
        container(
            row![
                ui::chip("需要确认", ui::Tone::Coral),
                text(if project {
                    "删除此项目及其全部内容？"
                } else {
                    "删除本集及其全部内容？"
                })
                .size(13)
                .color(ui::TEXT),
                space::horizontal(),
                button("取消")
                    .style(ui::secondary)
                    .on_press(Message::ConfirmDelete(false)),
                button("确认删除")
                    .style(ui::danger)
                    .on_press(Message::ConfirmDelete(true)),
            ]
            .spacing(12)
            .align_y(iced::Center),
        )
        .padding([10, 14])
        .width(Fill)
        .style(ui::danger_banner)
        .into()
    }

    fn new_project_panel(&self) -> Element<'_, Message> {
        container(
            column![
                row![
                    column![ui::eyebrow("NEW PROJECT"), ui::heading("新建项目"),].spacing(3),
                    space::horizontal(),
                    button("取消")
                        .style(ui::ghost)
                        .on_press(Message::ToggleNewProject),
                ]
                .align_y(iced::Center),
                row![
                    column![
                        ui::eyebrow("项目名称"),
                        text_input("例如：星海来信", &self.project_name)
                            .on_input(Message::ProjectName)
                            .style(ui::field)
                            .padding([9, 12]),
                    ]
                    .spacing(5)
                    .width(Fill),
                    column![
                        ui::eyebrow("一句话简介（选填）"),
                        text_input("这部片子讲什么？", &self.project_description)
                            .on_input(Message::NewProjectDescription)
                            .style(ui::field)
                            .padding([9, 12]),
                    ]
                    .spacing(5)
                    .width(Fill),
                    button("创建")
                        .style(ui::primary)
                        .padding([9, 18])
                        .on_press_maybe(
                            (!self.project_name.trim().is_empty())
                                .then_some(Message::CreateProject)
                        ),
                ]
                .spacing(12)
                .align_y(iced::Bottom),
            ]
            .spacing(14),
        )
        .padding(16)
        .width(Fill)
        .style(ui::panel)
        .into()
    }

    fn workbench_view(&self) -> Element<'_, Message> {
        let (stage_index, _stage_state) = self.current_stage();
        let title = self
            .current_episode()
            .map(|episode| label(episode, "title").to_owned())
            .or_else(|| self.project.as_ref().map(|project| project.name.clone()))
            .unwrap_or_else(|| "未命名项目".into());
        let subtitle = match (self.project.as_ref(), self.current_episode()) {
            (Some(project), Some(episode)) => format!(
                "{} · EP{:02} · {}",
                project.name,
                episode_number(episode),
                STAGE_TITLES[stage_index]
            ),
            (Some(project), None) => format!("{} · 先新增一集，再开始制作", project.name),
            _ => "本地工作区".into(),
        };
        let mut actions: Vec<Element<'_, Message>> = vec![];
        if self.session.current.is_some() {
            actions.push(
                button("继续写作")
                    .style(ui::primary)
                    .padding([9, 16])
                    .on_press(Message::ScriptStep(0))
                    .into(),
            );
        } else {
            actions.push(
                button("新增一集")
                    .style(ui::primary)
                    .padding([9, 16])
                    .on_press(Message::CreateEpisode)
                    .into(),
            );
        }
        actions.push(
            button("生成记录")
                .style(ui::secondary)
                .padding([9, 14])
                .on_press(Message::Page(Page::Runs))
                .into(),
        );
        if self
            .commit_key()
            .is_some_and(|key| self.pending_commits.contains_key(&key))
        {
            actions.push(
                button("重试写入")
                    .style(ui::danger)
                    .padding([9, 14])
                    .on_press(Message::ApplyArtifacts)
                    .into(),
            );
        }
        let mut content = column![
            self.page_heading(title, subtitle, actions),
            self.workbench_stats(),
            self.stage_row(),
            self.workbench_lower(),
            self.recent_runs(),
            self.project_management(),
        ]
        .spacing(20);
        if self.busy.is_some() {
            content = column![self.busy_banner(), content].spacing(16);
        }
        if self
            .commit_key()
            .is_some_and(|key| self.pending_commits.contains_key(&key))
        {
            content = column![self.pending_banner(), content].spacing(16);
        }
        if let Some(message) = self.paused_banner() {
            content = column![message, content].spacing(16);
        }
        scrollable(content)
            .height(Fill)
            .style(ui::scrollable)
            .into()
    }

    fn workbench_stats(&self) -> Element<'_, Message> {
        let states = self.stage_states();
        let done = states
            .iter()
            .filter(|state| **state == StageState::Done)
            .count();
        let chars = self.script_chars();
        let seconds = self.estimated_seconds();
        let duration = if seconds >= 60 {
            format!("{} 分 {:02} 秒", seconds / 60, seconds % 60)
        } else {
            format!("约 {seconds} 秒")
        };
        let shots = self.shots().map(Vec::len).unwrap_or(0);
        let cards = [
            (
                "制作进度",
                format!("{done} / 5"),
                format!("{} 个阶段已完成", done),
            ),
            ("本集字数", format!("{chars}"), "按 4.1 字/秒估算".into()),
            ("预估时长", duration, "只作时长预算".into()),
            (
                "分镜镜头",
                format!("{shots}"),
                if self
                    .production
                    .episode(self.session.current.map(|k| k.episode).unwrap_or(0))
                    .storyboard_approved
                {
                    "已确认定稿".into()
                } else {
                    "定稿后进入生成".into()
                },
            ),
        ];
        let mut row = row![].spacing(12);
        for (label, value, hint) in cards {
            row = row.push(
                container(
                    column![
                        ui::eyebrow(label),
                        ui::number(value),
                        ui::muted(hint).size(11),
                    ]
                    .spacing(6),
                )
                .padding(16)
                .width(Fill)
                .style(ui::panel),
            );
        }
        row.into()
    }

    fn stage_row(&self) -> Element<'_, Message> {
        let states = self.stage_states();
        let details = [
            if self.has_outline() {
                if self.production.outline_approved {
                    "全剧大纲已确认".into()
                } else {
                    "全剧大纲已生成，等待确认".into()
                }
            } else if self.has_series_bible() {
                "故事设定已有，缺全剧大纲".into()
            } else {
                "还没有故事设定".into()
            },
            if self.episode_locked() {
                format!("已锁定 · {} 字", self.script_chars())
            } else if self.script_chars() > 0 {
                format!("{} 字 · 待锁定", self.script_chars())
            } else {
                "还没有本集剧本".into()
            },
            if self.has_structure() {
                "结构产物已生成，等待确认".into()
            } else {
                "尚未拆解场次与节拍".into()
            },
            if self.production.assets_frozen {
                format!("已冻结 · {} 个资产", self.items.len())
            } else if self.has_assets() {
                format!("{} 个资产 · 待冻结", self.items.len())
            } else {
                "还没有角色或场景资产".into()
            },
            if self.has_shots() {
                format!(
                    "共 {} 镜 · {}",
                    self.shots().map(Vec::len).unwrap_or(0),
                    if self
                        .production
                        .episode(self.session.current.map(|k| k.episode).unwrap_or(0))
                        .storyboard_approved
                    {
                        "已定稿"
                    } else {
                        "待确认"
                    }
                )
            } else {
                "尚未生成镜头表".into()
            },
        ];
        let targets = [
            (Message::Page(Page::Context), "进入"),
            (Message::ScriptStep(0), "写作"),
            (Message::ScriptStep(1), "拆解"),
            (Message::Page(Page::Assets), "资产"),
            (Message::Page(Page::Storyboard), "分镜"),
        ];
        let gates = [
            Some(Gate::Outline),
            None,
            Some(Gate::Structure),
            Some(Gate::Assets),
            Some(Gate::Storyboard),
        ];
        let mut cards = row![].spacing(10);
        for index in 0..5 {
            cards = cards.push(self.stage_card(
                index,
                states[index],
                details[index].clone(),
                targets[index].0.clone(),
                targets[index].1,
                gates[index],
            ));
        }
        column![
            row![ui::heading("制作流程"), space::horizontal()].align_y(iced::Center),
            cards,
        ]
        .spacing(12)
        .into()
    }

    fn stage_card(
        &self,
        index: usize,
        state: StageState,
        detail: String,
        target: Message,
        action: &'static str,
        gate: Option<Gate>,
    ) -> Element<'_, Message> {
        let border = match state {
            StageState::Done => ui::MINT,
            StageState::Review => ui::GOLD,
            StageState::Active => ui::INDIGO,
            StageState::Empty => ui::LINE_SOFT,
        };
        let background = match state {
            StageState::Review => ui::GOLD_SOFT,
            StageState::Done => Color::from_rgba8(0x6F, 0xD3, 0xA8, 0.04),
            _ => ui::SURFACE,
        };
        let mut footer = row![
            button(action)
                .style(ui::ghost)
                .padding([5, 0])
                .on_press(target)
        ]
        .spacing(6)
        .align_y(iced::Center);
        if state == StageState::Review
            && let Some(gate) = gate
        {
            footer = footer.push(
                button(gate.label())
                    .style(ui::primary)
                    .padding([5, 10])
                    .on_press(Message::ToggleGate(gate)),
            );
        }
        container(
            column![
                row![
                    text(format!("{:02}", index + 1))
                        .size(11)
                        .font(ui::font(iced::font::Weight::Medium))
                        .color(if state == StageState::Review {
                            ui::GOLD
                        } else {
                            ui::TEXT_MUTED
                        }),
                    space::horizontal(),
                    ui::chip(state.label(), state.tone()),
                ]
                .align_y(iced::Center),
                text(STAGE_TITLES[index])
                    .size(17)
                    .font(ui::font(iced::font::Weight::Semibold))
                    .color(ui::TEXT),
                ui::muted(detail).size(12),
                space::vertical(),
                footer,
            ]
            .spacing(6),
        )
        .padding(14)
        .width(Fill)
        .height(172)
        .style(move |_: &Theme| container::Style {
            background: Some(background.into()),
            border: iced::Border {
                color: border,
                width: 1.0,
                radius: 12.0.into(),
            },
            text_color: Some(ui::TEXT),
            ..container::Style::default()
        })
        .into()
    }

    fn workbench_lower(&self) -> Element<'_, Message> {
        let script = self.current_script();
        let preview = if script.trim().is_empty() {
            "还没有正文。写下第一段，或者从原文粘贴。".into()
        } else {
            short(script.trim(), 260)
        };
        let title = self
            .current_episode()
            .map(|episode| label(episode, "title").to_owned())
            .unwrap_or_else(|| "本集剧本".into());
        let script_card = container(
            column![
                row![
                    ui::eyebrow("本集剧本"),
                    space::horizontal(),
                    ui::chip(format!("{} 字", self.script_chars()), ui::Tone::Muted),
                ]
                .align_y(iced::Center),
                ui::heading(title),
                text(preview)
                    .size(13)
                    .color(ui::TEXT_SECONDARY)
                    .line_height(1.75),
                space::vertical(),
                row![
                    ui::muted(format!(
                        "约 {} 秒 · {} 场",
                        self.estimated_seconds(),
                        self.scene_count()
                    )),
                    space::horizontal(),
                    button("打开剧本工坊 →")
                        .style(ui::ghost)
                        .on_press(Message::ScriptStep(0)),
                ]
                .align_y(iced::Center),
            ]
            .spacing(10),
        )
        .padding(18)
        .width(Fill)
        .height(252)
        .style(ui::panel);
        let next = container(self.next_steps())
            .padding(18)
            .width(352)
            .height(252)
            .style(ui::panel);
        row![script_card, next].spacing(12).into()
    }

    fn next_steps(&self) -> Element<'_, Message> {
        struct Step {
            title: &'static str,
            hint: &'static str,
            message: Message,
        }
        let mut steps = vec![];
        if !self.has_outline() {
            steps.push(Step {
                title: "完成故事基线",
                hint: "世界观、人物与全剧大纲",
                message: Message::Page(Page::Context),
            });
        }
        if self.script_chars() == 0 {
            steps.push(Step {
                title: "写下第一版剧本",
                hint: "先完成可拍的文字，再谈资产",
                message: Message::ScriptStep(0),
            });
        } else if !self.episode_locked() {
            steps.push(Step {
                title: "锁定本集剧本",
                hint: "锁定后可进入结构拆解与分镜",
                message: Message::ToggleScriptLock,
            });
        }
        if !self.has_structure() {
            steps.push(Step {
                title: "拆解本集结构",
                hint: "场次、节拍与可拍性检查",
                message: Message::Run("episode_outline_generate".into()),
            });
        }
        if !self.has_assets() {
            steps.push(Step {
                title: "提取角色与场景",
                hint: "从本集剧本识别资产",
                message: Message::Run("episode_assets_visual_dna".into()),
            });
        } else if !self.production.assets_frozen {
            steps.push(Step {
                title: "冻结资产",
                hint: "冻结后分镜会读取一致的视觉设定",
                message: Message::ToggleGate(Gate::Assets),
            });
        }
        if !self.has_shots() {
            steps.push(Step {
                title: "生成镜头表",
                hint: "把剧本编译成可检查的镜头",
                message: Message::Page(Page::Storyboard),
            });
        } else if !self
            .production
            .episode(self.session.current.map(|k| k.episode).unwrap_or(0))
            .storyboard_approved
        {
            steps.push(Step {
                title: "确认分镜定稿",
                hint: "定稿后进入关键帧与生成",
                message: Message::ToggleGate(Gate::Storyboard),
            });
        }
        let total = steps.len();
        let mut list = column![
            row![
                ui::eyebrow("下一步"),
                space::horizontal(),
                ui::muted(format!("{total} 项")),
            ]
            .align_y(iced::Center)
        ]
        .spacing(10);
        if steps.is_empty() {
            list = list.push(
                column![
                    ui::heading("本集已就绪"),
                    ui::muted("关键帧、配音与合成属于下一阶段，目前尚未接入。"),
                ]
                .spacing(6),
            );
        } else {
            for (index, step) in steps.into_iter().take(3).enumerate() {
                list = list.push(
                    row![
                        text(format!("{:02}", index + 1))
                            .size(11)
                            .color(ui::GOLD)
                            .font(ui::font(iced::font::Weight::Medium)),
                        column![
                            text(step.title).size(13).color(ui::TEXT),
                            ui::muted(step.hint).size(11),
                        ]
                        .spacing(2)
                        .width(Fill),
                        button("→").style(ui::ghost).padding([4, 7]).on_press_maybe(
                            (self.busy.is_none() || !matches!(&step.message, Message::Run(_)))
                                .then_some(step.message)
                        ),
                    ]
                    .spacing(9)
                    .align_y(iced::Center),
                );
            }
            if total > 3 {
                list = list.push(ui::muted(format!(
                    "还有 {} 项，完成当前步骤后会出现在这里。",
                    total - 3
                )));
            }
        }
        list.into()
    }

    fn recent_runs(&self) -> Element<'_, Message> {
        let mut list = column![
            row![
                ui::heading("最近生成"),
                space::horizontal(),
                button("全部记录 →")
                    .style(ui::ghost)
                    .on_press(Message::Page(Page::Runs)),
            ]
            .align_y(iced::Center)
        ]
        .spacing(10);
        if self.run_list.is_empty() {
            list = list.push(
                container(ui::muted(
                    "还没有生成记录。第一次运行会出现在这里，并保留阶段产物。",
                ))
                .padding(16)
                .width(Fill)
                .style(ui::card),
            );
        } else {
            for run in self.run_list.iter().take(3) {
                let action = {
                    let value = label(run, "final_action_key");
                    if value.is_empty() {
                        label(run, "action")
                    } else {
                        value
                    }
                };
                let status = label(run, "status");
                list = list.push(
                    container(
                        row![
                            column![
                                text(action_label(if action.is_empty() {
                                    "运行"
                                } else {
                                    action
                                }))
                                .size(13)
                                .color(ui::TEXT),
                                ui::muted(format!(
                                    "{} · {}",
                                    short(label(run, "run_id"), 10),
                                    relative_time(run["created_at_ms"].as_u64().unwrap_or(0))
                                ))
                                .size(11),
                            ]
                            .spacing(3)
                            .width(Fill),
                            ui::chip(run_status_label(status), run_status_tone(status)),
                            button("查看")
                                .style(ui::ghost)
                                .on_press(Message::SelectRun(label(run, "run_id").into())),
                        ]
                        .spacing(12)
                        .align_y(iced::Center),
                    )
                    .padding([11, 14])
                    .width(Fill)
                    .style(ui::card),
                );
            }
        }
        list.into()
    }

    fn project_management(&self) -> Element<'_, Message> {
        let mut actions = row![].spacing(8).align_y(iced::Center);
        if let Some(key) = self.session.current {
            actions = actions.push(
                button("删除本集")
                    .style(ui::danger)
                    .padding([6, 10])
                    .on_press(Message::DeleteEpisode(key.episode)),
            );
        }
        actions = actions.push(
            button("导出项目")
                .style(ui::secondary)
                .padding([6, 10])
                .on_press(Message::ExportProject),
        );
        actions = actions.push(
            button("删除项目")
                .style(ui::danger)
                .padding([6, 10])
                .on_press(Message::DeleteProject),
        );
        container(
            row![
                column![
                    ui::eyebrow("项目管理"),
                    ui::muted("导出为单个 .xqproj（不含运行历史与记忆）；删除会移除该范围内的剧本、资产关联与运行引用，操作前会再次确认。"),
                ]
                .spacing(3),
                space::horizontal(),
                actions,
            ]
            .align_y(iced::Center),
        )
        .padding(14)
        .width(Fill)
        .style(ui::card)
        .into()
    }

    fn busy_banner(&self) -> Element<'_, Message> {
        let run = self
            .busy
            .as_ref()
            .map(|(_, run, _)| run.clone())
            .unwrap_or_default();
        container(
            row![
                ui::chip("执行中", ui::Tone::Gold),
                ui::muted(format!(
                    "正在运行 {}，阶段产物会实时写入运行记录。",
                    short(&run, 12)
                )),
                space::horizontal(),
                button("查看过程")
                    .style(ui::secondary)
                    .on_press(Message::SelectRun(run)),
                button("取消执行")
                    .style(ui::danger)
                    .on_press(Message::Cancel),
            ]
            .spacing(12)
            .align_y(iced::Center),
        )
        .padding([10, 14])
        .width(Fill)
        .style(ui::banner)
        .into()
    }

    fn pending_banner(&self) -> Element<'_, Message> {
        container(
            row![
                ui::chip("待写入", ui::Tone::Coral),
                ui::muted("生成结果已保留，但写入本地失败。可以重试，不需要重新生成。"),
                space::horizontal(),
                button("重试写入")
                    .style(ui::primary)
                    .on_press(Message::ApplyArtifacts),
            ]
            .spacing(12)
            .align_y(iced::Center),
        )
        .padding([10, 14])
        .width(Fill)
        .style(ui::danger_banner)
        .into()
    }

    fn paused_banner(&self) -> Option<Element<'_, Message>> {
        let (key, _) = self.paused.as_ref()?;
        if self.busy.is_some() || self.session.current != Some(*key) {
            return None;
        }
        Some(
            container(
                row![
                    ui::chip("待审核", ui::Tone::Gold),
                    ui::muted("生成已暂停。检查当前结果后再继续，继续会沿用同一个 Run。"),
                    space::horizontal(),
                    button("确认继续")
                        .style(ui::primary)
                        .on_press(Message::Review(true)),
                    button("拒绝")
                        .style(ui::secondary)
                        .on_press(Message::Review(false)),
                ]
                .spacing(12)
                .align_y(iced::Center),
            )
            .padding([10, 14])
            .width(Fill)
            .style(ui::banner)
            .into(),
        )
    }
    fn script_view(&self) -> Element<'_, Message> {
        if self.session.current.is_none() {
            return self.script_empty_view();
        }
        let locked = self.episode_locked();
        let dirty = self.session.draft().is_some_and(|draft| draft.dirty);
        let order = self.current_episode().map(episode_number).unwrap_or(0);
        let project = self
            .project
            .as_ref()
            .map(|project| project.name.clone())
            .unwrap_or_default();
        let mut actions: Vec<Element<'_, Message>> = vec![
            button("保存")
                .style(ui::secondary)
                .padding([8, 15])
                .on_press(Message::Save)
                .into(),
        ];
        actions.push(
            button(if locked {
                "解锁剧本"
            } else {
                "锁定剧本"
            })
            .style(if locked { ui::secondary } else { ui::primary })
            .padding([8, 15])
            .on_press(Message::ToggleScriptLock)
            .into(),
        );
        let mut subtitle = format!(
            "{} · EP{:02} · {} 字 · 约 {} 秒 · {} 场",
            project,
            order,
            self.script_chars(),
            self.estimated_seconds(),
            self.scene_count()
        );
        if self.session.draft().is_some_and(|draft| draft.dirty) {
            subtitle.push_str(" · 有未保存修改");
        }
        let header = row![
            column![
                ui::eyebrow(format!("{} · {}", self.page.eyebrow(), self.page.label())),
                text_input("未命名这一集", &self.title)
                    .on_input(Message::Title)
                    .size(23)
                    .padding([0, 0])
                    .style(ui::title_field)
                    .width(Fill),
                row![
                    ui::muted(subtitle),
                    ui::chip(
                        if locked { "已锁定" } else { "可编辑" },
                        if locked {
                            ui::Tone::Mint
                        } else {
                            ui::Tone::Muted
                        }
                    ),
                    ui::chip(
                        if dirty { "未保存" } else { "已保存" },
                        if dirty {
                            ui::Tone::Gold
                        } else {
                            ui::Tone::Muted
                        }
                    ),
                ]
                .spacing(10)
                .align_y(iced::Center),
            ]
            .spacing(6)
            .width(Fill),
            row(actions).spacing(8).align_y(iced::Center),
        ]
        .align_y(iced::Center);
        let tabs = row![
            self.step_button(0, "01  剧本"),
            self.step_button(1, "02  结构"),
            self.step_button(2, "03  资产"),
            space::horizontal(),
            button("运行记录 →")
                .style(ui::ghost)
                .on_press(Message::Page(Page::Runs)),
        ]
        .spacing(8)
        .align_y(iced::Center);
        let mut content = column![header, tabs].spacing(14);
        if let Some(banner) = self.paused_banner() {
            content = content.push(banner);
        }
        if self.busy.is_some() {
            content = content.push(self.busy_banner());
        }
        if self
            .commit_key()
            .is_some_and(|key| self.pending_commits.contains_key(&key))
        {
            content = content.push(self.pending_banner());
        }
        let workspace: Element<'_, Message> = if self.step == 0 {
            row![self.editor_panel(), self.ai_panel()]
                .spacing(12)
                .height(Fill)
                .into()
        } else {
            row![self.board_panel(), self.inspector_panel()]
                .spacing(12)
                .height(Fill)
                .into()
        };
        content = content.push(workspace);
        content.height(Fill).into()
    }

    fn script_empty_view(&self) -> Element<'_, Message> {
        container(
            column![
                ui::eyebrow("SCRIPT · 剧本工坊"),
                ui::display("先选一集，再开始写。"),
                ui::muted("左栏的制作台可以创建新集；分集是剧本、资产与运行的边界。"),
                button("回到制作台")
                    .style(ui::primary)
                    .on_press(Message::Page(Page::Workbench)),
            ]
            .spacing(14)
            .align_x(iced::Center),
        )
        .center(Fill)
        .into()
    }

    fn step_button(&self, step: u8, label: &'static str) -> Element<'_, Message> {
        button(label)
            .style(if self.step == step {
                ui::primary
            } else {
                ui::secondary
            })
            .padding([8, 14])
            .on_press(Message::ScriptStep(step))
            .into()
    }

    fn field_editor_height(text: &str, min: f32, chars_per_line: f32) -> f32 {
        let lines = text
            .lines()
            .map(|line| 1.0 + (line.chars().count() as f32 / chars_per_line).floor())
            .sum::<f32>()
            .max(1.0);
        (lines * 20.8 + 28.0).clamp(min, 40_000.0)
    }

    fn editor_height(&self) -> f32 {
        let script = self.current_script();
        let mut lines = 0.0;
        for line in script.lines() {
            let chars = line.chars().count() as f32;
            lines += 1.0 + (chars / 44.0).floor();
        }
        (lines * 26.25 + 80.0).clamp(520.0, 20_000.0)
    }

    fn editor_panel(&self) -> Element<'_, Message> {
        let height = self.editor_height();
        let editor = ReadableEditor::new(
            text_editor(&self.editor)
                .placeholder("从第一场开始写。动作、对白、OS/VO 都可以先用自然段记录。")
                .height(height)
                .on_action(Message::Edit)
                .padding(20)
                .size(15)
                .line_height(1.75)
                .style(ui::editor),
            self.editor.text(),
        )
        .padding(20.0)
        .size(15.0)
        .line_height(26.25)
        .color(ui::TEXT)
        .into_element();
        let content = column![
            row![
                ui::eyebrow("正文"),
                space::horizontal(),
                ui::muted("Cmd/Ctrl + S 保存"),
            ]
            .align_y(iced::Center),
            scrollable(editor)
                .id("script-editor")
                .height(Fill)
                .style(ui::scrollable),
        ]
        .spacing(10)
        .padding(14);
        ui::panel_surface(content)
    }

    fn ai_panel(&self) -> Element<'_, Message> {
        let mut panel = column![
            row![
                ui::heading("AI 工坊"),
                space::horizontal(),
                ui::chip("只写新 Run", ui::Tone::Muted),
            ]
            .align_y(iced::Center),
            ui::muted("生成会先保存当前剧本，旧版本与阶段记录都会保留。"),
        ]
        .spacing(10);
        if self
            .artifacts()
            .is_some_and(|artifacts| artifacts["script"].is_string())
        {
            panel = panel.push(
                container(
                    column![
                        ui::muted("已有一版生成剧本，可放入编辑器检查。"),
                        button("应用生成剧本")
                            .style(ui::secondary)
                            .width(Fill)
                            .on_press(Message::ApplyScript),
                    ]
                    .spacing(7),
                )
                .padding(10)
                .width(Fill)
                .style(ui::card),
            );
        }
        if self.settings.api_key.is_empty() {
            panel = panel.push(
                container(
                    column![
                        ui::muted("尚未配置 API Key，运行会失败。"),
                        button("去配置 AI 服务")
                            .style(ui::secondary)
                            .on_press(Message::Page(Page::Ai)),
                    ]
                    .spacing(8),
                )
                .padding(10)
                .width(Fill)
                .style(ui::card),
            );
        }
        panel = panel
            .push(self.ai_action(
                "优化剧本",
                "收紧对白与动作，保留剧情与人物口吻。",
                "script_optimize",
            ))
            .push(self.ai_action(
                "完整改稿",
                "结构、写作与质检一次完成，耗时较长。",
                "workflow_script",
            ))
            .push(self.ai_action(
                "结构拆解",
                "把剧本拆成可拍的场次与节拍。",
                "episode_outline_generate",
            ))
            .push(self.ai_action(
                "提取资产",
                "从本集识别角色、场景与道具，写入资产库。",
                "episode_assets_visual_dna",
            ));
        if self.busy.is_some() {
            panel = panel.push(
                button("取消当前执行")
                    .style(ui::danger)
                    .width(Fill)
                    .on_press(Message::Cancel),
            );
        }
        panel = panel.push(
            button("查看全部运行记录 →")
                .style(ui::ghost)
                .on_press(Message::Page(Page::Runs)),
        );
        container(panel)
            .padding(16)
            .width(340)
            .height(Fill)
            .style(ui::panel)
            .into()
    }

    fn ai_action(
        &self,
        title: &'static str,
        hint: &'static str,
        action: &'static str,
    ) -> Element<'_, Message> {
        container(
            column![
                row![
                    text(title).size(13).color(ui::TEXT),
                    space::horizontal(),
                    button("运行")
                        .style(ui::secondary)
                        .padding([5, 12])
                        .on_press_maybe(self.busy.is_none().then_some(Message::Run(action.into()))),
                ]
                .align_y(iced::Center),
                ui::muted(hint).size(11),
            ]
            .spacing(6),
        )
        .padding(11)
        .width(Fill)
        .style(ui::card)
        .into()
    }

    fn board_panel(&self) -> Element<'_, Message> {
        let cards = self.cards();
        container(
            column![
                row![
                    ui::eyebrow(match self.step {
                        2 => "资产画布",
                        _ => "结构画布",
                    }),
                    space::horizontal(),
                    ui::muted("拖动卡片调整位置 · 滚轮缩放"),
                ]
                .align_y(iced::Center),
                iced::widget::canvas(Board {
                    cards,
                    view: self.board.clone(),
                })
                .width(Fill)
                .height(Fill),
            ]
            .spacing(10),
        )
        .padding(14)
        .width(Fill)
        .height(Fill)
        .style(ui::panel)
        .into()
    }

    fn inspector_panel(&self) -> Element<'_, Message> {
        let content = column![
            ui::eyebrow("内容详情"),
            scrollable(
                text(self.editor.text())
                    .size(14)
                    .color(ui::TEXT_SECONDARY)
                    .line_height(1.7)
                    .width(Fill),
            )
            .height(Fill)
            .style(ui::scrollable),
        ]
        .spacing(10)
        .padding(14);
        container(ui::panel_surface(content))
            .width(420)
            .height(Fill)
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
                                body: readable(item),
                            }),
                            _ => visit(item, heading, cards),
                        }
                    }
                }
                Value::Object(fields) => {
                    for (key, item) in fields {
                        visit(item, &field_label(key), cards);
                    }
                }
                _ => {}
            }
        }
        if self.step == 2 && value["series_style"].is_object() {
            cards.push(BoardCard {
                title: "全局视觉风格".into(),
                body: readable(&value["series_style"]),
            });
        }
        visit(value, "内容", &mut cards);
        if cards.is_empty() {
            cards.push(BoardCard {
                title: "生成结果".into(),
                body: readable(value),
            });
        }
        cards
    }
    fn storyboard_view(&self) -> Element<'_, Message> {
        let shots = self.shots().cloned().unwrap_or_default();
        let episode = self.session.current.map(|key| key.episode).unwrap_or(0);
        let approved = self.production.episode(episode).storyboard_approved;
        let mut actions: Vec<Element<'_, Message>> = vec![];
        actions.push(
            button(if shots.is_empty() {
                "生成分镜"
            } else {
                "重新生成"
            })
            .style(if shots.is_empty() {
                ui::primary
            } else {
                ui::secondary
            })
            .padding([9, 16])
            .on_press_maybe(
                self.busy
                    .is_none()
                    .then_some(Message::Run("workflow_storyboard".into())),
            )
            .into(),
        );
        if !shots.is_empty() {
            actions.push(
                button(if approved {
                    "撤销定稿"
                } else {
                    "确认定稿"
                })
                .style(if approved { ui::secondary } else { ui::primary })
                .padding([9, 16])
                .on_press(Message::ToggleGate(Gate::Storyboard))
                .into(),
            );
        }
        let subtitle = if shots.is_empty() {
            "镜头表还空着：先生成，再逐镜检查".into()
        } else {
            format!(
                "共 {} 镜 · {}",
                shots.len(),
                if approved { "已定稿" } else { "待确认" }
            )
        };
        let mut content = column![
            self.page_heading(
                self.current_episode()
                    .map(|episode| label(episode, "title").to_owned())
                    .unwrap_or_else(|| "镜头表".into()),
                subtitle,
                actions,
            )
        ]
        .spacing(16);
        content = content.push(self.preconditions());
        content = content.push(self.storyboard_parameters());
        if self.busy.is_some() {
            content = content.push(self.busy_banner());
        }
        if self.nothing_locked() {
            content = content.push(
                container(
                    row![
                        ui::chip("前置条件", ui::Tone::Gold),
                        ui::muted("剧本尚未锁定。生成仍会先保存当前剧本，但建议先完成锁定。"),
                        space::horizontal(),
                        button("锁定剧本")
                            .style(ui::secondary)
                            .on_press(Message::ToggleScriptLock),
                    ]
                    .spacing(12)
                    .align_y(iced::Center),
                )
                .padding([10, 14])
                .width(Fill)
                .style(ui::banner),
            );
        }
        if shots.is_empty() {
            content = content.push(self.storyboard_empty());
        } else {
            let mut list = column![].spacing(10);
            for (index, shot) in shots.iter().enumerate() {
                list = list.push(Self::shot_row(index, shot));
            }
            content = content.push(list);
        }
        scrollable(content)
            .height(Fill)
            .style(ui::scrollable)
            .into()
    }

    fn nothing_locked(&self) -> bool {
        self.session.current.is_some() && !self.episode_locked()
    }

    fn preconditions(&self) -> Element<'_, Message> {
        let dna = self.visual_dna_ids.len();
        container(
            row![
                ui::chip(
                    if self.episode_locked() {
                        "剧本已锁定"
                    } else {
                        "剧本未锁定"
                    },
                    if self.episode_locked() {
                        ui::Tone::Mint
                    } else {
                        ui::Tone::Gold
                    },
                ),
                ui::chip(
                    if self.production.assets_frozen {
                        "资产已冻结"
                    } else {
                        "资产未冻结"
                    },
                    if self.production.assets_frozen {
                        ui::Tone::Mint
                    } else {
                        ui::Tone::Gold
                    },
                ),
                ui::chip(format!("视觉设定 {dna} 项"), ui::Tone::Indigo),
                ui::chip(
                    format!("分镜 {} 镜", self.shots().map(Vec::len).unwrap_or(0)),
                    ui::Tone::Indigo,
                ),
                space::horizontal(),
                ui::muted("闸门只是检查点；不会阻止你预览生成参数。"),
            ]
            .spacing(10)
            .align_y(iced::Center),
        )
        .padding([8, 12])
        .width(Fill)
        .style(ui::card)
        .into()
    }

    fn storyboard_parameters(&self) -> Element<'_, Message> {
        const ASPECTS: [&str; 3] = ["9:16", "16:9", "1:1"];
        const STYLES: [&str; 2] = ["通用提示词", "Midjourney v6"];
        container(
            row![
                column![
                    ui::eyebrow("最多镜头"),
                    text_input("24", &self.max_shots)
                        .on_input(Message::StoryboardLimit)
                        .style(ui::field)
                        .padding([8, 10])
                        .width(84),
                ]
                .spacing(5),
                column![
                    ui::eyebrow("画幅"),
                    pick_list(
                        ASPECTS.as_slice(),
                        Some(self.aspect_ratio.as_str()),
                        |value| Message::StoryboardAspect(value.into())
                    )
                    .style(ui::pick)
                    .width(112),
                ]
                .spacing(5),
                column![
                    ui::eyebrow("提示词风格"),
                    pick_list(
                        STYLES.as_slice(),
                        Some(self.prompt_style.as_str()),
                        |value| Message::StoryboardPromptStyle(value.into())
                    )
                    .style(ui::pick)
                    .width(176),
                ]
                .spacing(5),
                space::horizontal(),
                ui::muted("提示词由镜头字段与已确认视觉设定编译。"),
            ]
            .spacing(16)
            .align_y(iced::Bottom),
        )
        .padding(14)
        .width(Fill)
        .style(ui::panel)
        .into()
    }

    fn storyboard_empty(&self) -> Element<'_, Message> {
        container(
            column![
                ui::eyebrow("STORYBOARD · 空白"),
                ui::display("分镜表还空着。"),
                text("生成前建议先锁定剧本、冻结资产。分镜会把剧本编译成可检查的镜头行，并保留在运行记录里。")
                    .size(14)
                    .color(ui::TEXT_SECONDARY)
                    .line_height(1.7),
                row![
                    button("生成分镜")
                        .style(ui::primary)
                        .padding([9, 16])
                        .on_press_maybe(
                            self.busy
                                .is_none()
                                .then_some(Message::Run("workflow_storyboard".into()))
                        ),
                    button("去资产库")
                        .style(ui::secondary)
                        .on_press(Message::Page(Page::Assets)),
                    button("回到剧本")
                        .style(ui::ghost)
                        .on_press(Message::ScriptStep(0)),
                ]
                .spacing(10),
            ]
            .spacing(14)
            .align_x(iced::Center),
        )
        .padding([52, 32])
        .center_x(Fill)
        .style(ui::panel)
        .into()
    }

    fn shot_row(index: usize, shot: &Value) -> Element<'static, Message> {
        let size = label(shot, "shot_size");
        let angle = label(shot, "camera_angle");
        let light = label(shot, "lighting_style");
        let prompt = label(shot, "prompt").to_owned();
        let negative = label(shot, "negative_prompt").to_owned();
        let title = label(shot, "title").to_owned();
        let action = label(shot, "action_text").to_owned();
        let dialogue = label(shot, "dialogue").to_owned();
        let mut meta = row![
            ui::chip(shot_size_label(size), ui::Tone::Indigo),
            ui::chip(camera_angle_label(angle), ui::Tone::Muted),
            ui::chip(lighting_label(light), ui::Tone::Muted),
        ]
        .spacing(6)
        .align_y(iced::Center);
        if prompt.is_empty() {
            meta = meta.push(ui::chip("缺提示词", ui::Tone::Coral));
        }
        let mut center = column![
            row![
                ui::heading(if title.is_empty() {
                    format!("镜头 {:02}", index + 1)
                } else {
                    title.clone()
                }),
                space::horizontal(),
                meta,
            ]
            .align_y(iced::Center),
            text(action.clone())
                .size(13)
                .color(ui::TEXT_SECONDARY)
                .line_height(1.6),
        ]
        .spacing(7)
        .width(Fill);
        if !dialogue.is_empty() {
            center = center.push(
                row![
                    ui::eyebrow("台词"),
                    text(dialogue.clone())
                        .size(13)
                        .color(ui::TEXT)
                        .line_height(1.55),
                ]
                .spacing(8),
            );
        }
        let negative_block: Element<'_, Message> = if negative.is_empty() {
            space::vertical().height(0).into()
        } else {
            text(format!("排除 · {}", short(&negative, 120)))
                .size(11)
                .color(ui::TEXT_MUTED)
                .line_height(1.5)
                .into()
        };
        let prompt_block = column![
            row![
                ui::eyebrow("画面提示词"),
                space::horizontal(),
                button("复制")
                    .style(ui::ghost)
                    .padding([3, 8])
                    .on_press_maybe(
                        (!prompt.is_empty()).then_some(Message::CopyPrompt(prompt.clone()))
                    ),
            ]
            .align_y(iced::Center),
            text(if prompt.is_empty() {
                "尚未编译提示词".into()
            } else {
                short(&prompt, 240)
            })
            .size(12)
            .color(if prompt.is_empty() {
                ui::CORAL
            } else {
                ui::TEXT_SECONDARY
            })
            .line_height(1.6),
            negative_block,
        ]
        .spacing(5)
        .width(390);
        container(
            row![
                column![
                    text(format!("{:02}", index + 1))
                        .size(24)
                        .font(ui::font(iced::font::Weight::Semibold))
                        .color(if index == 0 { ui::GOLD } else { ui::TEXT_MUTED }),
                    ui::eyebrow("镜"),
                ]
                .spacing(0)
                .align_x(iced::Center)
                .width(52),
                center,
                prompt_block,
            ]
            .spacing(16)
            .align_y(iced::Alignment::Start),
        )
        .padding(16)
        .width(Fill)
        .style(ui::panel)
        .into()
    }
    fn assets_view(&self) -> Element<'_, Message> {
        let selected = self
            .selected_item
            .and_then(|selected| self.items.iter().find(|item| id(item) == selected).cloned());
        let frozen = self.production.assets_frozen;
        let mut actions: Vec<Element<'_, Message>> = vec![
            button("导入素材")
                .style(ui::secondary)
                .padding([8, 14])
                .on_press_maybe(self.selected_item.is_some().then_some(Message::ImportMedia))
                .into(),
            button(if frozen {
                "撤销冻结"
            } else {
                "冻结资产"
            })
            .style(if frozen { ui::secondary } else { ui::primary })
            .padding([8, 15])
            .on_press_maybe(
                (self.has_assets() || frozen).then_some(Message::ToggleGate(Gate::Assets)),
            )
            .into(),
        ];
        if let Some(selected) = self.selected_item {
            actions.push(
                button("删除条目")
                    .style(ui::danger)
                    .padding([8, 14])
                    .on_press(Message::DeleteItem(selected))
                    .into(),
            );
        }
        let subtitle = format!(
            "共 {} 个资产 · {} 个已有视觉设定 · {}",
            self.items.len(),
            self.visual_dna_ids.len(),
            if frozen {
                "已冻结，分镜会读取当前版本"
            } else {
                "未冻结，分镜不会读取变更中的资产"
            }
        );
        column![
            self.page_heading("角色、场景与道具", subtitle, actions),
            row![
                self.asset_filters(),
                self.asset_grid(),
                self.asset_detail(selected)
            ]
            .spacing(12)
            .height(Fill),
        ]
        .spacing(16)
        .height(Fill)
        .into()
    }

    fn asset_filters(&self) -> Element<'_, Message> {
        let mut filters = column![
            row![
                ui::heading("资产分类"),
                space::horizontal(),
                ui::muted(format!("{}", self.items.len())),
            ]
            .align_y(iced::Center),
        ]
        .spacing(6);
        let all = self.category_count("all");
        filters = filters.push(self.category_button("all", "全部资产", all));
        for category in ASSET_CATEGORIES {
            let count = self.category_count(category.0);
            filters = filters.push(self.category_button(category.0, category.1, count));
        }
        let new_item = container(
            column![
                ui::eyebrow("新建资产"),
                text_input("名称，例如：林夏", &self.new_name)
                    .on_input(Message::NewName)
                    .style(ui::field)
                    .padding([8, 10]),
                pick_list(
                    ASSET_CATEGORIES,
                    ASSET_CATEGORIES
                        .iter()
                        .find(|category| category.0 == self.item_category)
                        .copied(),
                    |category| Message::ItemField("category", category.0.into())
                )
                .style(ui::pick)
                .width(Fill),
                button("创建条目")
                    .style(ui::secondary)
                    .width(Fill)
                    .on_press_maybe(
                        (!self.new_name.trim().is_empty()).then_some(Message::CreateItem)
                    ),
            ]
            .spacing(7),
        )
        .padding(11)
        .width(Fill)
        .style(ui::card);
        container(
            scrollable(column![filters, new_item].spacing(12))
                .height(Fill)
                .style(ui::scrollable),
        )
        .padding(12)
        .width(212)
        .height(Fill)
        .style(ui::panel)
        .into()
    }

    fn category_count(&self, code: &str) -> usize {
        if code == "all" {
            self.items.len()
        } else {
            self.items
                .iter()
                .filter(|item| label(item, "category") == code)
                .count()
        }
    }

    fn category_button(
        &self,
        code: &'static str,
        label: &'static str,
        count: usize,
    ) -> Element<'_, Message> {
        let selected = self.category == code;
        button(
            row![
                text(label).size(12).color(if selected {
                    ui::GOLD_BRIGHT
                } else {
                    ui::TEXT_SECONDARY
                }),
                space::horizontal(),
                text(count.to_string()).size(11).color(if selected {
                    ui::GOLD
                } else {
                    ui::TEXT_MUTED
                }),
            ]
            .align_y(iced::Center),
        )
        .width(Fill)
        .padding([5, 8])
        .style(ui::nav(selected))
        .on_press(Message::Category(code.into()))
        .into()
    }

    fn asset_grid(&self) -> Element<'_, Message> {
        let filtered = self
            .items
            .iter()
            .filter(|item| self.category == "all" || label(item, "category") == self.category)
            .cloned()
            .collect::<Vec<_>>();
        if filtered.is_empty() {
            return container(
                column![
                    ui::eyebrow("ASSETS · 空白"),
                    ui::display(if self.items.is_empty() {
                        "还没有资产。"
                    } else {
                        "这个分类还没有资产。"
                    }),
                    ui::muted(if self.current_script().trim().is_empty() {
                        "先在剧本工坊完成本集剧本，再从剧本提取角色、场景与道具。"
                    } else {
                        "可以从本集剧本提取，也可以手动新建一个条目。"
                    }),
                    button("从本集剧本提取").style(ui::primary).on_press_maybe(
                        self.busy
                            .is_none()
                            .then_some(Message::Run("episode_assets_visual_dna".into()))
                    ),
                ]
                .spacing(12)
                .align_x(iced::Center),
            )
            .padding([56, 32])
            .center_x(Fill)
            .style(ui::panel)
            .into();
        }
        let mut grid = grid::Grid::new().fluid(246.0).height(168.0).spacing(12);
        for item in filtered {
            grid = grid.push(self.asset_card(&item));
        }
        scrollable(grid).height(Fill).style(ui::scrollable).into()
    }

    fn asset_card(&self, item: &Value) -> Element<'_, Message> {
        let item_id = id(item);
        let selected = self.selected_item == Some(item_id);
        let dna = self.visual_dna_ids.contains(&item_id);
        let description = label(item, "description");
        button(
            column![
                row![
                    ui::eyebrow(category_label(label(item, "category"))),
                    space::horizontal(),
                    ui::chip(
                        if dna { "VD" } else { "VD —" },
                        if dna { ui::Tone::Mint } else { ui::Tone::Muted },
                    ),
                ]
                .align_y(iced::Center),
                text(short(label(item, "name"), 18))
                    .size(16)
                    .font(ui::font(iced::font::Weight::Semibold))
                    .color(ui::TEXT),
                text(if description.is_empty() {
                    "暂无描述".into()
                } else {
                    short(description, 54)
                })
                .size(12)
                .color(ui::TEXT_SECONDARY)
                .line_height(1.55),
                row![
                    ui::muted(if selected {
                        "正在查看"
                    } else {
                        "点击查看"
                    })
                    .size(11),
                    space::horizontal(),
                    text("→")
                        .size(13)
                        .color(if selected { ui::GOLD } else { ui::TEXT_MUTED }),
                ]
                .align_y(iced::Center),
            ]
            .spacing(6),
        )
        .padding(14)
        .width(Fill)
        .height(Fill)
        .style(move |_: &Theme, status| {
            let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
            button::Style {
                background: Some(
                    if selected || hovered {
                        ui::SURFACE_HIGH
                    } else {
                        ui::SURFACE
                    }
                    .into(),
                ),
                text_color: ui::TEXT,
                border: iced::Border {
                    color: if selected {
                        ui::GOLD_DIM
                    } else if hovered {
                        ui::LINE
                    } else {
                        ui::LINE_SOFT
                    },
                    width: 1.0,
                    radius: 12.0.into(),
                },
                ..button::Style::default()
            }
        })
        .on_press(Message::SelectItem(item_id))
        .into()
    }

    fn asset_detail(&self, selected: Option<Value>) -> Element<'_, Message> {
        let Some(item) = selected else {
            return container(
                column![
                    ui::eyebrow("详情"),
                    ui::heading("选择一个资产"),
                    ui::muted(
                        "查看描述、基础 Prompt、素材与视觉设定。冻结前请确认角色与场景都已齐全。"
                    ),
                ]
                .spacing(9),
            )
            .padding(18)
            .width(360)
            .height(Fill)
            .style(ui::panel)
            .into();
        };
        let selected_id = id(&item);
        let dna = self.visual_dna_ids.contains(&selected_id);
        let mut detail = column![
            row![
                ui::eyebrow("详情"),
                space::horizontal(),
                ui::chip(
                    if dna {
                        "视觉设定已生成"
                    } else {
                        "无视觉设定"
                    },
                    if dna { ui::Tone::Mint } else { ui::Tone::Gold },
                ),
            ]
            .align_y(iced::Center),
            text_input("资产名称", &self.item_name)
                .on_input(|value| Message::ItemField("name", value))
                .style(ui::field)
                .padding([8, 10]),
            pick_list(
                ASSET_CATEGORIES,
                ASSET_CATEGORIES
                    .iter()
                    .find(|category| category.0 == self.item_category)
                    .copied(),
                |category| Message::ItemField("category", category.0.into())
            )
            .style(ui::pick)
            .width(Fill),
            ui::eyebrow("描述"),
            dark_editor(
                &self.item_description,
                Self::field_editor_height(&self.item_description.text(), 86.0, 52.0),
                Message::ItemDescription,
            ),
            ui::eyebrow("基础 Prompt"),
            dark_editor(
                &self.item_prompt,
                Self::field_editor_height(&self.item_prompt.text(), 86.0, 52.0),
                Message::ItemPrompt,
            ),
            button("保存条目")
                .style(ui::primary)
                .width(Fill)
                .on_press(Message::SaveItem),
            row![
                ui::heading("素材"),
                space::horizontal(),
                button("导入")
                    .style(ui::secondary)
                    .on_press(Message::ImportMedia),
            ]
            .align_y(iced::Center),
        ]
        .spacing(9);
        if self.media.is_empty() {
            detail = detail.push(ui::muted("还没有素材。导入参考图后，可据此生成视觉设定。"));
        }
        for asset in &self.media {
            let asset_id = id(asset);
            let path = match self.store.asset_path(label(asset, "file_path")) {
                Ok(path) => path,
                Err(_) => {
                    detail = detail.push(ui::muted("此素材路径不可用，请检查原始工作区。"));
                    continue;
                }
            };
            let mut card = column![
                row![
                    text(short(label(asset, "file_path"), 22))
                        .size(11)
                        .color(ui::TEXT_SECONDARY),
                    space::horizontal(),
                    button("打开")
                        .style(ui::ghost)
                        .padding([2, 4])
                        .on_press(Message::OpenMedia(path.clone())),
                    button("删除")
                        .style(ui::ghost)
                        .padding([2, 4])
                        .on_press(Message::DeleteAsset(asset_id)),
                ]
                .align_y(iced::Center)
            ]
            .spacing(4);
            if label(asset, "file_type") == "image" {
                card = card
                    .push(
                        image(path)
                            .height(116)
                            .content_fit(iced::ContentFit::Contain),
                    )
                    .push(
                        button("用这张图生成视觉设定")
                            .style(ui::secondary)
                            .width(Fill)
                            .on_press(Message::VisualAsset(asset_id)),
                    );
            }
            detail = detail.push(container(card).padding(8).width(Fill).style(ui::card));
        }
        container(ui::panel_surface(
            container(scrollable(detail).height(Fill).style(ui::scrollable)).padding(14),
        ))
        .width(360)
        .height(Fill)
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
        let outline_selected = self.context_kind == "project_outline";
        let approved = self.production.outline_approved;
        let mut actions: Vec<Element<'_, Message>> = vec![
            button("保存上下文")
                .style(ui::secondary)
                .padding([8, 14])
                .on_press(Message::SaveContext)
                .into(),
        ];
        if outline_selected && self.has_outline() {
            actions.push(
                button(if approved {
                    "撤销确认"
                } else {
                    "确认大纲"
                })
                .style(if approved { ui::secondary } else { ui::primary })
                .padding([8, 15])
                .on_press(Message::ToggleGate(Gate::Outline))
                .into(),
            );
        }
        let tabs = row![
            button("故事设定")
                .style(ui::nav(self.context_kind == "series_bible"))
                .padding([8, 12])
                .on_press(Message::ContextKind("series_bible".into())),
            button("项目大纲")
                .style(ui::nav(self.context_kind == "project_outline"))
                .padding([8, 12])
                .on_press(Message::ContextKind("project_outline".into())),
            pick_list(items, None::<Choice>, Message::ContextVisual)
                .placeholder("资产视觉设定")
                .style(ui::pick)
                .width(200),
            space::horizontal(),
            ui::muted(if outline_selected {
                "项目大纲是分集与分镜的上游事实。"
            } else {
                "故事设定会作为所有生成的创作上下文。"
            }),
        ]
        .spacing(8)
        .align_y(iced::Center);
        let document: Element<'_, Message> = if self.context_document_ready {
            scrollable(container(self.context_form.view().map(Message::ContextDocument)).padding(4))
                .height(Fill)
                .style(ui::scrollable)
                .into()
        } else {
            container(
                column![
                    ui::eyebrow("BIBLE · 未载入"),
                    ui::heading("选择项目和上下文"),
                    ui::muted("故事设定、项目大纲或资产视觉设定会显示在这里。"),
                ]
                .spacing(8),
            )
            .padding(24)
            .height(Fill)
            .into()
        };
        let summary_height = Self::field_editor_height(&self.context_summary.text(), 112.0, 52.0);
        let tools = container(ui::panel_surface(
            scrollable(
                column![
                    ui::heading("大纲工具"),
                    ui::muted("先生成项目大纲，再逐集拆分；优化只改大纲，不改已写剧本。"),
                    ui::eyebrow("故事概述"),
                    ui::muted("故事背景、主线冲突与结局方向").size(11),
                    dark_editor(
                        &self.context_summary,
                        summary_height,
                        Message::ContextSummary
                    ),
                    row![
                        column![
                            ui::eyebrow("预计集数"),
                            text_input("12", &self.context_episodes)
                                .on_input(|value| Message::ContextField("episodes", value))
                                .style(ui::field)
                                .padding([8, 10])
                                .width(88),
                        ]
                        .spacing(5),
                        column![
                            ui::eyebrow("动作"),
                            button("生成项目大纲").style(ui::primary).on_press_maybe(
                                self.busy
                                    .is_none()
                                    .then_some(Message::Run("project_outline_generate".into()))
                            ),
                        ]
                        .spacing(5),
                    ]
                    .spacing(10)
                    .align_y(iced::Bottom),
                    ui::divider(),
                    ui::eyebrow("优化指令"),
                    row![
                        text_input("例如：加强集尾钩子，压缩前两集…", &self.context_instruction)
                            .on_input(|value| Message::ContextField("instruction", value))
                            .style(ui::field)
                            .padding([8, 10]),
                        button("优化").style(ui::secondary).on_press_maybe(
                            self.busy
                                .is_none()
                                .then_some(Message::Run("project_outline_optimize".into()))
                        ),
                    ]
                    .spacing(8),
                    ui::muted("生成与优化都会先保存为运行记录；可在生成记录中查看阶段。"),
                ]
                .spacing(10)
                .padding(16),
            )
            .height(Fill)
            .style(ui::scrollable),
        ))
        .width(352)
        .height(Fill);
        column![
            self.page_heading(
                "故事基线",
                format!(
                    "{} · {}",
                    if self.has_series_bible() {
                        "故事设定已存在"
                    } else {
                        "故事设定未建立"
                    },
                    if self.has_outline() {
                        "全剧大纲已生成"
                    } else {
                        "全剧大纲未生成"
                    }
                ),
                actions,
            ),
            tabs,
            row![
                container(ui::panel_surface(document))
                    .width(Fill)
                    .height(Fill),
                tools
            ]
            .spacing(12)
            .height(Fill),
        ]
        .spacing(16)
        .height(Fill)
        .into()
    }

    fn runs_view(&self) -> Element<'_, Message> {
        let mut actions: Vec<Element<'_, Message>> = vec![
            button("刷新")
                .style(ui::secondary)
                .padding([8, 14])
                .on_press(Message::Refresh)
                .into(),
        ];
        if self
            .commit_key()
            .is_some_and(|key| self.pending_commits.contains_key(&key))
        {
            actions.push(
                button("重试写入")
                    .style(ui::danger)
                    .padding([8, 14])
                    .on_press(Message::ApplyArtifacts)
                    .into(),
            );
        }
        let mut runs = column![
            row![
                ui::heading("运行"),
                space::horizontal(),
                ui::muted(format!("{}", self.run_list.len())),
            ]
            .align_y(iced::Center)
        ]
        .spacing(6);
        if self.run_list.is_empty() {
            runs = runs.push(ui::muted("还没有运行记录。生成动作会创建 Run。"));
        }
        for run in &self.run_list {
            let run_id = label(run, "run_id");
            let selected = self.selected_run == run_id;
            let action = {
                let value = label(run, "final_action_key");
                if value.is_empty() {
                    label(run, "action")
                } else {
                    value
                }
            };
            let status = label(run, "status");
            runs = runs.push(
                button(
                    column![
                        row![
                            text(action_label(if action.is_empty() {
                                "运行"
                            } else {
                                action
                            }))
                            .size(12)
                            .color(if selected {
                                ui::GOLD_BRIGHT
                            } else {
                                ui::TEXT_SECONDARY
                            }),
                            space::horizontal(),
                            ui::chip(run_status_label(status), run_status_tone(status)),
                        ]
                        .align_y(iced::Center),
                        ui::muted(format!(
                            "{} · {}",
                            short(run_id, 10),
                            relative_time(run["created_at_ms"].as_u64().unwrap_or(0))
                        ))
                        .size(10),
                    ]
                    .spacing(4),
                )
                .width(Fill)
                .padding([8, 9])
                .style(ui::nav(selected))
                .on_press(Message::SelectRun(run_id.into())),
            );
        }
        let mut stages = column![
            ui::eyebrow("阶段"),
            if self.selected_run.is_empty() {
                ui::muted("选择左侧一次运行")
            } else {
                ui::muted(short(&self.selected_run, 14))
            },
        ]
        .spacing(6);
        for stage in &self.stages {
            stages = stages.push(
                button(text(stage_label(stage)).size(11))
                    .width(Fill)
                    .padding([6, 8])
                    .style(ui::nav(self.selected_stage == *stage))
                    .on_press(Message::SelectStage(stage.clone())),
            );
        }
        let detail = container(ui::panel_surface(
            container(
                scrollable(
                    text(self.editor.text())
                        .size(14)
                        .color(ui::TEXT_SECONDARY)
                        .line_height(1.7)
                        .width(Fill),
                )
                .height(Fill)
                .style(ui::scrollable),
            )
            .padding(12),
        ))
        .width(Fill)
        .height(Fill);
        column![
            self.page_heading(
                "生成记录",
                "每次生成都有 Run、阶段和产物；失败与暂停不会覆盖旧结果。",
                actions,
            ),
            row![
                container(scrollable(runs).height(Fill).style(ui::scrollable))
                    .padding(12)
                    .width(300)
                    .height(Fill)
                    .style(ui::panel),
                container(scrollable(stages).height(Fill).style(ui::scrollable))
                    .padding(12)
                    .width(180)
                    .height(Fill)
                    .style(ui::panel),
                detail,
            ]
            .spacing(12)
            .height(Fill),
        ]
        .spacing(16)
        .height(Fill)
        .into()
    }

    fn memory_view(&self) -> Element<'_, Message> {
        let actions = vec![
            button("刷新")
                .style(ui::secondary)
                .padding([8, 14])
                .on_press(Message::Refresh)
                .into(),
            button("重建语义索引")
                .style(ui::secondary)
                .padding([8, 14])
                .on_press(Message::RebuildMemory)
                .into(),
        ];
        let mut records = column![
            row![
                ui::chip(
                    format!("{} 条事实", self.memory_records.len()),
                    ui::Tone::Indigo
                ),
                ui::chip(
                    format!("{} 个变更集", self.changesets.len()),
                    ui::Tone::Gold
                ),
                ui::chip(format!("{} 个冲突", self.conflicts.len()), ui::Tone::Coral),
                space::horizontal(),
                ui::muted("只读查看；事实变更需要审核。"),
            ]
            .spacing(8)
            .align_y(iced::Center),
            row![
                text_input("搜索事实原文", &self.memory_search)
                    .on_input(Message::MemorySearch)
                    .style(ui::field)
                    .padding([8, 10]),
                button("词面检索")
                    .style(ui::secondary)
                    .on_press(Message::Refresh),
                button("语义检索")
                    .style(ui::secondary)
                    .on_press(Message::SemanticSearch),
            ]
            .spacing(8),
        ]
        .spacing(12);
        for changeset in &self.changesets {
            let change_id = label(changeset, "changeset_id");
            records = records.push(
                container(
                    column![
                        row![
                            ui::heading("待审核变更集"),
                            space::horizontal(),
                            ui::chip(label(changeset, "review_status"), ui::Tone::Gold),
                        ]
                        .align_y(iced::Center),
                        text(readable(changeset))
                            .size(13)
                            .color(ui::TEXT_SECONDARY)
                            .line_height(1.6),
                        row![
                            button("批准")
                                .style(ui::primary)
                                .on_press(Message::ReviewChangeset(change_id.into(), true)),
                            button("拒绝")
                                .style(ui::secondary)
                                .on_press(Message::ReviewChangeset(change_id.into(), false)),
                        ]
                        .spacing(8),
                    ]
                    .spacing(9),
                )
                .padding(16)
                .width(Fill)
                .style(ui::panel),
            );
        }
        for conflict in &self.conflicts {
            let conflict_id = label(conflict, "conflict_id");
            records = records.push(
                container(
                    column![
                        row![
                            ui::heading("待裁决冲突"),
                            space::horizontal(),
                            ui::chip("需要人", ui::Tone::Coral),
                        ]
                        .align_y(iced::Center),
                        text(readable(conflict))
                            .size(13)
                            .color(ui::TEXT_SECONDARY)
                            .line_height(1.6),
                        button("标记已裁决")
                            .style(ui::secondary)
                            .on_press(Message::ResolveConflict(conflict_id.into())),
                    ]
                    .spacing(9),
                )
                .padding(16)
                .width(Fill)
                .style(ui::panel),
            );
        }
        for record in &self.memory_records {
            records = records.push(
                container(
                    text(readable(record))
                        .size(13)
                        .color(ui::TEXT_SECONDARY)
                        .line_height(1.6),
                )
                .padding(14)
                .width(Fill)
                .style(ui::card),
            );
        }
        if self.memory_records.is_empty() && self.changesets.is_empty() && self.conflicts.is_empty()
        {
            records = records.push(
                container(
                    column![
                        ui::heading("还没有记忆事实"),
                        ui::muted("运行记忆抽取或审核变更后，事实、证据与冲突会显示在这里。"),
                    ]
                    .spacing(6),
                )
                .padding(24)
                .width(Fill)
                .style(ui::panel),
            );
        }
        column![
            self.page_heading(
                "项目记忆",
                "证据、事实与审核记录；语义索引可重建，原文不会因重建丢失。",
                actions,
            ),
            scrollable(records).height(Fill).style(ui::scrollable),
        ]
        .spacing(16)
        .height(Fill)
        .into()
    }
    fn settings_view(&self) -> Element<'_, Message> {
        let field = |name: &'static str, value: &str, key: &'static str| {
            column![
                ui::eyebrow(name),
                text_input(name, value)
                    .on_input(move |value| Message::Setting(key, value.into()))
                    .style(ui::field)
                    .padding([8, 10]),
            ]
            .spacing(5)
        };
        let provider_hint = match self.settings.provider {
            AiProvider::OpenAi => "OpenAI 兼容接口。填写服务地址后，从模型列表选择聊天模型。",
            AiProvider::Google => "Google 原生 generateContent 接口。模型必须支持内容生成。",
        };
        let mut model_picker = column![
            row![
                ui::eyebrow("服务提供的模型"),
                space::horizontal(),
                button(if self.model_catalog_loading {
                    "正在获取…"
                } else if self.model_catalog_loaded {
                    "刷新"
                } else {
                    "获取模型列表"
                })
                .style(ui::secondary)
                .padding([5, 10])
                .on_press_maybe((!self.model_catalog_loading).then_some(Message::FetchModels)),
            ]
            .align_y(iced::Center),
            ui::muted(&self.model_catalog_status).size(11),
        ]
        .spacing(6);
        if !self.model_catalog.options().is_empty() {
            let selected = self
                .model_catalog
                .options()
                .iter()
                .find(|model| model.supported && model.info.id == self.settings.model);
            model_picker = model_picker.push(
                combo_box(
                    &self.model_catalog,
                    "搜索模型名称或 ID",
                    selected,
                    |model| Message::SelectModel(model.info.id),
                )
                .width(Fill)
                .menu_height(280),
            );
            if selected.is_none() {
                model_picker = model_picker.push(ui::muted(
                    "当前模型未在列表中；已保留原设置，可从列表选择其他模型。",
                ));
            }
        }
        let ai = container(
            column![
                column![
                    ui::eyebrow("模型服务"),
                    pick_list(
                        AiProvider::ALL,
                        Some(self.settings.provider),
                        Message::Provider
                    )
                    .style(ui::pick)
                    .width(Fill),
                    ui::muted(provider_hint).size(11),
                ]
                .spacing(5),
                field("API Key", &self.settings.api_key, "api_key"),
                field("服务地址", &self.settings.base_url, "base_url"),
                model_picker,
                row![
                    ui::chip(
                        format!("当前模型 · {}", self.settings.model),
                        ui::Tone::Indigo
                    ),
                    space::horizontal(),
                    ui::muted("模型可见不代表账号有调用权限。"),
                ]
                .align_y(iced::Center),
                ui::divider(),
                ui::eyebrow("生成参数"),
                row![
                    field("温度", &self.settings_temperature, "temperature"),
                    field("最大输出 Tokens", &self.settings_tokens, "tokens"),
                    field("超时（秒）", &self.settings_timeout, "timeout"),
                ]
                .spacing(10),
                row![
                    button("保存设置")
                        .style(ui::primary)
                        .padding([9, 16])
                        .on_press(Message::SaveSettings),
                    button("测试连接")
                        .style(ui::secondary)
                        .padding([9, 16])
                        .on_press(Message::TestAi),
                    ui::muted("测试只发送简短文字，不发送项目内容。"),
                ]
                .spacing(10)
                .align_y(iced::Center),
            ]
            .spacing(14),
        )
        .padding(20)
        .width(Fill)
        .height(Fill)
        .style(ui::panel);
        let mut memory = column![
            ui::heading("记忆检索"),
            ui::muted("默认只做事实与词面检索；启用语义检索需要独立配置。"),
            row![
                button("仅事实检索")
                    .style(ui::nav(self.embedding.mode == "disabled"))
                    .padding([6, 10])
                    .on_press(Message::Embedding("mode", "disabled".into())),
                button("本地 BGE-M3")
                    .style(ui::nav(self.embedding.mode == "local"))
                    .padding([6, 10])
                    .on_press(Message::Embedding("mode", "local".into())),
                button("兼容 API")
                    .style(ui::nav(self.embedding.mode == "http"))
                    .padding([6, 10])
                    .on_press(Message::Embedding("mode", "http".into())),
            ]
            .spacing(6),
        ]
        .spacing(9);
        if self.embedding.mode == "local" {
            memory = memory
                .push(ui::muted(
                    "选择含 config.json、tokenizer.json、model.safetensors 的目录",
                ))
                .push(
                    text_input("模型目录", &self.embedding.model_dir)
                        .on_input(|value| Message::Embedding("dir", value))
                        .style(ui::field)
                        .padding([8, 10]),
                )
                .push(
                    text_input("最大输入长度", &self.embedding_max)
                        .on_input(|value| Message::Embedding("max", value))
                        .style(ui::field)
                        .padding([8, 10]),
                );
        }
        if self.embedding.mode == "http" {
            memory = memory
                .push(
                    text_input("Embedding 服务地址", &self.embedding.base_url)
                        .on_input(|value| Message::Embedding("url", value))
                        .style(ui::field)
                        .padding([8, 10]),
                )
                .push(
                    text_input("Embedding 模型", &self.embedding.model)
                        .on_input(|value| Message::Embedding("model", value))
                        .style(ui::field)
                        .padding([8, 10]),
                )
                .push(
                    text_input("Embedding API Key", &self.embedding.api_key)
                        .secure(true)
                        .on_input(|value| Message::Embedding("key", value))
                        .style(ui::field)
                        .padding([8, 10]),
                );
        }
        memory = memory.push(
            button("保存记忆检索设置")
                .style(ui::secondary)
                .on_press(Message::SaveEmbedding),
        );
        let security = container(
            column![
                ui::eyebrow("本地边界"),
                ui::muted("密钥只写入本机设置文件；不写日志、不进入测试输出。"),
                ui::muted("模型目录、素材与数据库都由本地路径管理；没有云同步。"),
            ]
            .spacing(6),
        )
        .padding(16)
        .width(Fill)
        .style(ui::card);
        column![
            self.page_heading(
                "AI 服务",
                "模型连接、生成参数与记忆检索；所有请求由 Rust 直接发出。",
                vec![],
            ),
            row![
                ai,
                column![
                    container(memory).padding(18).width(Fill).style(ui::panel),
                    security
                ]
                .spacing(12)
                .width(380),
            ]
            .spacing(12)
            .height(Fill),
        ]
        .spacing(16)
        .height(Fill)
        .into()
    }

    fn prompts_view(&self) -> Element<'_, Message> {
        let mut list = column![
            row![
                ui::heading("模板"),
                space::horizontal(),
                ui::muted(format!("{}", self.templates.len())),
            ]
            .align_y(iced::Center),
            text_input("搜索模板", &self.prompt_search)
                .on_input(Message::PromptSearch)
                .style(ui::field)
                .padding([8, 10]),
        ]
        .spacing(8);
        for template in self.templates.iter().filter(|template| {
            format!("{} {}", label(template, "key"), label(template, "title"))
                .to_lowercase()
                .contains(&self.prompt_search.to_lowercase())
        }) {
            let key = label(template, "key");
            list = list.push(
                button(
                    column![
                        text(label(template, "title"))
                            .size(12)
                            .color(if self.prompt_key == key {
                                ui::GOLD_BRIGHT
                            } else {
                                ui::TEXT_SECONDARY
                            }),
                        ui::muted(key).size(10),
                    ]
                    .spacing(3),
                )
                .width(Fill)
                .padding([7, 9])
                .style(ui::nav(self.prompt_key == key))
                .on_press(Message::SelectPrompt(key.into())),
            );
        }
        let prompt_height = Self::field_editor_height(&self.editor.text(), 320.0, 68.0);
        let editor = container(ui::panel_surface(
            column![
                row![
                    column![
                        ui::eyebrow("模板 Key"),
                        text_input("key", &self.prompt_key)
                            .on_input(|value| Message::PromptField("key", value))
                            .style(ui::field)
                            .padding([8, 10]),
                    ]
                    .spacing(5)
                    .width(Fill),
                    column![
                        ui::eyebrow("名称"),
                        text_input("名称", &self.prompt_title)
                            .on_input(|value| Message::PromptField("title", value))
                            .style(ui::field)
                            .padding([8, 10]),
                    ]
                    .spacing(5)
                    .width(Fill),
                    column![
                        ui::eyebrow("分类"),
                        text_input("custom", &self.prompt_category)
                            .on_input(|value| Message::PromptField("category", value))
                            .style(ui::field)
                            .padding([8, 10]),
                    ]
                    .spacing(5)
                    .width(160),
                ]
                .spacing(10),
                scrollable(dark_editor(&self.editor, prompt_height, Message::Edit))
                    .id("prompt-editor")
                    .height(Fill)
                    .style(ui::scrollable),
                row![
                    button("保存模板")
                        .style(ui::primary)
                        .on_press(Message::SavePrompt),
                    button("恢复内置")
                        .style(ui::secondary)
                        .on_press(Message::ResetPrompt),
                    button("删除自定义")
                        .style(ui::danger)
                        .on_press(Message::DeletePrompt),
                    space::horizontal(),
                    ui::muted("模板改动只影响之后的新 Run。"),
                ]
                .spacing(10)
                .align_y(iced::Center),
            ]
            .spacing(12)
            .padding(16),
        ))
        .width(Fill)
        .height(Fill);
        column![
            self.page_heading(
                "提示词",
                "内置模板可恢复；自定义模板用于覆盖工作流行为。",
                vec![],
            ),
            row![
                container(scrollable(list).height(Fill).style(ui::scrollable))
                    .padding(12)
                    .width(300)
                    .height(Fill)
                    .style(ui::panel),
                editor,
            ]
            .spacing(12)
            .height(Fill),
        ]
        .spacing(16)
        .height(Fill)
        .into()
    }

    fn events_view(&self) -> Element<'_, Message> {
        let mut events = column![
            row![
                ui::heading("事件"),
                space::horizontal(),
                ui::muted(format!("{}", self.events.len())),
            ]
            .align_y(iced::Center),
            text_input("搜索事件", &self.event_search)
                .on_input(Message::EventFilter)
                .style(ui::field)
                .padding([8, 10]),
            pick_list(
                ["名称", "关联数量"],
                Some(self.event_sort.as_str()),
                |sort| Message::EventSort(sort.into())
            )
            .style(ui::pick)
            .width(Fill),
        ]
        .spacing(7);
        for event in self.events.iter().filter(|event| {
            label(event, "name").contains(&self.event_search)
                || label(event, "description").contains(&self.event_search)
        }) {
            let selected = self.selected_event == Some(id(event));
            events = events.push(
                row![
                    button(if self.event_focus.contains(&id(event)) {
                        "●"
                    } else {
                        "○"
                    })
                    .style(ui::ghost)
                    .padding([2, 3])
                    .on_press(Message::EventFocus(id(event))),
                    button(text(label(event, "name")).size(12).color(if selected {
                        ui::GOLD_BRIGHT
                    } else {
                        ui::TEXT_SECONDARY
                    }))
                    .width(Fill)
                    .padding([6, 7])
                    .style(ui::nav(selected))
                    .on_press(Message::SelectEvent(id(event))),
                    button("×")
                        .style(ui::ghost)
                        .padding([2, 4])
                        .on_press(Message::DeleteEvent(id(event))),
                ]
                .spacing(2)
                .align_y(iced::Center),
            );
        }
        events = events
            .push(ui::divider())
            .push(
                text_input("新事件名称", &self.new_name)
                    .on_input(Message::NewName)
                    .style(ui::field)
                    .padding([8, 10]),
            )
            .push(
                button("新增事件")
                    .style(ui::secondary)
                    .width(Fill)
                    .on_press_maybe(
                        (!self.new_name.trim().is_empty()).then_some(Message::CreateEvent),
                    ),
            );
        let mut detail = column![
            row![
                ui::heading("事件与场景"),
                space::horizontal(),
                button("保存关联")
                    .style(ui::secondary)
                    .on_press(Message::SaveEvent),
            ]
            .align_y(iced::Center),
            ui::muted("选择事件后，将它关联到相应场景。"),
        ]
        .spacing(9);
        if self.selected_event.is_some() {
            detail = detail
                .push(
                    row![
                        text_input("事件名称", &self.event_name)
                            .on_input(|value| Message::EventField("name", value))
                            .style(ui::field)
                            .padding([8, 10]),
                        text_input("颜色 #RRGGBB", &self.event_color)
                            .on_input(|value| Message::EventField("color", value))
                            .style(ui::field)
                            .padding([8, 10])
                            .width(150),
                        button("保存事件")
                            .style(ui::secondary)
                            .on_press(Message::SaveEventMetadata),
                    ]
                    .spacing(8),
                )
                .push(
                    text_input("事件描述", &self.event_description)
                        .on_input(|value| Message::EventField("description", value))
                        .style(ui::field)
                        .padding([8, 10]),
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
                .style(ui::nav(checked))
                .width(Fill)
                .on_press(Message::ToggleScene(id(scene))),
            );
        }
        column![
            self.page_heading(
                "故事线",
                "事件与场景的关系网；矩阵用于检查跨集节奏与关联。",
                vec![],
            ),
            row![
                container(scrollable(events).height(Fill).style(ui::scrollable))
                    .padding(12)
                    .width(300)
                    .height(Fill)
                    .style(ui::panel),
                column![
                    container(
                        event_matrix::matrix(
                            &self.matrix_events,
                            &self.episodes,
                            &self.scenes,
                            self.selected_event,
                            &self.matrix_view
                        )
                        .map(Message::Matrix)
                    )
                    .padding(8)
                    .width(Fill)
                    .height(Fill)
                    .style(ui::panel),
                    container(scrollable(detail).height(Fill).style(ui::scrollable))
                        .padding(14)
                        .width(Fill)
                        .height(260)
                        .style(ui::panel),
                ]
                .spacing(12)
                .height(Fill),
            ]
            .spacing(12)
            .height(Fill),
        ]
        .spacing(16)
        .height(Fill)
        .into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn model_settings_app() -> (tempfile::TempDir, App) {
        let root = tempfile::tempdir().unwrap();
        let store = Store::open(root.path()).unwrap();
        let app = App::new(
            store,
            Options {
                root: root.path().into(),
                ..Options::default()
            },
        );
        (root, app)
    }

    fn catalog_model(id: &str, methods: &[&str]) -> ModelInfo {
        ModelInfo {
            id: id.into(),
            display_name: format!("显示名称 {id}"),
            supported_generation_methods: methods.iter().map(|method| (*method).into()).collect(),
        }
    }

    #[test]
    fn model_catalog_fetch_is_explicit_and_duplicate_requests_are_ignored() {
        let (_root, mut app) = model_settings_app();
        assert_eq!(app.model_catalog_request, 0);
        assert!(!app.model_catalog_loading);
        assert!(!app.model_catalog_loaded);
        assert!(app.model_catalog.options().is_empty());
        let _ = app.update(Message::FetchModels);
        let request = app.model_catalog_request;
        assert!(app.model_catalog_loading);
        let _ = app.update(Message::FetchModels);
        assert_eq!(app.model_catalog_request, request);
    }

    #[test]
    fn model_catalog_selection_is_saved_without_auto_selecting_or_replacing_aliases() {
        let (root, mut app) = model_settings_app();
        app.settings.model = "custom-alias".into();
        let _ = app.update(Message::FetchModels);
        let request = app.model_catalog_request;
        let _ = app.update(Message::ModelsFetched(
            request,
            Ok(vec![catalog_model("available", &[])]),
        ));
        assert_eq!(app.settings.model, "custom-alias");
        assert!(app.model_catalog_loaded);
        assert!(!app.model_catalog_loading);
        assert!(app.model_catalog_status.contains("共 1 个模型，均可选择"));
        assert!(app.model_catalog_status.contains("未提供统一能力信息"));

        let _ = app.update(Message::SelectModel("available".into()));
        assert_eq!(app.settings.model, "available");
        let _ = app.update(Message::SaveSettings);
        assert_eq!(AiSettings::load(root.path()).unwrap().model, "available");
    }

    #[test]
    fn google_non_chat_models_are_displayed_but_cannot_replace_current_model() {
        let (_root, mut app) = model_settings_app();
        let _ = app.update(Message::Provider(AiProvider::Google));
        let current = app.settings.model.clone();
        let _ = app.update(Message::FetchModels);
        let request = app.model_catalog_request;
        let _ = app.update(Message::ModelsFetched(
            request,
            Ok(vec![
                catalog_model("gemini-text", &["generateContent"]),
                catalog_model("embedding-only", &["embedContent"]),
            ]),
        ));
        assert_eq!(app.model_catalog.options().len(), 2);
        assert!(
            app.model_catalog.options()[1]
                .to_string()
                .contains("不适用于当前创作")
        );
        assert!(
            app.model_catalog_status
                .contains("共 2 个模型，1 个支持当前生成接口")
        );
        let _ = app.update(Message::SelectModel("embedding-only".into()));
        assert_eq!(app.settings.model, current);
        assert!(app.status.contains("不支持当前生成接口"));
        let _ = app.update(Message::SelectModel("gemini-text".into()));
        assert_eq!(app.settings.model, "gemini-text");
    }

    #[test]
    fn failed_model_refresh_preserves_catalog_and_selected_model_for_saving() {
        let (root, mut app) = model_settings_app();
        let _ = app.update(Message::FetchModels);
        let request = app.model_catalog_request;
        let _ = app.update(Message::ModelsFetched(
            request,
            Ok(vec![catalog_model("available", &[])]),
        ));
        let _ = app.update(Message::SelectModel("available".into()));
        let _ = app.update(Message::FetchModels);
        let request = app.model_catalog_request;
        let _ = app.update(Message::ModelsFetched(request, Err("服务暂不可用".into())));
        assert_eq!(app.settings.model, "available");
        assert_eq!(app.model_catalog.options().len(), 1);
        assert!(!app.model_catalog_loading);
        assert!(app.model_catalog_status.contains("已保留当前选择"));
        let _ = app.update(Message::SaveSettings);
        assert_eq!(AiSettings::load(root.path()).unwrap().model, "available");
    }

    #[test]
    fn address_or_key_changes_invalidate_catalog_and_discard_stale_success_and_failure() {
        for field in ["base_url", "api_key"] {
            let (_root, mut app) = model_settings_app();
            let _ = app.update(Message::FetchModels);
            let first = app.model_catalog_request;
            let _ = app.update(Message::ModelsFetched(
                first,
                Ok(vec![catalog_model("old", &[])]),
            ));
            let _ = app.update(Message::FetchModels);
            let stale = app.model_catalog_request;
            let _ = app.update(Message::Setting(field, "changed-config".into()));
            let status = app.model_catalog_status.clone();
            assert!(app.model_catalog.options().is_empty());
            assert!(!app.model_catalog_loaded);
            assert!(!app.model_catalog_loading);
            assert_ne!(app.model_catalog_request, stale);
            let _ = app.update(Message::FetchModels);
            let current = app.model_catalog_request;
            let _ = app.update(Message::ModelsFetched(
                stale,
                Ok(vec![catalog_model("stale", &[])]),
            ));
            let _ = app.update(Message::ModelsFetched(stale, Err("过期错误".into())));
            assert!(app.model_catalog_loading);
            assert!(app.model_catalog.options().is_empty());
            assert!(!app.model_catalog_status.contains("过期错误"));
            assert_ne!(app.model_catalog_status, status);
            let _ = app.update(Message::ModelsFetched(
                current,
                Ok(vec![catalog_model("current", &[])]),
            ));
            assert_eq!(app.model_catalog.options()[0].info.id, "current");
        }
    }

    #[test]
    fn provider_change_invalidates_catalog_and_stale_errors_do_not_replace_status() {
        let (_root, mut app) = model_settings_app();
        let _ = app.update(Message::FetchModels);
        let stale = app.model_catalog_request;
        let _ = app.update(Message::Provider(AiProvider::Google));
        let status = app.status.clone();
        let catalog_status = app.model_catalog_status.clone();
        let _ = app.update(Message::ModelsFetched(stale, Err("过期错误".into())));
        let _ = app.update(Message::ModelsFetched(
            stale,
            Ok(vec![catalog_model("stale", &[])]),
        ));
        assert_eq!(app.status, status);
        assert_eq!(app.model_catalog_status, catalog_status);
        assert!(app.model_catalog.options().is_empty());
    }

    #[test]
    fn late_refresh_result_cannot_replace_newer_catalog_or_failure() {
        let (_root, mut app) = model_settings_app();
        let _ = app.update(Message::FetchModels);
        let first = app.model_catalog_request;
        let _ = app.update(Message::ModelsFetched(
            first,
            Ok(vec![catalog_model("first", &[])]),
        ));
        let _ = app.update(Message::FetchModels);
        let second = app.model_catalog_request;
        let _ = app.update(Message::ModelsFetched(second, Err("本次错误".into())));
        let status = app.model_catalog_status.clone();
        let _ = app.update(Message::ModelsFetched(
            first,
            Ok(vec![catalog_model("late", &[])]),
        ));
        let _ = app.update(Message::ModelsFetched(first, Err("过期错误".into())));
        assert_eq!(app.model_catalog_status, status);
        assert_eq!(app.model_catalog.options()[0].info.id, "first");
    }

    #[test]
    fn model_selection_and_numeric_edits_keep_catalog_and_same_credentials_do_not_invalidate() {
        let (_root, mut app) = model_settings_app();
        let _ = app.update(Message::FetchModels);
        let request = app.model_catalog_request;
        let _ = app.update(Message::ModelsFetched(
            request,
            Ok(vec![catalog_model("available", &[])]),
        ));
        let _ = app.update(Message::Provider(app.settings.provider));
        let _ = app.update(Message::Setting(
            "base_url",
            app.settings.base_url.clone().into(),
        ));
        let _ = app.update(Message::Setting(
            "api_key",
            app.settings.api_key.clone().into(),
        ));
        let _ = app.update(Message::SelectModel("available".into()));
        for (field, value) in [
            ("tokens", "4096"),
            ("timeout", "20"),
            ("temperature", "0.7"),
        ] {
            let _ = app.update(Message::Setting(field, value.into()));
        }
        assert_eq!(app.model_catalog_request, request);
        assert_eq!(app.model_catalog.options().len(), 1);
        assert!(app.model_catalog_loaded);
        assert_eq!(app.settings.model, "available");
    }

    #[test]
    fn api_key_input_is_hidden_in_message_debug_output() {
        let message = Message::Setting("api_key", "do-not-print-this-test-key".into());
        let debug = format!("{message:?}");
        assert!(!debug.contains("do-not-print-this-test-key"));
        assert!(debug.contains("已隐藏"));
    }

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
        let _ = app.update(Message::FetchModels);
        let request = app.model_catalog_request;
        let _ = app.update(Message::ModelsFetched(
            request,
            Ok(vec![catalog_model(
                "gemini-available",
                &["generateContent"],
            )]),
        ));
        let _ = app.update(Message::SelectModel("gemini-available".into()));
        let _ = app.update(Message::SaveSettings);
        let saved = AiSettings::load(root.path()).unwrap();
        assert_eq!(saved.provider, AiProvider::Google);
        assert_eq!(saved.api_key, "new-provider-test-key");
        assert_eq!(saved.model, "gemini-available");
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
    fn switching_from_context_form_back_to_script_preserves_the_script() {
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
        assert_eq!(app.context_form.value().unwrap(), json!({}));
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
        assert_eq!(app.context_form.value().unwrap(), json!({"world":"B"}));
    }
    #[test]
    fn readable_result_and_stage_views_preserve_original_artifacts() {
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
        let original_script = app.session.draft().unwrap().text.clone();
        let artifacts = json!({"outline":{"scenes":[{"title":"雨夜重逢","对白":"林夏：你回来了。\n陈默：我没有迟到。"}]}});
        app.outputs.insert(key, artifacts.clone());
        app.select_episode(key.episode, 1);
        assert!(app.editor.text().contains("雨夜重逢"));
        assert!(app.editor.text().contains("\n    "));
        assert!(!app.editor.text().contains("\"scenes\""));
        assert!(!app.editor.text().contains("\\n"));
        assert_eq!(app.outputs[&key], artifacts);
        assert_eq!(app.session.draft().unwrap().text, original_script);

        let contexts = ContextStore::new(root.path().into());
        let stage = json!({"response":serde_json::to_string(&artifacts).unwrap()});
        contexts
            .write_stage(&key.project.to_string(), "readable", "writer", &stage)
            .unwrap();
        let _ = app.update(Message::SelectRun("readable".into()));
        let _ = app.update(Message::SelectStage("writer".into()));
        assert!(app.editor.text().contains("雨夜重逢"));
        assert!(!app.editor.text().contains("\\\""));
        assert_eq!(
            contexts
                .read_stage(&key.project.to_string(), "readable", "writer")
                .unwrap(),
            Some(stage)
        );
        assert_eq!(
            store.episodes(key.project).unwrap()[0]
                .description
                .as_deref(),
            Some(original_script.as_str())
        );
    }

    #[test]
    fn context_form_saves_typed_edits_and_supplies_outline_optimization() {
        let root = tempfile::tempdir().unwrap();
        let store = Store::open(root.path()).unwrap();
        let project = store.create_project("表单编辑", None).unwrap();
        let contexts = ContextStore::new(root.path().into());
        let mut expected = json!({"episode_count":12,"world":{"notes":"第一段\n第二段","custom_flag":true},"unknown_list":[null,7]});
        contexts
            .put_context(&project.id.to_string(), "project_outline", "v1", &expected)
            .unwrap();
        let mut app = App::new(
            store,
            Options {
                root: root.path().into(),
                ..Options::default()
            },
        );
        let _ = app.update(Message::Page(Page::Context));
        let _ = app.update(Message::ContextKind("project_outline".into()));
        let count = app.context_form.field_id(&["episode_count"]).unwrap();
        let _ = app.update(Message::ContextDocument(ContextFormMessage::Number(
            count,
            "24".into(),
        )));
        // The unrelated text editor must never be parsed or saved as context.
        app.editor = text_editor::Content::with_text("此编辑器不保存上下文表单");
        let _ = app.update(Message::SaveContext);
        expected["episode_count"] = json!(24);
        assert_eq!(
            contexts
                .get_context(&project.id.to_string(), "project_outline", "v1")
                .unwrap(),
            Some(expected.clone())
        );
        app.context_instruction = "增加悬念".into();
        let (_, input) = app
            .prepare_input("project_outline_optimize".into())
            .unwrap();
        assert_eq!(input.context["current_outline"], expected);

        let _ = app.update(Message::ContextDocument(ContextFormMessage::Number(
            count,
            "无效数字".into(),
        )));
        let _ = app.update(Message::SaveContext);
        assert!(app.status.contains("数字"));
        assert_eq!(
            contexts
                .get_context(&project.id.to_string(), "project_outline", "v1")
                .unwrap(),
            Some(expected.clone())
        );
        assert!(
            app.prepare_input("project_outline_optimize".into())
                .is_err()
        );
        let _ = app.update(Message::ContextKind("series_bible".into()));
        assert_eq!(app.context_form.value().unwrap(), json!({}));
        let (_, input) = app
            .prepare_input("project_outline_optimize".into())
            .unwrap();
        assert_eq!(input.context["current_outline"], expected);
    }

    #[test]
    fn failed_context_load_cannot_overwrite_the_existing_document() {
        let root = tempfile::tempdir().unwrap();
        let store = Store::open(root.path()).unwrap();
        let project = store.create_project("损坏文档", None).unwrap();
        let contexts = ContextStore::new(root.path().into());
        let path = contexts
            .context_path(&project.id.to_string(), "series_bible", "v1")
            .unwrap();
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let broken = "{incomplete";
        std::fs::write(&path, broken).unwrap();
        let mut app = App::new(
            store,
            Options {
                root: root.path().into(),
                ..Options::default()
            },
        );
        let _ = app.update(Message::Page(Page::Context));
        assert!(!app.context_document_ready);
        let _ = app.update(Message::SaveContext);
        assert!(app.status.contains("尚未载入"));
        assert_eq!(std::fs::read_to_string(path).unwrap(), broken);
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
    #[test]
    fn stage_states_require_artifacts_before_reporting_done() {
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
        app.production.set(key.episode, Gate::Structure, true);
        app.production.set(key.episode, Gate::Storyboard, true);
        app.outputs.insert(
            key,
            json!({
                "outline": {"scenes": []},
                "shots": [{"title": "测试镜头"}]
            }),
        );
        let states = app.stage_states();
        assert_eq!(states[2], StageState::Done);
        assert_eq!(states[4], StageState::Done);

        app.outputs.insert(key, json!({"outline": {"scenes": []}}));
        let states = app.stage_states();
        assert_eq!(states[2], StageState::Done);
        assert_eq!(states[4], StageState::Empty);
        app.production.set(0, Gate::Assets, true);
        assert_eq!(app.stage_states()[4], StageState::Active);
    }

    #[test]
    fn a_gate_cannot_be_confirmed_without_its_artifact() {
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
        let episode = app.session.current.unwrap().episode;
        app.toggle_gate(Gate::Storyboard);
        assert!(!app.production.confirmed(episode, Gate::Storyboard));
        assert!(app.status.contains("缺少对应产物"));
    }

    #[test]
    fn asset_gate_persists_and_requires_an_asset() {
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
        store
            .create_asset_item(pid, "林夏", "persona_visual", Some("主角"))
            .unwrap();
        app.reload();
        app.toggle_gate(Gate::Assets);
        assert!(app.production.assets_frozen);
        let saved = ProductionState::load(&ContextStore::new(root.path().into()), pid);
        assert!(saved.assets_frozen);
        assert_eq!(app.status, "冻结资产 已确认");
    }

    #[test]
    fn storyboard_input_collects_locked_visual_dna_and_parameters() {
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
            .create_asset_item(pid, "林夏", "persona_visual", Some("主角"))
            .unwrap();
        let contexts = ContextStore::new(root.path().into());
        contexts
            .put_context(
                &pid.to_string(),
                &format!("visual_dna.asset_item_{}", item.id),
                "v1",
                &json!({"visual_dna": {"hair": "black"}}),
            )
            .unwrap();
        app.reload();
        app.max_shots = "12".into();
        app.aspect_ratio = "16:9".into();
        app.prompt_style = "Midjourney v6".into();
        let (_, input) = app.prepare_input("workflow_storyboard".into()).unwrap();
        assert_eq!(input.action, "workflow_storyboard");
        assert_eq!(input.context["max_shots"], 12);
        assert_eq!(input.context["aspect_ratio"], "16:9");
        assert_eq!(input.context["prompt_style"], "mj_v6");
        let dna = input.context["locked_visual_dna"].as_array().unwrap();
        assert_eq!(dna.len(), 1);
        assert_eq!(dna[0]["name"], "林夏");
    }

    #[test]
    fn editing_and_saving_a_locked_script_unlocks_it() {
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
        app.toggle_script_lock();
        let key = app.session.current.unwrap();
        assert!(store.episode(key.episode).unwrap().script_locked);
        app.page = Page::Script;
        app.step = 0;
        app.editor = text_editor::Content::with_text("第 1 场 · 新站台 · 夜\n\n新的第一段。");
        assert!(app.save_script());
        assert!(!store.episode(key.episode).unwrap().script_locked);
        assert!(app.status.contains("锁定解除"));
    }

    #[test]
    fn long_scripts_get_a_taller_editor_surface() {
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
        app.page = Page::Script;
        app.step = 0;
        let short = app.editor_height();
        app.editor = text_editor::Content::with_text(&"一段很长的剧本内容。".repeat(2000));
        assert!(app.editor_height() > short);
        assert!(app.editor_height() <= 20_000.0);
    }
    #[test]
    fn a_generation_must_be_confirmed_before_it_starts() {
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
        let _ = app.update(Message::Run("script_optimize".into()));
        assert!(app.pending_run.is_some());
        assert!(app.busy.is_none());
        assert!(app.status.contains("等待确认"));
        let _ = app.update(Message::CancelRun);
        assert!(app.pending_run.is_none());
    }

    #[test]
    fn confirming_a_prepared_run_starts_a_single_busy_task() {
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
        app.settings.api_key = "test-key".into();
        let _ = app.update(Message::Run("script_optimize".into()));
        let key = app.session.current.unwrap();
        let _ = app.update(Message::ConfirmRun);
        assert!(app.pending_run.is_none());
        assert!(app.busy.is_some());
        assert_eq!(store.episode(key.episode).unwrap().exec_status, "running");
        let _ = app.update(Message::Cancel);
        assert!(app.busy.is_none());
    }

    #[test]
    fn switching_projects_or_episodes_drops_an_unconfirmed_generation() {
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
        let _ = app.update(Message::Run("script_optimize".into()));
        assert!(app.pending_run.is_some());
        let second = id(app.episodes.get(1).unwrap());
        let _ = app.update(Message::Episode(second, 0));
        assert!(app.pending_run.is_none());
    }
    #[test]
    fn changing_the_script_after_the_preview_requires_a_new_confirmation() {
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
        app.settings.api_key = "test-key".into();
        let _ = app.update(Message::Run("script_optimize".into()));
        assert!(app.pending_run.is_some());
        app.page = Page::Script;
        app.step = 0;
        app.editor = text_editor::Content::with_text("预览之后改过的正文。");
        let _ = app.update(Message::ConfirmRun);
        assert!(app.pending_run.is_none());
        assert!(app.busy.is_none());
        assert!(app.status.contains("输入已发生变化"));
    }
    #[test]
    fn imported_project_result_selects_the_new_project() {
        let root = tempfile::tempdir().unwrap();
        let store = Store::open(root.path()).unwrap();
        let mut app = App::new(
            store.clone(),
            Options {
                root: root.path().into(),
                demo: false,
                ..Options::default()
            },
        );
        assert!(app.project.is_none());
        let project = store.create_project("导入项目", None).unwrap();
        let _ = app.update(Message::ProjectImported(Ok(Some(project.id))));
        assert_eq!(
            app.project.as_ref().map(|project| project.id),
            Some(project.id)
        );
        assert!(app.status.contains("项目已导入"));
    }
    #[test]
    fn export_file_stem_removes_path_separators() {
        assert_eq!(export_file_stem("../A/B"), ".._A_B");
        assert_eq!(export_file_stem("   "), "星晴项目");
    }
}
