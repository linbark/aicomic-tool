//! Native atelier state. Navigation never performs implicit loading, saving or rebinding.
use std::collections::{BTreeMap, HashMap};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
    mpsc,
};

use iced::widget::text_editor;
use ie::studio::StudioConfig;
use serde_json::{Value, json};

use crate::runtime;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Kind {
    #[default]
    Technique,
    Recipe,
    Formula,
}
impl Kind {
    pub const ALL: [Self; 3] = [Self::Technique, Self::Recipe, Self::Formula];
    pub fn key(self) -> &'static str {
        match self {
            Self::Technique => "technique",
            Self::Recipe => "recipe",
            Self::Formula => "formula",
        }
    }
    pub fn title(self) -> &'static str {
        match self {
            Self::Technique => "手法谱",
            Self::Recipe => "配方卷",
            Self::Formula => "丹方录",
        }
    }
    pub fn short(self) -> &'static str {
        match self {
            Self::Technique => "手法",
            Self::Recipe => "配方",
            Self::Formula => "丹方",
        }
    }
    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|kind| kind.key() == s)
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct AssetId {
    pub kind: Kind,
    pub key: String,
}
impl AssetId {
    pub fn from_value(v: &Value) -> Option<Self> {
        Some(Self {
            kind: Kind::parse(v["kind"].as_str()?)?,
            key: v["key"].as_str()?.into(),
        })
    }
}
#[derive(Clone, Debug)]
pub struct Scope {
    pub generation: u64,
    pub config: StudioConfig,
}
impl Scope {
    pub fn matches(&self, generation: u64, config: &StudioConfig) -> bool {
        self.generation == generation && runtime::same_config(&self.config, config)
    }
}
#[derive(Clone, Copy, Debug)]
pub enum Field {
    Name,
    InputType,
    Prompt,
    Schema,
    Definition,
    Constraints,
}
pub struct Draft {
    pub name: String,
    pub input_type: String,
    pub prompt: text_editor::Content,
    pub schema: text_editor::Content,
    pub definition: text_editor::Content,
    pub constraints: text_editor::Content,
    pub edit: u64,
    pub saved_edit: Option<u64>,
    // Only an untouched workshop snapshot may be replaced when a save is retried.
    workshop_edit: Option<u64>,
    pub advanced: bool,
}
impl Draft {
    pub fn from_revision(v: &Value) -> Self {
        let content = |key: &str| text_editor::Content::with_text(&crate::data::pretty(&v[key]));
        Self {
            name: v["name"].as_str().unwrap_or("").into(),
            input_type: v["input_type"].as_str().unwrap_or("any").into(),
            prompt: text_editor::Content::with_text(v["prompt"].as_str().unwrap_or("")),
            schema: content("output_schema"),
            definition: content("definition"),
            constraints: text_editor::Content::with_text(&crate::data::pretty(
                &v["definition"]["constraints"],
            )),
            edit: 0,
            saved_edit: Some(0),
            workshop_edit: None,
            advanced: false,
        }
    }
    pub fn dirty(&self) -> bool {
        self.saved_edit != Some(self.edit)
    }
    fn sync_recipe_fields(&mut self, field: Field) {
        match field {
            Field::Definition => {
                if let Ok(value) = serde_json::from_str::<Value>(&self.definition.text())
                    && value.is_object()
                {
                    self.constraints = text_editor::Content::with_text(
                        &serde_json::to_string_pretty(&value["constraints"])
                            .unwrap_or_else(|_| "null".into()),
                    );
                }
            }
            Field::Constraints => {
                if let (Ok(mut definition), Ok(constraints)) = (
                    serde_json::from_str::<Value>(&self.definition.text()),
                    serde_json::from_str::<Value>(&self.constraints.text()),
                ) && definition.is_object()
                {
                    definition["constraints"] = constraints;
                    self.definition =
                        text_editor::Content::with_text(&crate::data::pretty(&definition));
                }
            }
            _ => {}
        }
    }
    pub fn payload(&self, id: &AssetId) -> Result<Value, String> {
        let schema: Value = serde_json::from_str(&self.schema.text())
            .map_err(|e| format!("输出契约 JSON 无效：{e}"))?;
        let mut definition: Value = serde_json::from_str(&self.definition.text())
            .map_err(|e| format!("结构 JSON 无效：{e}"))?;
        if !schema.is_object() || !definition.is_object() {
            return Err("输出契约和结构必须为 JSON object。".into());
        }
        if id.kind == Kind::Recipe {
            definition["constraints"] = serde_json::from_str(&self.constraints.text())
                .map_err(|e| format!("约束 JSON 无效：{e}"))?;
        }
        if self.name.trim().is_empty() {
            return Err("请先填写资产名称。".into());
        }
        Ok(
            json!({"kind":id.kind.key(),"key":id.key,"name":self.name.trim(),"prompt":self.prompt.text(),"input_type":self.input_type,"output_schema":schema,"definition":definition}),
        )
    }
}
#[derive(Default)]
pub struct Catalog {
    pub query: String,
    pub selected: Option<AssetId>,
    pub preview: Option<u32>,
}
#[derive(Clone, Debug, PartialEq)]
pub struct Node {
    pub id: String,
    pub op: String,
    pub inputs: String,
    pub revision: Option<Value>,
    pub position: (f32, f32),
}
impl Node {
    pub fn value(&self) -> Value {
        let mut v = json!({"id": self.id, "op":self.op, "inputs": self.inputs.split(',').map(str::trim).filter(|s| !s.is_empty()).collect::<Vec<_>>()});
        if let Some(rev) = &self.revision {
            v["asset"] = json!({"kind":rev["kind"],"key":rev["key"],"version":rev["version"]});
        }
        v
    }
    pub fn name(&self) -> &str {
        self.revision
            .as_ref()
            .and_then(|v| v["name"].as_str())
            .unwrap_or(if self.op == "source" {
                "输入源料"
            } else {
                "导出产物"
            })
    }
}
pub struct Workshop {
    pub name: String,
    pub nodes: Vec<Node>,
    pub selected: Option<usize>,
    pub picker: bool,
    pub picker_kind: Kind,
    pub picker_query: String,
    pub source: text_editor::Content,
    pub use_work: bool,
    pub work: Option<i64>,
    pub formula_key: String,
    pub model: Model,
    pub show_config: bool,
    pub show_output: bool,
    pub history: Option<i64>,
    pub pending_formula: Option<Value>,
    pub next_node: u64,
}
impl Default for Workshop {
    fn default() -> Self {
        Self {
            name: "未命名丹方".into(),
            nodes: vec![Node {
                id: "source".into(),
                op: "source".into(),
                inputs: String::new(),
                revision: None,
                position: (40., 100.),
            }],
            selected: Some(0),
            picker: false,
            picker_kind: Kind::Technique,
            picker_query: String::new(),
            source: text_editor::Content::new(),
            use_work: false,
            work: None,
            formula_key: String::new(),
            model: Model::default(),
            show_config: false,
            show_output: false,
            history: None,
            pending_formula: None,
            next_node: 1,
        }
    }
}
#[derive(Clone)]
pub struct Model {
    pub protocol: String,
    pub endpoint: String,
    pub name: String,
    pub temperature: String,
    pub context: String,
    pub max_tokens: String,
    pub timeout: String,
    pub vision: bool,
    pub models: Vec<String>,
    pub connection: String,
    pub request: u64,
    pub loading: bool,
}
impl Default for Model {
    fn default() -> Self {
        Self {
            protocol: "ollama".into(),
            endpoint: "http://127.0.0.1:11434".into(),
            name: String::new(),
            temperature: "0.2".into(),
            context: "8192".into(),
            max_tokens: "2048".into(),
            timeout: "120".into(),
            vision: false,
            models: vec![],
            connection: "未连接 · 点击刷新模型列表才检测".into(),
            request: 0,
            loading: false,
        }
    }
}
impl Model {
    pub fn value(&self) -> Result<Value, String> {
        let integer =
            |s: &str, label: &str| s.parse::<u32>().map_err(|_| format!("{label}须为正整数"));
        Ok(
            json!({"protocol":self.protocol,"endpoint":self.endpoint,"model":self.name,"temperature":self.temperature.parse::<f64>().map_err(|_|"Temperature 无效")?,"context":integer(&self.context,"Context")?,"max_tokens":integer(&self.max_tokens,"最大输出")?,"timeout_seconds":integer(&self.timeout,"超时")?,"vision":self.vision}),
        )
    }
}
#[derive(Clone, Copy, Debug)]
pub enum ModelField {
    Protocol,
    Endpoint,
    Name,
    Temperature,
    Context,
    MaxTokens,
    Timeout,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RunState {
    Running,
    StopRequested,
    Finished(String),
}
pub struct ActiveRun {
    pub scope: Scope,
    pub request: u64,
    pub state: RunState,
    pub cancel: Arc<AtomicBool>,
    pub receiver: mpsc::Receiver<Value>,
    pub events: Vec<Value>,
    pub result: Option<Value>,
}
impl ActiveRun {
    pub fn active(&self) -> bool {
        !matches!(self.state, RunState::Finished(_))
    }
}
#[derive(Clone, Debug)]
pub struct SaveRequest {
    pub request: u64,
    pub id: AssetId,
    pub edit: u64,
}
#[derive(Clone, Debug)]
pub enum Message {
    Tab(Kind),
    Query(String),
    Select(AssetId),
    New,
    Edit(Field, String),
    Editor(Field, text_editor::Action),
    Advanced,
    Preview(u32),
    Restore(u32),
    Save,
    Saved {
        scope: Scope,
        request: u64,
        result: Result<Value, String>,
    },
    Picker,
    PickerTab(Kind),
    PickerQuery(String),
    Bind(Value, bool),
    Send,
    ConfirmFormula(bool),
    SelectNode(usize),
    MoveNode(usize, f32, f32),
    RemoveNode,
    NodeInputs(String),
    AddSource,
    AddExport,
    EditBound,
    PlanName(String),
    FormulaKey(String),
    SavePlan,
    Source(text_editor::Action),
    UseWork(bool),
    SetWork(i64),
    Configure,
    NodeDetails,
    Model(ModelField, String),
    Vision(bool),
    Discover,
    Models {
        scope: Scope,
        request: u64,
        result: Result<Value, String>,
    },
    Run,
    Stop,
    Event {
        scope: Scope,
        request: u64,
        event: Value,
    },
    Finished {
        scope: Scope,
        request: u64,
        result: Result<Value, String>,
    },
    Tick,
    Output(bool),
    History(i64),
    AddComponent,
    Component(usize, String, String),
    RemoveComponent(usize),
    FormulaNode(usize, String, String),
    AddFormulaNode,
    RemoveFormulaNode(usize),
}
pub enum Effect {
    None,
    Save {
        scope: Scope,
        request: u64,
        draft: Value,
    },
    Discover {
        scope: Scope,
        request: u64,
        model: Value,
    },
    Run {
        scope: Scope,
        request: u64,
        plan: Value,
        model: Value,
        cancel: Arc<AtomicBool>,
        sender: mpsc::Sender<Value>,
    },
    EditAsset(AssetId),
}
pub struct Context<'a> {
    pub config: &'a StudioConfig,
    pub can_write: bool,
}
pub struct Atelier {
    pub generation: u64,
    pub kind: Kind,
    pub catalogs: BTreeMap<Kind, Catalog>,
    pub assets: BTreeMap<AssetId, Vec<Value>>,
    pub drafts: HashMap<AssetId, Draft>,
    pub workshop: Workshop,
    pub runs: Vec<Value>,
    pub available: bool,
    pub integrity_error: Option<String>,
    pub note: String,
    pub error: bool,
    pub save: Option<SaveRequest>,
    pub run: Option<ActiveRun>,
    serial: u64,
}
impl Default for Atelier {
    fn default() -> Self {
        Self {
            generation: 0,
            kind: Kind::Technique,
            catalogs: Kind::ALL
                .into_iter()
                .map(|k| (k, Catalog::default()))
                .collect(),
            assets: BTreeMap::new(),
            drafts: HashMap::new(),
            workshop: Workshop::default(),
            runs: vec![],
            available: false,
            integrity_error: None,
            note: "先在藏经阁保存资产版本，再从工坊明确取用。".into(),
            error: false,
            save: None,
            run: None,
            serial: 0,
        }
    }
}
impl Atelier {
    pub fn catalog(&self) -> &Catalog {
        &self.catalogs[&self.kind]
    }
    pub fn selected(&self) -> Option<&AssetId> {
        self.catalog().selected.as_ref()
    }
    pub fn draft(&self) -> Option<&Draft> {
        self.selected().and_then(|id| self.drafts.get(id))
    }
    pub fn running(&self) -> bool {
        self.run.as_ref().is_some_and(ActiveRun::active)
    }
    /// A new asset root changes async identity, not the user's database or drafts.
    pub fn rebase(&mut self) {
        self.generation = self.generation.wrapping_add(1);
        self.workshop.model.request = self.workshop.model.request.wrapping_add(1);
        self.workshop.model.loading = false;
        self.workshop.model.connection = "目录已变化 · 未检测".into();
        self.note = "素材目录已更改；藏经阁草稿与工坊装配均已保留。".into();
    }
    pub fn reset(&mut self) {
        let generation = self.generation.wrapping_add(1);
        if let Some(run) = &self.run {
            run.cancel.store(true, Ordering::Relaxed);
        }
        *self = Self::default();
        self.generation = generation;
    }
    pub fn ingest(&mut self, value: &Value) {
        self.available = value["available"].as_bool().unwrap_or(false);
        self.integrity_error = value["error"].as_str().map(str::to_owned);
        self.assets.clear();
        for asset in value["assets"].as_array().into_iter().flatten() {
            if let Some(id) = AssetId::from_value(asset) {
                let revisions = asset["revisions"].as_array().cloned().unwrap_or_default();
                self.assets.insert(id.clone(), revisions);
                self.ensure_draft(&id);
            }
        }
        self.runs = value["runs"].as_array().cloned().unwrap_or_default();
        for kind in Kind::ALL {
            if self.catalogs[&kind].selected.is_none() {
                self.catalogs.get_mut(&kind).unwrap().selected =
                    self.assets.keys().find(|id| id.kind == kind).cloned();
            }
        }
    }
    fn ensure_draft(&mut self, id: &AssetId) {
        if !self.drafts.contains_key(id)
            && let Some(rev) = self.assets.get(id).and_then(|v| v.last())
        {
            self.drafts.insert(id.clone(), Draft::from_revision(rev));
        }
    }
    pub fn revision(&self, id: &AssetId, version: u32) -> Option<&Value> {
        self.assets
            .get(id)?
            .iter()
            .find(|v| v["version"].as_u64() == Some(version as u64))
    }
    pub fn visible_assets(&self, kind: Kind, query: &str) -> Vec<AssetId> {
        let query = query.trim().to_lowercase();
        let mut ids: Vec<_> = self
            .assets
            .keys()
            .chain(self.drafts.keys())
            .filter(|id| id.kind == kind)
            .cloned()
            .collect();
        ids.sort();
        ids.dedup();
        ids.into_iter()
            .filter(|id| {
                query.is_empty()
                    || id.key.to_lowercase().contains(&query)
                    || self
                        .drafts
                        .get(id)
                        .is_some_and(|d| d.name.to_lowercase().contains(&query))
            })
            .collect()
    }
    fn fail(&mut self, note: impl Into<String>) {
        self.note = note.into();
        self.error = true;
    }
    fn scope(&self, context: &Context<'_>) -> Scope {
        Scope {
            generation: self.generation,
            config: context.config.clone(),
        }
    }
    fn next(&mut self) -> u64 {
        self.serial = self.serial.wrapping_add(1);
        self.serial
    }
    pub fn plan(&self) -> Result<Value, String> {
        if self.workshop.name.trim().is_empty() {
            return Err("请填写丹方名称。".into());
        }
        let source = if self.workshop.use_work {
            json!({"work":self.workshop.work.ok_or("请先到资料 / 素材库选择真实素材，再点「使用所选素材」。")?})
        } else {
            let text = self.workshop.source.text();
            if text.trim().is_empty() {
                return Err("请输入任务文字，或选择真实素材作为输入。".into());
            }
            json!({"text":text})
        };
        Ok(
            json!({"name":self.workshop.name,"nodes":self.workshop.nodes.iter().map(Node::value).collect::<Vec<_>>(),"source":source}),
        )
    }
    fn bind(&mut self, rev: Value, replace: bool) {
        if self.running() {
            self.fail("运行中的计划已冻结；结束后再调整节点。");
            return;
        }
        let Some(id) = AssetId::from_value(&rev) else {
            self.fail("只可取用已保存的明确版本。");
            return;
        };
        let Some(rev) = rev["version"]
            .as_u64()
            .and_then(|version| self.revision(&id, version as u32))
            .cloned()
        else {
            self.fail("此版本尚未保存或不属于当前资料库；未改变装配。");
            return;
        };
        if rev["kind"] == "formula" {
            self.workshop.pending_formula = Some(rev);
            self.note = "载入丹方会替换装配。当前装配将保留到你确认；藏经阁草稿不会被覆盖。".into();
            return;
        }
        let op = rev["kind"].as_str().unwrap_or("technique").to_owned();
        if replace
            && let Some(index) = self.workshop.selected
            && let Some(node) = self.workshop.nodes.get_mut(index)
            && node.op != "source"
            && node.op != "export"
        {
            node.op = op;
            node.revision = Some(rev);
            self.note = "已明确替换选中节点的版本绑定。".into();
            return;
        }
        let id = format!("node-{}", self.workshop.next_node);
        self.workshop.next_node += 1;
        let inputs = self
            .workshop
            .selected
            .and_then(|i| self.workshop.nodes.get(i))
            .map(|n| n.id.clone())
            .unwrap_or_else(|| "source".into());
        let index = self.workshop.nodes.len();
        self.workshop.nodes.push(Node {
            id,
            op,
            inputs,
            revision: Some(rev),
            position: node_position(index),
        });
        self.workshop.selected = Some(index);
        self.note = "已加入保存版本；后续修订不会自动改变此绑定。".into();
    }
    fn save_draft(&mut self, id: AssetId, context: &Context<'_>) -> Effect {
        if !context.can_write || self.running() || self.save.is_some() {
            self.fail("当前只读或有任务执行中。请先建立工作副本 / 新库；不会迁移原库。");
            return Effect::None;
        }
        let Some(draft) = self.drafts.get(&id) else {
            return Effect::None;
        };
        let edit = draft.edit;
        match draft.payload(&id) {
            Ok(payload) => {
                let request = self.next();
                self.save = Some(SaveRequest { request, id, edit });
                self.note = "正在保存新的不可变版本…".into();
                Effect::Save {
                    scope: self.scope(context),
                    request,
                    draft: payload,
                }
            }
            Err(e) => {
                self.fail(e);
                Effect::None
            }
        }
    }
    pub fn update(&mut self, message: Message, context: Context<'_>) -> Effect {
        if let Some(error) = self.integrity_error.clone()
            && matches!(
                message,
                Message::Save
                    | Message::SavePlan
                    | Message::Run
                    | Message::Bind(_, _)
                    | Message::Send
                    | Message::ConfirmFormula(true)
            )
        {
            self.fail(format!(
                "版本库完整性错误：{error}。已停用保存、版本取用与开炉；旧资料仍可浏览。"
            ));
            return Effect::None;
        }
        // Reject stale results before touching even presentation state such as errors.
        let current = match &message {
            Message::Saved { scope, request, .. } => {
                scope.matches(self.generation, context.config)
                    && self.save.as_ref().is_some_and(|p| p.request == *request)
            }
            Message::Models { scope, request, .. } => {
                scope.matches(self.generation, context.config)
                    && self.workshop.model.request == *request
            }
            Message::Event { scope, request, .. } => {
                scope.matches(self.generation, context.config)
                    && self
                        .run
                        .as_ref()
                        .is_some_and(|r| r.request == *request && r.active())
            }
            Message::Finished { scope, request, .. } => {
                scope.matches(self.generation, context.config)
                    && self.run.as_ref().is_some_and(|r| r.request == *request)
            }
            _ => true,
        };
        if !current {
            return Effect::None;
        }
        if !matches!(message, Message::Tick | Message::Event { .. }) {
            self.error = false;
        }
        match message {
            Message::Tab(kind) => self.kind = kind,
            Message::Query(query) => self.catalogs.get_mut(&self.kind).unwrap().query = query,
            Message::Select(id) => {
                self.ensure_draft(&id);
                self.kind = id.kind;
                let c = self.catalogs.get_mut(&id.kind).unwrap();
                c.selected = Some(id);
                c.preview = None;
            }
            Message::New => {
                let n = self.next();
                let stamp = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis();
                let id = AssetId {
                    kind: self.kind,
                    key: format!("{}-{stamp}-{n}", self.kind.key()),
                };
                let definition = match self.kind {
                    Kind::Technique => json!({}),
                    Kind::Recipe => {
                        json!({"components":[{"input":"source","ratio":1.0}],"constraints":[]})
                    }
                    Kind::Formula => {
                        json!({"nodes":[{"id":"source","op":"source","inputs":[]},{"id":"export","op":"export","inputs":["source"]}]})
                    }
                };
                let mut draft = Draft::from_revision(
                    &json!({"name":"","input_type":"any","prompt":"","output_schema":{"type":"object"},"definition":definition}),
                );
                draft.saved_edit = None;
                self.drafts.insert(id.clone(), draft);
                self.catalogs.get_mut(&self.kind).unwrap().selected = Some(id);
            }
            Message::Edit(field, value) => {
                if let Some(id) = self.selected().cloned()
                    && let Some(d) = self.drafts.get_mut(&id)
                {
                    match field {
                        Field::Name => d.name = value,
                        Field::InputType => d.input_type = value,
                        Field::Prompt => d.prompt = text_editor::Content::with_text(&value),
                        Field::Schema => d.schema = text_editor::Content::with_text(&value),
                        Field::Definition => d.definition = text_editor::Content::with_text(&value),
                        Field::Constraints => {
                            d.constraints = text_editor::Content::with_text(&value)
                        }
                    }
                    if id.kind == Kind::Recipe {
                        d.sync_recipe_fields(field);
                    }
                    d.edit += 1;
                }
            }
            Message::Editor(field, action) => {
                if let Some(id) = self.selected().cloned()
                    && let Some(d) = self.drafts.get_mut(&id)
                {
                    let edit = action.is_edit();
                    match field {
                        Field::Prompt => d.prompt.perform(action),
                        Field::Schema => d.schema.perform(action),
                        Field::Definition => d.definition.perform(action),
                        Field::Constraints => d.constraints.perform(action),
                        _ => {}
                    }
                    if edit {
                        if id.kind == Kind::Recipe {
                            d.sync_recipe_fields(field);
                        }
                        d.edit += 1;
                    }
                }
            }
            Message::Advanced => {
                if let Some(id) = self.selected().cloned()
                    && let Some(d) = self.drafts.get_mut(&id)
                {
                    d.advanced = !d.advanced;
                }
            }
            Message::Preview(version) => {
                self.catalogs.get_mut(&self.kind).unwrap().preview = Some(version)
            }
            Message::Restore(version) => {
                if let Some(id) = self.selected().cloned()
                    && let Some(rev) = self.revision(&id, version).cloned()
                {
                    let next_edit = self.drafts.get(&id).map_or(1, |d| d.edit + 1);
                    let mut d = Draft::from_revision(&rev);
                    d.edit = next_edit;
                    d.saved_edit = None;
                    self.drafts.insert(id, d);
                    self.note = format!("v{version} 已恢复为草稿；历史版本未修改。");
                }
            }
            Message::Save => {
                if let Some(id) = self.selected().cloned() {
                    return self.save_draft(id, &context);
                }
            }
            Message::Saved {
                scope,
                request,
                result,
            } => {
                if !scope.matches(self.generation, context.config)
                    || self.save.as_ref().is_none_or(|p| p.request != request)
                {
                    return Effect::None;
                }
                let pending = self.save.take().unwrap();
                match result {
                    Ok(rev) => {
                        let Some(actual) = AssetId::from_value(&rev) else {
                            self.fail("版本已提交但返回字段不完整，请刷新核对，勿重复保存。");
                            return Effect::None;
                        };
                        if actual != pending.id {
                            self.fail("保存返回的资产标识不符，请刷新核对。");
                            return Effect::None;
                        }
                        let list = self.assets.entry(actual.clone()).or_default();
                        list.retain(|v| v["version"] != rev["version"]);
                        list.push(rev.clone());
                        list.sort_by_key(|v| v["version"].as_u64().unwrap_or(0));
                        if let Some(d) = self.drafts.get_mut(&actual)
                            && d.edit == pending.edit
                        {
                            d.saved_edit = Some(d.edit);
                        }
                        self.note = format!(
                            "已保存不可变 v{}；工坊已有节点仍固定原版本。",
                            rev["version"]
                        );
                    }
                    Err(error) => self.fail(error),
                }
            }
            Message::Picker => self.workshop.picker = !self.workshop.picker,
            Message::PickerTab(kind) => self.workshop.picker_kind = kind,
            Message::PickerQuery(query) => self.workshop.picker_query = query,
            Message::Bind(rev, replace) => self.bind(rev, replace),
            Message::Send => {
                if let Some(id) = self.selected().cloned() {
                    if self.draft().is_some_and(Draft::dirty) {
                        self.fail("草稿尚未保存。请先保存新版本，再明确取用。");
                    } else if let Some(rev) = self.assets.get(&id).and_then(|v| v.last()).cloned() {
                        self.bind(rev, false);
                    }
                }
            }
            Message::ConfirmFormula(confirm) => {
                if let Some(rev) = self.workshop.pending_formula.take()
                    && confirm
                {
                    let mut nodes = vec![];
                    for (i, v) in rev["definition"]["nodes"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .enumerate()
                    {
                        let revision = AssetId::from_value(&v["asset"])
                            .and_then(|id| {
                                self.revision(
                                    &id,
                                    v["asset"]["version"].as_u64().unwrap_or(0) as u32,
                                )
                            })
                            .cloned();
                        if v.get("asset").is_some() && revision.is_none() {
                            self.fail("丹方引用版本不可用，保留现有装配。");
                            return Effect::None;
                        }
                        nodes.push(Node {
                            id: v["id"].as_str().unwrap_or("").into(),
                            op: v["op"].as_str().unwrap_or("").into(),
                            inputs: v["inputs"]
                                .as_array()
                                .into_iter()
                                .flatten()
                                .filter_map(Value::as_str)
                                .collect::<Vec<_>>()
                                .join(", "),
                            revision,
                            position: node_position(i),
                        });
                    }
                    self.workshop.name = rev["name"].as_str().unwrap_or("未命名丹方").into();
                    self.workshop.nodes = nodes;
                    self.workshop.selected = Some(0);
                    self.workshop.formula_key = String::new();
                    self.workshop.next_node = 100;
                    self.note = "已明确载入保存丹方；源料输入与本地模型配置保持不变。".into();
                }
            }
            Message::SelectNode(index) => self.workshop.selected = Some(index),
            Message::MoveNode(index, x, y) => {
                if !self.running()
                    && let Some(n) = self.workshop.nodes.get_mut(index)
                {
                    n.position = (x.max(8.), y.max(8.));
                }
            }
            Message::NodeInputs(value) => {
                if !self.running()
                    && let Some(i) = self.workshop.selected
                    && let Some(n) = self.workshop.nodes.get_mut(i)
                {
                    n.inputs = value;
                }
            }
            Message::RemoveNode => {
                if !self.running()
                    && let Some(i) = self.workshop.selected
                    && i < self.workshop.nodes.len()
                {
                    self.workshop.nodes.remove(i);
                    self.workshop.selected = None;
                }
            }
            Message::AddSource | Message::AddExport => {
                if !self.running() {
                    let source = matches!(message, Message::AddSource);
                    let i = self.workshop.nodes.len();
                    let id = format!(
                        "{}-{}",
                        if source { "source" } else { "export" },
                        self.workshop.next_node
                    );
                    self.workshop.next_node += 1;
                    let inputs = if source {
                        String::new()
                    } else {
                        self.workshop
                            .selected
                            .and_then(|i| self.workshop.nodes.get(i))
                            .map(|n| n.id.clone())
                            .unwrap_or_default()
                    };
                    self.workshop.nodes.push(Node {
                        id,
                        op: if source { "source" } else { "export" }.into(),
                        inputs,
                        revision: None,
                        position: node_position(i),
                    });
                    self.workshop.selected = Some(i);
                }
            }
            Message::EditBound => {
                if let Some(i) = self.workshop.selected
                    && let Some(rev) = self.workshop.nodes.get(i).and_then(|n| n.revision.as_ref())
                    && let Some(id) = AssetId::from_value(rev)
                {
                    self.ensure_draft(&id);
                    self.kind = id.kind;
                    self.catalogs.get_mut(&id.kind).unwrap().selected = Some(id.clone());
                    return Effect::EditAsset(id);
                }
            }
            Message::PlanName(name) => self.workshop.name = name,
            Message::FormulaKey(key) => self.workshop.formula_key = key,
            Message::SavePlan => {
                if !context.can_write || self.running() || self.save.is_some() {
                    self.fail("当前只读或有任务执行中。请先建立工作副本 / 新库；不会迁移原库。");
                    return Effect::None;
                }
                if self.workshop.formula_key.trim().is_empty() {
                    let n = self.next();
                    let stamp = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_millis();
                    self.workshop.formula_key = format!("formula-{stamp}-{n}");
                }
                let id = AssetId {
                    kind: Kind::Formula,
                    key: self.workshop.formula_key.clone(),
                };
                if self
                    .drafts
                    .get(&id)
                    .is_some_and(|d| d.dirty() && d.workshop_edit != Some(d.edit))
                {
                    self.fail(
                        "藏经阁有这份丹方的未保存草稿。请在那里保存或另用新标识，未覆盖草稿。",
                    );
                    return Effect::None;
                }
                let v = json!({"name":self.workshop.name,"input_type":"any","prompt":"","output_schema":{"type":"object"},"definition":{"nodes":self.workshop.nodes.iter().map(Node::value).collect::<Vec<_>>()}});
                let mut draft = Draft::from_revision(&v);
                draft.saved_edit = None;
                draft.workshop_edit = Some(draft.edit);
                self.drafts.insert(id.clone(), draft);
                return self.save_draft(id, &context);
            }
            Message::Source(action) => self.workshop.source.perform(action),
            Message::UseWork(value) => self.workshop.use_work = value,
            Message::SetWork(id) => {
                self.workshop.work = Some(id);
                self.workshop.use_work = true;
            }
            Message::Configure => self.workshop.show_config = !self.workshop.show_config,
            Message::NodeDetails => self.workshop.show_config = false,
            Message::Model(field, value) => {
                let model = &mut self.workshop.model;
                match field {
                    ModelField::Protocol => model.protocol = value,
                    ModelField::Endpoint => model.endpoint = value,
                    ModelField::Name => model.name = value,
                    ModelField::Temperature => model.temperature = value,
                    ModelField::Context => model.context = value,
                    ModelField::MaxTokens => model.max_tokens = value,
                    ModelField::Timeout => model.timeout = value,
                }
                model.request += 1;
                model.loading = false;
                model.connection = "配置已修改 · 未检测".into();
            }
            Message::Vision(value) => self.workshop.model.vision = value,
            Message::Discover => {
                if !self.workshop.model.loading {
                    match self.workshop.model.value() {
                        Ok(model) => {
                            self.workshop.model.request += 1;
                            self.workshop.model.loading = true;
                            self.workshop.model.connection = "正在请求本地模型列表…".into();
                            return Effect::Discover {
                                scope: self.scope(&context),
                                request: self.workshop.model.request,
                                model,
                            };
                        }
                        Err(e) => self.fail(e),
                    }
                }
            }
            Message::Models {
                scope,
                request,
                result,
            } => {
                if !scope.matches(self.generation, context.config)
                    || request != self.workshop.model.request
                {
                    return Effect::None;
                }
                let model = &mut self.workshop.model;
                model.loading = false;
                match result {
                    Ok(v) => {
                        model.models = v["models"]
                            .as_array()
                            .into_iter()
                            .flatten()
                            .filter_map(|v| v["name"].as_str().map(str::to_owned))
                            .collect();
                        model.connection = if v["connected"] == true {
                            format!("已连接本地服务 · {} 个模型", model.models.len())
                        } else {
                            format!(
                                "未连接 · {}",
                                v["error"].as_str().unwrap_or("服务未返回可用列表")
                            )
                        };
                    }
                    Err(e) => {
                        model.models.clear();
                        model.connection = format!("未连接 · {e}");
                    }
                }
            }
            Message::Run => {
                if !context.can_write || self.running() || self.save.is_some() {
                    self.fail("开炉需要可写工作副本，且没有其他保存或运行任务。");
                    return Effect::None;
                }
                match self
                    .plan()
                    .and_then(|plan| self.workshop.model.value().map(|model| (plan, model)))
                {
                    Ok((plan, model)) => {
                        let request = self.next();
                        let cancel = Arc::new(AtomicBool::new(false));
                        let (sender, receiver) = mpsc::channel();
                        self.run = Some(ActiveRun {
                            scope: self.scope(&context),
                            request,
                            state: RunState::Running,
                            cancel: cancel.clone(),
                            receiver,
                            events: vec![],
                            result: None,
                        });
                        self.workshop.history = None;
                        self.note = "后台执行中；可切页。进度仅来自真实节点事件。".into();
                        return Effect::Run {
                            scope: self.scope(&context),
                            request,
                            plan,
                            model,
                            cancel,
                            sender,
                        };
                    }
                    Err(e) => self.fail(e),
                }
            }
            Message::Stop => {
                if let Some(run) = self.run.as_mut()
                    && run.active()
                {
                    run.cancel.store(true, Ordering::Relaxed);
                    run.state = RunState::StopRequested;
                    self.note =
                        "已请求停止；阻塞中的本地请求需等待返回或超时，不再提交其结果。".into();
                }
            }
            Message::Event {
                scope,
                request,
                event,
            } => {
                if scope.matches(self.generation, context.config)
                    && let Some(run) = self.run.as_mut()
                    && run.request == request
                    && run.active()
                {
                    run.events.push(event);
                }
            }
            Message::Tick => {
                if let Some(run) = self.run.as_ref() {
                    let scope = run.scope.clone();
                    let request = run.request;
                    let events: Vec<_> = run.receiver.try_iter().collect();
                    for event in events {
                        self.update(
                            Message::Event {
                                scope: scope.clone(),
                                request,
                                event,
                            },
                            Context {
                                config: context.config,
                                can_write: context.can_write,
                            },
                        );
                    }
                }
            }
            Message::Finished {
                scope,
                request,
                result,
            } => {
                if !scope.matches(self.generation, context.config)
                    || self.run.as_ref().is_none_or(|r| r.request != request)
                {
                    return Effect::None;
                }
                let run = self.run.as_mut().unwrap();
                run.events.extend(run.receiver.try_iter());
                match result {
                    Ok(record) => {
                        let status = record["status"].as_str().unwrap_or("unknown").to_owned();
                        run.state = RunState::Finished(status.clone());
                        run.result = Some(record.clone());
                        if let Some(id) = record["id"].as_i64() {
                            self.runs.retain(|v| v["id"].as_i64() != Some(id));
                            self.runs.insert(0, record.clone());
                        }
                        self.note = format!(
                            "运行结束：{status}。{}",
                            record["error"]
                                .as_str()
                                .unwrap_or("请审阅实际 JSON 与输出路径。")
                        );
                        self.error = status != "completed";
                    }
                    Err(e) => {
                        run.state = RunState::Finished("failed".into());
                        self.fail(e);
                    }
                }
            }
            Message::Output(value) => self.workshop.show_output = value,
            Message::History(id) => {
                self.workshop.history = Some(id);
                self.workshop.show_output = true;
            }
            Message::AddComponent
            | Message::RemoveComponent(_)
            | Message::Component(_, _, _)
            | Message::AddFormulaNode
            | Message::RemoveFormulaNode(_)
            | Message::FormulaNode(_, _, _) => self.structured_edit(message),
        }
        Effect::None
    }
    fn structured_edit(&mut self, message: Message) {
        let Some(id) = self.selected().cloned() else {
            return;
        };
        let Some(d) = self.drafts.get_mut(&id) else {
            return;
        };
        let Ok(mut definition) = serde_json::from_str::<Value>(&d.definition.text()) else {
            self.fail("结构 JSON 无效，请先在高级编辑中修正。");
            return;
        };
        if !definition.is_object() {
            self.fail("结构需要 JSON object，请先在高级编辑中修正。");
            return;
        }
        match message {
            Message::AddComponent => {
                if let Some(a) = definition["components"].as_array_mut() {
                    a.push(json!({"input":"source","ratio":1.0}));
                }
            }
            Message::RemoveComponent(i) => {
                if let Some(a) = definition["components"].as_array_mut()
                    && i < a.len()
                {
                    a.remove(i);
                }
            }
            Message::Component(i, key, value) => {
                if let Some(v) = definition["components"].get_mut(i) {
                    if !v.is_object() {
                        self.fail("配方维度需要 JSON object，请在高级编辑中修正。");
                        return;
                    }
                    if key == "ratio" {
                        let Ok(n) = value.parse::<f64>() else {
                            return;
                        };
                        v[key] = json!(n);
                    } else {
                        v[key] = json!(value);
                    }
                }
            }
            Message::AddFormulaNode => {
                if let Some(a) = definition["nodes"].as_array_mut() {
                    a.push(json!({"id":format!("node-{}",a.len()+1),"op":"export","inputs":["source"]}));
                }
            }
            Message::RemoveFormulaNode(i) => {
                if let Some(a) = definition["nodes"].as_array_mut()
                    && i < a.len()
                {
                    a.remove(i);
                }
            }
            Message::FormulaNode(i, key, value) => {
                if let Some(v) = definition["nodes"].get_mut(i) {
                    if !v.is_object() {
                        self.fail("丹方节点需要 JSON object，请在高级编辑中修正。");
                        return;
                    }
                    match key.as_str() {
                        "op" => {
                            if let Some(object) = v.as_object_mut() {
                                object.remove("asset");
                            }
                            v["op"] = json!(value);
                            if v["op"] == "source" {
                                v["inputs"] = json!([]);
                            }
                        }
                        "inputs" => {
                            v[key] = json!(
                                value
                                    .split(',')
                                    .map(str::trim)
                                    .filter(|s| !s.is_empty())
                                    .collect::<Vec<_>>()
                            )
                        }
                        "asset" => {
                            if let Ok(asset) = serde_json::from_str::<Value>(&value) {
                                v["asset"] = asset;
                            }
                        }
                        _ => v[key] = json!(value),
                    }
                }
            }
            _ => {}
        }
        d.definition = text_editor::Content::with_text(&crate::data::pretty(&definition));
        d.edit += 1;
    }
}
pub fn node_position(i: usize) -> (f32, f32) {
    let col = i % 3;
    let row = i / 3;
    (40. + col as f32 * 280., 72. + row as f32 * 196.)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn config() -> StudioConfig {
        StudioConfig {
            db: "/isolated/native.db".into(),
            root: "/isolated/assets".into(),
            recipes: "/isolated/recipes".into(),
        }
    }
    fn update(a: &mut Atelier, m: Message, writable: bool) -> Effect {
        a.update(
            m,
            Context {
                config: &config(),
                can_write: writable,
            },
        )
    }
    #[test]
    fn unsaved_revision_cannot_bind_even_when_it_has_a_version_number() {
        let mut a = Atelier::default();
        update(
            &mut a,
            Message::Bind(
                json!({"kind":"technique","key":"invented","version":1,"name":"未保存","prompt":"no"}),
                false,
            ),
            false,
        );
        assert_eq!(
            a.workshop.nodes.len(),
            1,
            "only persisted revisions may enter the workshop"
        );
    }
    #[test]
    fn invalid_recipe_definition_is_reported_without_panicking() {
        let mut a = Atelier::default();
        update(&mut a, Message::Tab(Kind::Recipe), true);
        update(&mut a, Message::New, true);
        update(&mut a, Message::Edit(Field::Name, "约束配方".into()), true);
        update(&mut a, Message::Edit(Field::Definition, "[]".into()), true);
        assert!(matches!(update(&mut a, Message::Save, true), Effect::None));
        assert!(a.error);
        assert!(a.save.is_none());
    }
    #[test]
    fn catalog_queries_and_drafts_survive_independent_navigation() {
        let mut a = Atelier::default();
        update(&mut a, Message::New, false);
        let id = a.selected().unwrap().clone();
        update(&mut a, Message::Edit(Field::Name, "自建手法".into()), false);
        update(&mut a, Message::Query("手法查找".into()), false);
        update(&mut a, Message::Tab(Kind::Recipe), false);
        update(&mut a, Message::Query("配方查找".into()), false);
        update(&mut a, Message::Tab(Kind::Technique), false);
        assert_eq!(a.catalog().query, "手法查找");
        assert_eq!(a.selected(), Some(&id));
        assert_eq!(a.draft().unwrap().name, "自建手法");
        assert_eq!(a.workshop.nodes.len(), 1);
    }
}
