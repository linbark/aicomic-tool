use crate::store::{component, now_ms};
use crate::{
    AiError, AiSettings, ChatMessage, ContextStore, LlmClient, PromptRegistry, Result, extract_json,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::HashSet,
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
};
use tokio::sync::mpsc::UnboundedSender;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ExecutionInput {
    /// Use the old numeric project primary key as a string to retain old on-disk paths.
    pub project_id: String,
    pub episode_id: Option<i64>,
    pub script: String,
    #[serde(default)]
    pub context: Value,
    pub action: String,
    pub run_id: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WorkflowResult {
    pub run_id: String,
    pub status: String,
    pub artifacts: Value,
    pub output_text: String,
    pub waiting_for: Option<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WorkflowEvent {
    pub run_id: String,
    pub stage: String,
    pub data: Value,
}

/// Retain creative outputs for a new Run without transferring execution authority.
///
/// This is deliberately an allowlist: approval fields, pending mutation plans,
/// identifiers, status and future execution markers are owned by their original Run.
/// Do not use this when resuming an existing Run's persisted review state.
pub fn reusable_artifacts(previous: &Value) -> Value {
    const CREATIVE_OUTPUTS: &[&str] = &[
        "outline",
        "assets_visual_dna",
        "split_episodes",
        "script",
        "project_outline",
        "changes_summary",
        "visual_dna",
        "visual_dna_source",
        "series_bible",
        "beat_sheet",
        "script_fountain",
        "qc_report",
        "shots",
        "scenes",
        "derived",
    ];
    Value::Object(
        previous
            .as_object()
            .into_iter()
            .flat_map(|fields| fields.iter())
            .filter(|(key, _)| CREATIVE_OUTPUTS.contains(&key.as_str()))
            .map(|(key, value)| (key.clone(), value.clone()))
            .collect(),
    )
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReviewDecision {
    Confirmed,
    Regenerate,
    Rejected,
    Approve,
    Reject,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
struct ActionStep {
    action_key: String,
    #[serde(default)]
    input_text: String,
    #[serde(default)]
    why: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
struct ReviewState {
    #[serde(default = "resume_schema_version")]
    schema_version: u32,
    input: ExecutionInput,
    next_step: usize,
    steps: Vec<ActionStep>,
    artifacts: Value,
    kind: String,
}

#[derive(Clone, Debug)]
pub struct WorkflowEngine {
    settings: AiSettings,
    store: ContextStore,
    prompts: PromptRegistry,
    events: Option<UnboundedSender<WorkflowEvent>>,
    active: Arc<Mutex<HashSet<String>>>,
    log_sequence: Arc<AtomicU64>,
}
struct ActiveRun {
    active: Arc<Mutex<HashSet<String>>>,
    key: String,
}
impl Drop for ActiveRun {
    fn drop(&mut self) {
        if let Ok(mut set) = self.active.lock() {
            set.remove(&self.key);
        }
    }
}
impl WorkflowEngine {
    pub fn new(root: PathBuf, settings: AiSettings) -> Self {
        Self {
            settings,
            store: ContextStore::new(root.clone()),
            prompts: PromptRegistry::new(root),
            events: None,
            active: Arc::default(),
            log_sequence: Arc::new(AtomicU64::new(0)),
        }
    }
    pub fn with_events(mut self, sender: UnboundedSender<WorkflowEvent>) -> Self {
        self.events = Some(sender);
        self
    }
    pub fn store(&self) -> &ContextStore {
        &self.store
    }
    pub fn prompts(&self) -> &PromptRegistry {
        &self.prompts
    }
    pub async fn test_connection(&self) -> Result<String> {
        let mut settings = self.settings.clone();
        settings.temperature = 0.0;
        settings.max_tokens = Some(match settings.provider {
            crate::AiProvider::Google => 1024,
            crate::AiProvider::OpenAi => 8,
        });
        let text = LlmClient
            .chat(
                &settings,
                &[
                    ChatMessage {
                        role: "system".into(),
                        content: "You are a helpful assistant.".into(),
                    },
                    ChatMessage {
                        role: "user".into(),
                        content: "Reply with exactly: OK".into(),
                    },
                ],
            )
            .await?;
        Ok(if text.trim() == "OK" {
            "连接成功".into()
        } else {
            format!("连接成功（返回：{}）", preview(&text, 50))
        })
    }
    fn lock_run(&self, project: &str, run: &str) -> Result<ActiveRun> {
        component(project)?;
        component(run)?;
        let key = format!("{project}/{run}");
        let mut active = self
            .active
            .lock()
            .map_err(|_| AiError::InvalidState("任务锁不可用".into()))?;
        if !active.insert(key.clone()) {
            return Err(AiError::InvalidState("该 Run 正在执行".into()));
        }
        Ok(ActiveRun {
            active: self.active.clone(),
            key,
        })
    }
    fn stage(&self, input: &ExecutionInput, name: &str, data: Value) -> Result<()> {
        self.store
            .write_stage(&input.project_id, &input.run_id, name, &data)?;
        if let Some(sender) = &self.events {
            let _ = sender.send(WorkflowEvent {
                run_id: input.run_id.clone(),
                stage: name.into(),
                data,
            });
        }
        Ok(())
    }
    fn status(&self, input: &ExecutionInput, status: &str, extra: Value) -> Result<()> {
        let mut data = json!({"status":status,"at_ms":now_ms()});
        if let Some(extra) = extra.as_object() {
            for (k, v) in extra {
                data[k] = v.clone();
            }
        }
        self.stage(input, "chat.status", data)
    }
    fn log(&self, input: &ExecutionInput, stage: &str, summary: &str) -> Result<()> {
        let ts = now_ms();
        let seq = self.log_sequence.fetch_add(1, Ordering::Relaxed);
        self.stage(
            input,
            &format!("log.{ts}.{seq:03}"),
            json!({"at_ms":ts,"ts_ms":ts,"level":"INFO","stage":stage,"summary":summary}),
        )
    }
    fn snapshot(&self, input: &ExecutionInput, result: &WorkflowResult) -> Result<()> {
        self.store.snapshot_run(
            &input.project_id,
            &input.run_id,
            &serde_json::to_value(input)?,
            &serde_json::to_value(result)?,
            &json!({"workflow":input.action,"episode_id":input.episode_id}),
        )
    }
    fn fail(&self, input: &ExecutionInput, error: &AiError) {
        let text = error.to_string();
        let artifacts = self
            .store
            .read_run(&input.project_id, &input.run_id)
            .ok()
            .flatten()
            .map(|v| v["response"]["artifacts"].clone())
            .filter(|v| v.is_object())
            .unwrap_or(json!({}));
        let _ = self.stage(input, "chat.error", json!({"error":text,"at_ms":now_ms()}));
        let _ = self.status(input, "error", json!({}));
        let _ = self.snapshot(
            input,
            &WorkflowResult {
                run_id: input.run_id.clone(),
                status: "error".into(),
                artifacts,
                output_text: text,
                waiting_for: None,
            },
        );
    }
    pub async fn execute(&self, mut input: ExecutionInput) -> Result<WorkflowResult> {
        let _guard = self.lock_run(&input.project_id, &input.run_id)?;
        if let Some(previous) = input.context.get("artifacts") {
            input.context["artifacts"] = reusable_artifacts(previous);
        }
        if input.script.trim().is_empty() {
            return Err(AiError::InvalidInput("请输入剧本或创作指令".into()));
        }
        if self
            .store
            .read_run(&input.project_id, &input.run_id)?
            .is_some()
        {
            return Err(AiError::InvalidState(
                "Run 已存在，请使用新 Run ID 或恢复待审阅任务".into(),
            ));
        }
        self.snapshot(
            &input,
            &WorkflowResult {
                run_id: input.run_id.clone(),
                status: "running".into(),
                artifacts: json!({}),
                output_text: String::new(),
                waiting_for: None,
            },
        )?;
        self.status(&input, "running", json!({}))?;
        self.log(&input, "workflow.start", "开始执行 AI 工作流")?;
        let result = self.execute_inner(&input).await;
        if let Err(error) = &result {
            self.fail(&input, error);
        }
        result
    }
    async fn execute_inner(&self, input: &ExecutionInput) -> Result<WorkflowResult> {
        let mut artifacts = input
            .context
            .get("artifacts")
            .filter(|v| v.is_object())
            .cloned()
            .unwrap_or(json!({}));
        let steps = if input.action == "episode_execute" {
            [
                "episode_outline_generate",
                "episode_assets_visual_dna",
                "episode_split_episodes",
                "episode_asset_ingest",
            ]
            .into_iter()
            .map(|action| ActionStep {
                action_key: action.into(),
                input_text: String::new(),
                why: String::new(),
            })
            .collect()
        } else if input.action == "chat" {
            let planner_prompt = production_prompt("planner")?;
            let plan = self
                .json_call(input, "chat.planner", &planner_prompt, &input.script, true)
                .await?;
            self.stage(input, "chat.plan", json!({"plan":plan}))?;
            if plan["needs_clarification"].as_bool() == Some(true) {
                artifacts["clarification"] = plan.clone();
                let result = WorkflowResult {
                    run_id: input.run_id.clone(),
                    status: "needs_clarification".into(),
                    artifacts,
                    output_text: plan["clarifying_question"]
                        .as_str()
                        .unwrap_or("请补充创作要求")
                        .into(),
                    waiting_for: None,
                };
                self.status(input, "needs_clarification", json!({}))?;
                self.snapshot(input, &result)?;
                return Ok(result);
            }
            let allowed = [
                "outline_generate",
                "outline_optimize",
                "generate_script",
                "script_optimize",
                "workflow_script",
                "workflow_storyboard",
                "memory_extract_changeset",
            ];
            let raw = plan["steps"]
                .as_array()
                .ok_or_else(|| AiError::Protocol("Planner steps 为空或格式不正确".into()))?;
            let mut valid = vec![];
            for step in raw {
                let action = step["action_key"].as_str().unwrap_or("");
                if !allowed.contains(&action) {
                    return Err(AiError::UnsupportedAction(action.into()));
                }
                if valid.len() == 4 {
                    return Err(AiError::Protocol("Planner steps 不得超过4步".into()));
                }
                valid.push(ActionStep {
                    action_key: action.into(),
                    input_text: step["input_text"].as_str().unwrap_or("").into(),
                    why: step["why"].as_str().unwrap_or("").into(),
                });
            }
            if valid.is_empty() {
                return Err(AiError::Protocol("Planner steps 为空".into()));
            }
            artifacts["plan"] = plan;
            valid
        } else {
            vec![ActionStep {
                action_key: input.action.clone(),
                input_text: input.script.clone(),
                why: String::new(),
            }]
        };
        if input.action != "chat" {
            self.stage(input, "chat.plan", json!({"plan":{"steps":steps,"final_action_key":steps.last().map(|s|s.action_key.as_str())}}))?;
        }
        self.run_steps(input, &steps, 0, artifacts).await
    }
    fn outline_for_optimization(&self, input: &ExecutionInput) -> Result<Value> {
        match input.context.get("current_outline") {
            Some(current) if current.is_object() => Ok(current.clone()),
            Some(_) => Err(AiError::InvalidInput(
                "需要优化的项目大纲应包含分组内容，请重新载入后重试".into(),
            )),
            None => self
                .store
                .get_context(&input.project_id, "project_outline", "v1")?
                .ok_or_else(|| AiError::InvalidInput("请先保存需要优化的项目大纲".into())),
        }
    }
    fn with_context(&self, input: &ExecutionInput, stage: &str, text: &str) -> Result<String> {
        let bible = self
            .store
            .get_context(&input.project_id, "series_bible", "v1")?;
        let outline = if matches!(
            stage,
            "project_outline_optimize" | "project_outline_optimize.repair"
        ) {
            Some(self.outline_for_optimization(input)?)
        } else {
            self.store
                .get_context(&input.project_id, "project_outline", "v1")?
        };
        Ok(format!(
            "{text}\n\n### 创作上下文（遵循已确认设定）\n{}",
            json!({"series_bible":bible,"project_outline":outline,"provided":input.context})
        ))
    }
    async fn text_call(
        &self,
        input: &ExecutionInput,
        stage: &str,
        system: &str,
        user: &str,
        repair: bool,
    ) -> Result<String> {
        self.log(
            input,
            stage,
            if repair {
                "修复结构化输出"
            } else {
                "请求模型生成"
            },
        )?;
        let mut settings = self.settings.clone();
        if repair {
            settings.temperature = 0.0;
        }
        let user = self.with_context(input, stage, user)?;
        let output = LlmClient
            .chat(
                &settings,
                &[
                    ChatMessage {
                        role: "system".into(),
                        content: system.into(),
                    },
                    ChatMessage {
                        role: "user".into(),
                        content: user,
                    },
                ],
            )
            .await?;
        self.stage(
            input,
            &format!("{stage}.raw"),
            json!({"text":output,"at_ms":now_ms()}),
        )?;
        Ok(output)
    }
    async fn json_call(
        &self,
        input: &ExecutionInput,
        stage: &str,
        system: &str,
        user: &str,
        object: bool,
    ) -> Result<Value> {
        let output = self.text_call(input, stage, system, user, false).await?;
        if let Ok(value) = extract_json(&output)
            && ((object && value.is_object()) || (!object && value.is_array()))
        {
            return Ok(value);
        }
        let repaired = self.text_call(input, &format!("{stage}.repair"), system, &json!({"task":"repair_json_output","error":"Invalid JSON or unexpected root type","expected":if object {"JSON object"} else {"JSON array"},"bad_output":output}).to_string(), true).await?;
        let parsed = extract_json(&repaired)?;
        if (object && !parsed.is_object()) || (!object && !parsed.is_array()) {
            return Err(AiError::Protocol("LLM JSON 根类型不正确".into()));
        }
        Ok(parsed)
    }
    async fn run_steps(
        &self,
        input: &ExecutionInput,
        steps: &[ActionStep],
        start: usize,
        mut artifacts: Value,
    ) -> Result<WorkflowResult> {
        let mut output = String::new();
        for (index, step) in steps.iter().enumerate().skip(start) {
            let action = &step.action_key;
            let original = if !step.input_text.trim().is_empty() {
                step.input_text.clone()
            } else {
                match action.as_str() {
                    "generate_script" | "outline_optimize" => {
                        value_text(&artifacts["outline"]).unwrap_or_else(|| input.script.clone())
                    }
                    "script_optimize" => {
                        value_text(&artifacts["script"]).unwrap_or_else(|| input.script.clone())
                    }
                    _ => input.script.clone(),
                }
            };
            self.stage(input, &format!("chat.step.{index}.start"), json!({"step_index":index,"action_key":action,"why":step.why,"input_preview":preview(&original,400),"at_ms":now_ms()}))?;
            self.status(
                input,
                "running",
                json!({"current_step_index":index,"current_action_key":action}),
            )?;
            let started = now_ms();
            let result = self.action(input, action, &original, &mut artifacts).await;
            output = match result {
                Ok(output) => output,
                Err(error) => {
                    self.stage(input, &format!("chat.step.{index}.error"), json!({"step_index":index,"action_key":action,"error":error.to_string(),"at_ms":now_ms()}))?;
                    return Err(error);
                }
            };
            self.stage(input, &format!("chat.step.{index}.end"), json!({"step_index":index,"action_key":action,"ms":now_ms().saturating_sub(started),"output_preview":preview(&output,800),"at_ms":now_ms()}))?;
            self.snapshot(
                input,
                &WorkflowResult {
                    run_id: input.run_id.clone(),
                    status: "running".into(),
                    artifacts: artifacts.clone(),
                    output_text: output.clone(),
                    waiting_for: None,
                },
            )?;
            let kind = match action.as_str() {
                "episode_split_episodes" if input.action == "episode_execute" => {
                    Some("confirm_split")
                }
                "episode_asset_ingest" => Some("confirm_ingest"),
                "memory_extract_changeset" => Some("review_changeset"),
                _ => None,
            };
            if let Some(kind) = kind {
                return self.pause(input, steps, index + 1, artifacts, kind, output);
            }
        }
        let result = WorkflowResult {
            run_id: input.run_id.clone(),
            status: "done".into(),
            artifacts,
            output_text: output,
            waiting_for: None,
        };
        self.status(input, "done", json!({}))?;
        self.stage(
            input,
            "chat.final",
            json!({"run_id":input.run_id,"assistant_message":result.output_text}),
        )?;
        self.snapshot(input, &result)?;
        Ok(result)
    }
    fn pause(
        &self,
        input: &ExecutionInput,
        steps: &[ActionStep],
        next_step: usize,
        artifacts: Value,
        kind: &str,
        output: String,
    ) -> Result<WorkflowResult> {
        let state = ReviewState {
            schema_version: resume_schema_version(),
            input: input.clone(),
            next_step,
            steps: steps.to_vec(),
            artifacts: artifacts.clone(),
            kind: kind.into(),
        };
        self.stage(
            input,
            "chat.interrupt",
            json!({"kind":kind,"resume_state":state}),
        )?;
        self.status(input, "paused", json!({"waiting_for":kind}))?;
        let result = WorkflowResult {
            run_id: input.run_id.clone(),
            status: "paused".into(),
            artifacts,
            output_text: output,
            waiting_for: Some(kind.into()),
        };
        self.snapshot(input, &result)?;
        Ok(result)
    }
    /// Approval returns `approved_*` artifacts for the application transaction layer.
    /// This crate never claims database mutations have already been materialized.
    pub async fn resume(
        &self,
        project: &str,
        run: &str,
        decision: ReviewDecision,
        edited: Option<Value>,
    ) -> Result<WorkflowResult> {
        let _guard = self.lock_run(project, run)?;
        let status = self
            .store
            .read_stage(project, run, "chat.status")?
            .ok_or_else(|| AiError::InvalidState("Run 不存在".into()))?;
        if status["status"] != "paused" {
            return Err(AiError::InvalidState("该 Run 没有待处理的人工审核".into()));
        }
        let interrupt = self
            .store
            .read_stage(project, run, "chat.interrupt")?
            .ok_or_else(|| AiError::InvalidState("缺少任务恢复状态".into()))?;
        let raw_state = &interrupt["resume_state"];
        let mut state: ReviewState = if raw_state.get("input").is_some() {
            serde_json::from_value(raw_state.clone())?
        } else {
            // Legacy Python episode workflow stored a flat resume_state with both UUID and PK.
            // The folder/argument is authoritative; never move legacy numeric paths to a UUID.
            let old_pid = raw_state["project_id_pk"].as_i64().map(|p| p.to_string());
            if old_pid.as_deref() != Some(project) {
                return Err(AiError::InvalidState(
                    "旧任务项目主键与恢复目录不匹配".into(),
                ));
            }
            let legacy_run = raw_state["run_id"].as_str().unwrap_or("");
            if legacy_run != run {
                return Err(AiError::InvalidState("旧任务Run ID与恢复目录不匹配".into()));
            }
            let kind = interrupt["kind"].as_str().unwrap_or("");
            if !matches!(
                kind,
                "confirm_outline" | "confirm_assets" | "confirm_split" | "confirm_ingest"
            ) {
                return Err(AiError::InvalidState(
                    "旧任务恢复格式不受支持，请在记忆审核页处理已保存变更单".into(),
                ));
            }
            ReviewState {
                schema_version: resume_schema_version(),
                input: ExecutionInput {
                    project_id: project.into(),
                    episode_id: raw_state["episode_id"].as_i64(),
                    script: raw_state["script_text"].as_str().unwrap_or("").into(),
                    context: json!({}),
                    action: "episode_execute".into(),
                    run_id: run.into(),
                },
                next_step: raw_state["step_index"].as_u64().unwrap_or(0) as usize,
                steps: [
                    "episode_outline_generate",
                    "episode_assets_visual_dna",
                    "episode_split_episodes",
                    "episode_asset_ingest",
                ]
                .into_iter()
                .map(|action| ActionStep {
                    action_key: action.into(),
                    input_text: String::new(),
                    why: String::new(),
                })
                .collect(),
                artifacts: raw_state["artifacts"].clone(),
                kind: kind.into(),
            }
        };
        if state.schema_version != resume_schema_version() {
            return Err(AiError::InvalidState("任务恢复版本不受支持".into()));
        }
        if !state.artifacts.is_object()
            || state.steps.is_empty()
            || state.next_step > state.steps.len()
        {
            return Err(AiError::InvalidState("任务恢复产物或步骤状态已损坏".into()));
        }
        if state.input.project_id != project || state.input.run_id != run {
            return Err(AiError::InvalidState("恢复状态与项目或Run不匹配".into()));
        }
        if let Some(edited) = edited {
            let fields = edited
                .as_object()
                .ok_or_else(|| AiError::InvalidInput("审核产物必须为JSON object".into()))?;
            for (key, value) in fields {
                state.artifacts[key] = value.clone();
            }
        }
        if matches!(decision, ReviewDecision::Rejected | ReviewDecision::Reject) {
            self.status(&state.input, "rejected", json!({}))?;
            let result = WorkflowResult {
                run_id: run.into(),
                status: "rejected".into(),
                artifacts: state.artifacts,
                output_text: "已驳回，未提交待审阅产物。".into(),
                waiting_for: None,
            };
            self.snapshot(&state.input, &result)?;
            return Ok(result);
        }
        if matches!(decision, ReviewDecision::Regenerate) {
            state.next_step = state.next_step.saturating_sub(1);
        } else {
            match state.kind.as_str() {
                "confirm_outline" | "confirm_assets" => {}
                "confirm_split" => {
                    if state.artifacts["split_episodes"]["episodes"]
                        .as_array()
                        .is_none_or(|v| v.is_empty())
                    {
                        return Err(AiError::InvalidInput("缺少可确认的分集列表".into()));
                    }
                    state.artifacts["approved_split_episodes"] =
                        state.artifacts["split_episodes"].clone();
                }
                "confirm_ingest" => {
                    if !state.artifacts["asset_ingest_plan"].is_object()
                        || !state.artifacts["asset_ingest_plan"]["items"].is_array()
                    {
                        return Err(AiError::InvalidInput("缺少可确认的资产入库计划".into()));
                    }
                    state.artifacts["approved_asset_ingest_plan"] =
                        state.artifacts["asset_ingest_plan"].clone();
                }
                "review_changeset" => {
                    if state.artifacts["changeset"]["schema_version"] != "changeset.v0" {
                        return Err(AiError::InvalidInput("缺少可确认的changeset.v0".into()));
                    }
                    state.artifacts["approved_changeset"] = state.artifacts["changeset"].clone();
                }
                _ => return Err(AiError::InvalidState("未知的人工审核类型".into())),
            }
        }
        self.status(&state.input, "running", json!({}))?;
        let result = self
            .run_steps(&state.input, &state.steps, state.next_step, state.artifacts)
            .await;
        if let Err(error) = &result {
            self.fail(&state.input, error);
        }
        result
    }
    async fn action(
        &self,
        input: &ExecutionInput,
        action: &str,
        text: &str,
        artifacts: &mut Value,
    ) -> Result<String> {
        let user = text.to_owned();
        match action {
            "episode_outline_generate" => {
                let value = self
                    .json_call(
                        input,
                        action,
                        &production_prompt("episode_outline")?,
                        &user,
                        true,
                    )
                    .await?;
                artifacts["outline"] = value.clone();
                Ok(serde_json::to_string_pretty(&value)?)
            }
            "episode_assets_visual_dna" | "episode_split_episodes" => {
                let system = self.prompts.get(&format!("{action}_system"))?.prompt;
                let full = format!(
                    "{user}\n\n### outline\n{}\n\n### assets_visual_dna\n{}",
                    artifacts["outline"], artifacts["assets_visual_dna"]
                );
                let value = self.json_call(input, action, &system, &full, true).await?;
                let key = if action == "episode_assets_visual_dna" {
                    "assets_visual_dna"
                } else {
                    "split_episodes"
                };
                artifacts[key] = value.clone();
                Ok(serde_json::to_string_pretty(&value)?)
            }
            "episode_asset_ingest" => {
                let assets = artifacts["assets_visual_dna"].as_object().ok_or_else(|| {
                    AiError::InvalidState("缺少 assets_visual_dna，无法准备入库".into())
                })?;
                let mut items = vec![];
                for (key, kind, category) in [
                    ("characters", "character", "persona_visual"),
                    ("locations", "location", "background"),
                    ("props", "prop", "prop"),
                ] {
                    if let Some(values) = assets.get(key).and_then(Value::as_array) {
                        for value in values {
                            let name = value["name"].as_str().unwrap_or("").trim();
                            if !name.is_empty() {
                                items.push(json!({"kind":kind,"category":category,"name":name,"description":value["description"].as_str().unwrap_or(""),"base_prompt":value["stable_diffusion_tags"].as_str().unwrap_or(""),"visual_dna":value["visual_dna"]}));
                            }
                        }
                    }
                }
                let plan = json!({"items":items,"series_style":assets.get("series_style").cloned().unwrap_or(json!({}))});
                artifacts["asset_ingest_plan"] = plan.clone();
                Ok(serde_json::to_string_pretty(&plan)?)
            }
            "outline_generate" | "outline_optimize" | "script_generate" | "generate_script"
            | "script_optimize" => {
                let template = match action {
                    "generate_script" | "script_generate" => "script_generate_system",
                    "outline_generate" => "outline_generate_system",
                    "outline_optimize" => "outline_optimize_system",
                    _ => "script_optimize_system",
                };
                let system = self.prompts.get(template)?.prompt;
                let output = self.text_call(input, action, &system, &user, false).await?;
                artifacts[if action.starts_with("outline") {
                    "outline"
                } else {
                    "script"
                }] = json!(output);
                Ok(output)
            }
            "project_outline_generate" | "project_outline_optimize" => {
                let system = production_prompt(action)?;
                let requested = if action == "project_outline_generate" {
                    format!(
                        "{user}\n\n预计集数：{} 集。请生成完整项目级大纲。",
                        input.context["num_episodes"]
                            .as_u64()
                            .unwrap_or(12)
                            .clamp(1, 500)
                    )
                } else {
                    let current = self.outline_for_optimization(input)?;
                    format!("当前项目大纲：{current}\n\n优化指令：{user}")
                };
                let value = self
                    .json_call(input, action, &system, &requested, true)
                    .await?;
                let outline = if action == "project_outline_optimize" {
                    artifacts["changes_summary"] = value["changes_summary"].clone();
                    value["project_outline"].clone()
                } else {
                    value.clone()
                };
                if !outline.is_object() {
                    return Err(AiError::Protocol("项目大纲必须为JSON object".into()));
                }
                self.store
                    .put_context(&input.project_id, "project_outline", "v1", &outline)?;
                artifacts["project_outline"] = outline.clone();
                Ok(serde_json::to_string_pretty(&outline)?)
            }
            "split_scenes" | "split_shots" => {
                let (key, variable, default) = if action == "split_scenes" {
                    ("scenes", "max_scenes", 50)
                } else {
                    ("shots", "max_shots", 80)
                };
                let limit = input.context[variable]
                    .as_u64()
                    .unwrap_or(default)
                    .clamp(1, 1000) as usize;
                let system = self
                    .prompts
                    .render(&format!("{action}_system"), &json!({variable:limit}))?;
                let parsed = self.json_call(input, action, &system, &user, false).await?;
                let values: Vec<Value> = parsed
                    .as_array()
                    .expect("array validated")
                    .iter()
                    .take(limit)
                    .cloned()
                    .collect();
                for value in &values {
                    let field = if action == "split_scenes" {
                        "description"
                    } else {
                        "action_text"
                    };
                    if !value.is_object() || value[field].as_str().is_none() {
                        return Err(AiError::Protocol(format!("{action}输出缺少{field}文本")));
                    }
                }
                artifacts[key] = json!(values);
                Ok(serde_json::to_string_pretty(&values)?)
            }
            "visual_dna" | "visual_dna_ingest" => {
                let item = input.context["item_id"]
                    .as_i64()
                    .filter(|n| *n > 0)
                    .ok_or_else(|| AiError::InvalidInput("缺少有效资产 item_id".into()))?;
                let relative = input.context["asset_file_path"]
                    .as_str()
                    .ok_or_else(|| AiError::InvalidInput("缺少 asset_file_path".into()))?;
                let data_root = self.store.root().join("data").canonicalize()?;
                let path = data_root.join(relative).canonicalize()?;
                if !path.starts_with(&data_root) || !path.is_file() {
                    return Err(AiError::InvalidInput(
                        "素材文件必须位于应用 data 目录内".into(),
                    ));
                }
                let system = self.prompts.get("visual_dna_ingest_system")?.prompt;
                // Preserve the production text-only provider limitation. No image bytes are sent.
                let requested = format!(
                    "{user}\n文件路径：{relative}\n当前接口不能查看图片像素，仅能基于文件名和已提供文本推断。不要声称已经看见图片。请输出Visual DNA JSON。此输出需要人工核验。"
                );
                let dna = self
                    .json_call(input, "ingest", &system, &requested, true)
                    .await?;
                let version = input.context["version"].as_str().unwrap_or("v1");
                self.store.put_context(
                    &input.project_id,
                    &format!("visual_dna.asset_item_{item}"),
                    version,
                    &dna,
                )?;
                artifacts["visual_dna"] = dna.clone();
                artifacts["visual_dna_source"] = json!("filename_text_only");
                Ok(serde_json::to_string_pretty(&dna)?)
            }
            "workflow_script" => {
                let architect = format!(
                    "{}\n输出必须为JSON object，仅包含series_bible(object)、beat_sheet(array)。视觉优先；忽略内心独白。",
                    self.prompts.get("architect_system")?.prompt
                );
                let structure = self
                    .json_call(input, "architect", &architect, &user, true)
                    .await?;
                if !structure["series_bible"].is_object() || !structure["beat_sheet"].is_array() {
                    return Err(AiError::Protocol(
                        "Architect输出缺少series_bible或beat_sheet".into(),
                    ));
                }
                artifacts["series_bible"] = structure["series_bible"].clone();
                artifacts["beat_sheet"] = structure["beat_sheet"].clone();
                self.store.put_context(
                    &input.project_id,
                    "series_bible",
                    "v1",
                    &structure["series_bible"],
                )?;
                self.stage(input, "architect.parsed", structure.clone())?;
                let writer = format!(
                    "{}\n严格Fountain：INT./EXT.场景标题、全大写角色名；动作每段不超过3至4行；禁止心理动词，改为可见动作；每个气泡不超过30个汉字。只输出JSON object {{\"script_fountain\":\"剧本\"}}。",
                    self.prompts.get("writer_system")?.prompt
                );
                let written = self
                    .json_call(input, "writer", &writer, &structure.to_string(), true)
                    .await?;
                let mut script = written["script_fountain"]
                    .as_str()
                    .filter(|s| !s.trim().is_empty())
                    .ok_or_else(|| AiError::Protocol("Writer输出缺少script_fountain".into()))?
                    .to_owned();
                self.stage(input, "writer.parsed", written)?;
                let loops = input.context["qc_loops"].as_u64().unwrap_or(1).min(5);
                let qc_system = format!(
                    "{}\n只输出JSON object，字段issues(array，每项type/message/location可选)，revised_script_fountain(可选string)。检查世界观、角色、时代、物理和格式；如需修订提供完整Fountain剧本。",
                    self.prompts.get("qc_system")?.prompt
                );
                let mut report = json!({"issues":[]});
                for index in 1..=loops {
                    report=self.json_call(input,&format!("qc.{index}"),&qc_system,&json!({"series_bible":structure["series_bible"],"script_fountain":script}).to_string(),true).await?;
                    if !report["issues"].is_array() {
                        return Err(AiError::Protocol("QC输出缺少issues数组".into()));
                    }
                    if let Some(revision) = report["revised_script_fountain"]
                        .as_str()
                        .filter(|s| !s.trim().is_empty())
                    {
                        script = revision.to_owned();
                    }
                    self.stage(
                        input,
                        &format!("qc.parsed.{index}"),
                        json!({"qc_report":report,"script_fountain":script}),
                    )?;
                }
                artifacts["script_fountain"] = json!(script);
                artifacts["script"] = json!(script);
                artifacts["qc_report"] = report;
                if input.context["derived_split_scenes"].as_bool() == Some(true) {
                    let limit = input.context["max_scenes"]
                        .as_u64()
                        .unwrap_or(50)
                        .clamp(1, 1000) as usize;
                    let system = self
                        .prompts
                        .render("split_scenes_system", &json!({"max_scenes":limit}))?;
                    let values = self
                        .json_call(input, "split_scenes", &system, &script, false)
                        .await?;
                    let scenes: Vec<Value> = values
                        .as_array()
                        .expect("array validated")
                        .iter()
                        .take(limit)
                        .cloned()
                        .collect();
                    artifacts["derived"] = json!({"scenes":scenes});
                    self.stage(input, "split_scenes.parsed", artifacts["derived"].clone())?;
                }
                Ok(script)
            }
            "workflow_storyboard" => {
                let limit = input.context["max_shots"]
                    .as_u64()
                    .unwrap_or(80)
                    .clamp(1, 1000) as usize;
                let role = self.prompts.get("storyboard_system")?.prompt;
                let system = format!(
                    "{role}\n最多{limit}个镜头。只输出JSON array，每项含action_text(string)、title/dialogue(可选string)、shot_size=ELS|LS|MS|CU|ECU|INSERT、camera_angle=EYE|LOW|HIGH|DUTCH、lighting_style=SOFT|HARD|CHIAROSCURO|RIM|VOLUMETRIC。动作原子化，保持已确认角色视觉DNA。不要输出prompt。 "
                );
                let shots = self
                    .json_call(input, "storyboard", &system, &user, false)
                    .await?;
                let mut shots: Vec<Value> = shots
                    .as_array()
                    .expect("array validated")
                    .iter()
                    .take(limit)
                    .cloned()
                    .collect();
                for shot in &shots {
                    if !shot.is_object()
                        || shot["action_text"]
                            .as_str()
                            .is_none_or(|s| s.trim().is_empty())
                    {
                        return Err(AiError::Protocol("ShotSpec缺少动作描述".into()));
                    }
                    for (key, allowed) in [
                        ("shot_size", &["ELS", "LS", "MS", "CU", "ECU", "INSERT"][..]),
                        ("camera_angle", &["EYE", "LOW", "HIGH", "DUTCH"][..]),
                        (
                            "lighting_style",
                            &["SOFT", "HARD", "CHIAROSCURO", "RIM", "VOLUMETRIC"][..],
                        ),
                    ] {
                        if !allowed.contains(&shot[key].as_str().unwrap_or("")) {
                            return Err(AiError::Protocol(format!(
                                "ShotSpec的{key}不在受控词汇表中"
                            )));
                        }
                    }
                }
                self.stage(input, "storyboard.parsed", json!({"shots":shots}))?;
                if shots.is_empty() {
                    artifacts["shots"] = json!([]);
                    return Ok("[]".into());
                }
                let mj = input.context["prompt_style"] == "mj_v6";
                let role = self
                    .prompts
                    .get(if mj {
                        "prompt_translate_mj_system"
                    } else {
                        "prompt_translate_system"
                    })?
                    .prompt;
                let translate = format!(
                    "{role}\n输出严格JSON array，与输入shots等长；每项含非空prompt(string){}。",
                    if mj {
                        "，Midjourney v6语法，添加--v 6.0 --stylize 250及--ar宽高比"
                    } else {
                        "和非空negative_prompt(string)，使用英文逗号分隔tags"
                    }
                );
                let prompts=self.json_call(input,"prompt_translate",&translate,&json!({"shots":shots,"locked_visual_dna":input.context["locked_visual_dna"],"aspect_ratio":input.context["aspect_ratio"]}).to_string(),false).await?;
                let prompts = prompts.as_array().expect("array validated");
                if prompts.len() != shots.len() {
                    return Err(AiError::Protocol("prompt列表长度与shots不一致".into()));
                }
                for (shot, pair) in shots.iter_mut().zip(prompts) {
                    let prompt = pair["prompt"]
                        .as_str()
                        .filter(|s| !s.trim().is_empty())
                        .ok_or_else(|| AiError::Protocol("prompt不能为空".into()))?;
                    let negative = if mj {
                        ""
                    } else {
                        pair["negative_prompt"]
                            .as_str()
                            .filter(|s| !s.trim().is_empty())
                            .ok_or_else(|| AiError::Protocol("negative_prompt不能为空".into()))?
                    };
                    shot["prompt"] = json!(prompt);
                    shot["negative_prompt"] = json!(negative);
                }
                artifacts["shots"] = json!(shots);
                Ok(serde_json::to_string_pretty(&shots)?)
            }
            "memory_extract_changeset" => {
                let system = "从给定文本、已有创作产物和证据中抽取待人工审核的changeset.v0。只输出JSON object，字段schema_version='changeset.v0',project_id,episode_id,evidences,entities,snapshots,events,state_changes,time_constraints,time_blocks,conflicts,materialize。所有列表必须为array。不要捏造证据；evidence_ids只能引用提供的证据。未知字段、关系、时间和状态不得推断成事实。标注原文引述，保留出处和置信度。";
                let requested = format!("{user}\n\n当前产物：{artifacts}");
                let mut payload = self
                    .json_call(input, "changeset.extract", system, &requested, true)
                    .await?;
                if payload["schema_version"] != "changeset.v0" {
                    return Err(AiError::Protocol(
                        "记忆输出schema_version必须为changeset.v0".into(),
                    ));
                }
                let pid = input
                    .project_id
                    .parse::<i64>()
                    .map_err(|_| AiError::InvalidInput("记忆变更需要数字项目主键".into()))?;
                payload["project_id"] = json!(pid);
                payload["episode_id"] = json!(input.episode_id);
                for key in [
                    "evidences",
                    "entities",
                    "snapshots",
                    "events",
                    "state_changes",
                    "time_constraints",
                    "time_blocks",
                    "conflicts",
                ] {
                    if payload.get(key).is_none() {
                        payload[key] = json!([]);
                    }
                    if !payload[key].is_array() {
                        return Err(AiError::Protocol(format!("changeset字段{key}必须为数组")));
                    }
                }
                artifacts["changeset"] = payload.clone();
                Ok(serde_json::to_string_pretty(&payload)?)
            }
            _ => Err(AiError::UnsupportedAction(action.into())),
        }
    }
}
fn preview(text: &str, max: usize) -> String {
    text.chars().take(max).collect()
}
fn value_text(value: &Value) -> Option<String> {
    if value.is_null() {
        None
    } else {
        Some(
            value
                .as_str()
                .map(str::to_owned)
                .unwrap_or_else(|| value.to_string()),
        )
    }
}
fn production_prompt(key: &str) -> Result<String> {
    let prompts: Value = serde_json::from_str(include_str!("../assets/workflow_prompts.json"))?;
    prompts[key]
        .as_str()
        .map(str::to_owned)
        .ok_or_else(|| AiError::InvalidInput(format!("缺少工作流模板{key}")))
}
fn resume_schema_version() -> u32 {
    1
}
