//! Human-readable views of structured results. The source values stay unchanged.

use serde_json::Value;

const MAX_EMBEDDED_JSON_BYTES: usize = 1024 * 1024;
const MAX_EMBEDDED_JSON_LAYERS: usize = 4;

/// Render structured data as labelled, indented text without JSON punctuation.
pub fn readable(value: &Value) -> String {
    let mut lines = Vec::new();
    render(value, 0, 0, &mut lines);
    lines.join("\n")
}

/// Recognise a complete JSON object or array; leave prose and scripts unchanged.
pub fn readable_text(text: &str) -> String {
    structured_text(text).map_or_else(|| text.to_owned(), |value| readable(&value))
}

fn structured_text(text: &str) -> Option<Value> {
    let trimmed = text.trim();
    let trimmed = if let Some(fenced) = trimmed.strip_prefix("```") {
        let (language, content) = fenced.split_once('\n')?;
        if !language.trim().is_empty() && !language.trim().eq_ignore_ascii_case("json") {
            return None;
        }
        content.strip_suffix("```")?.trim()
    } else {
        trimmed
    };
    if trimmed.len() > MAX_EMBEDDED_JSON_BYTES
        || !(trimmed.starts_with('{') && trimmed.ends_with('}')
            || trimmed.starts_with('[') && trimmed.ends_with(']'))
    {
        return None;
    }
    serde_json::from_str::<Value>(trimmed)
        .ok()
        .filter(|value| value.is_object() || value.is_array())
}

fn push(lines: &mut Vec<String>, depth: usize, text: &str) {
    let indent = "  ".repeat(depth);
    // split retains blank lines, including a trailing newline in a script.
    lines.extend(text.split('\n').map(|line| format!("{indent}{line}")));
}

fn render(value: &Value, depth: usize, embedded_layers: usize, lines: &mut Vec<String>) {
    match value {
        Value::Object(fields) if !fields.is_empty() => {
            for (key, value) in fields {
                render_field(&field_label(key), value, depth, embedded_layers, lines);
            }
        }
        Value::Array(items) if !items.is_empty() => {
            for (index, value) in items.iter().enumerate() {
                render_field(
                    &format!("{}.", index + 1),
                    value,
                    depth,
                    embedded_layers,
                    lines,
                );
            }
        }
        Value::String(text) => {
            if embedded_layers < MAX_EMBEDDED_JSON_LAYERS
                && let Some(value) = structured_text(text)
            {
                render(&value, depth, embedded_layers + 1, lines);
            } else {
                push(
                    lines,
                    depth,
                    if text.is_empty() {
                        "（空文本）"
                    } else {
                        text
                    },
                );
            }
        }
        Value::Object(_) => push(lines, depth, "（无字段）"),
        Value::Array(_) => push(lines, depth, "（空列表）"),
        Value::Null => push(lines, depth, "未设置"),
        Value::Bool(value) => push(lines, depth, if *value { "是" } else { "否" }),
        Value::Number(value) => push(lines, depth, &value.to_string()),
    }
}

fn render_field(
    label: &str,
    value: &Value,
    depth: usize,
    embedded_layers: usize,
    lines: &mut Vec<String>,
) {
    let mut content = Vec::new();
    render(value, 0, embedded_layers, &mut content);
    let is_structure = value.is_object() && !value.as_object().unwrap().is_empty()
        || value.is_array() && !value.as_array().unwrap().is_empty()
        || value.as_str().is_some_and(|text| {
            embedded_layers < MAX_EMBEDDED_JSON_LAYERS && structured_text(text).is_some()
        });
    let separator = if label.ends_with('.') { "" } else { "：" };
    if content.len() == 1 && !is_structure {
        let spacing = if separator.is_empty() { " " } else { "" };
        push(
            lines,
            depth,
            &format!("{label}{separator}{spacing}{}", content[0]),
        );
    } else {
        push(lines, depth, &format!("{label}{separator}"));
        for line in content {
            push(lines, depth + 1, &line);
        }
    }
}

/// Labels cover the creative workflow and memory records; unknown fields remain visible.
pub fn field_label(key: &str) -> String {
    let label = match key {
        "id" | "uuid" => "标识",
        "name" | "canonical_name" => "名称",
        "title" => "标题",
        "description" => "描述",
        "summary" => "摘要",
        "content" | "contents" | "text" => "内容",
        "kind" | "type" => "类型",
        "category" => "分类",
        "status" | "exec_status" | "workflow_status" => "状态",
        "order" | "sequence_number" => "顺序",
        "project" => "项目",
        "project_id" => "项目标识",
        "episode" => "分集",
        "episode_id" => "分集标识",
        "scene" => "场景",
        "scene_id" => "场景标识",
        "shot" => "镜头",
        "shot_id" => "镜头标识",
        "character" => "角色",
        "character_id" => "角色标识",
        "characters" => "角色",
        "scenes" => "场景",
        "shots" => "镜头",
        "episodes" => "分集",
        "assets" | "items" => "素材",
        "action_text" | "action" => "动作",
        "dialogue" => "对白",
        "camera_angle" => "机位角度",
        "lighting_style" => "光线",
        "shot_size" => "景别",
        "camera_move" => "运镜",
        "duration_ms" => "时长",
        "speaker" => "说话者",
        "quote" => "原文",
        "script" | "script_fountain" | "draft" => "剧本",
        "script_locked" => "剧本已锁定",
        "series_bible" => "故事设定",
        "project_outline" => "全剧大纲",
        "episode_outline" | "outline" => "分集大纲",
        "episode_script" => "分集剧本",
        "beat_sheet" | "beats" => "情节节拍",
        "beat_type" => "节拍类型",
        "beat_index" => "节拍序号",
        "beat_count" => "节拍数量",
        "emotional_charge" => "情绪强度",
        "visual_focus" => "视觉重点",
        "visual_dna" | "locked_visual_dna" => "视觉设定",
        "visual_dna_ref" => "视觉设定引用",
        "series_style" | "style" => "整体风格",
        "stable_diffusion_tags" => "图像提示词",
        "prompt" | "base_prompt" => "提示词",
        "negative_prompt" => "反向提示词",
        "prompts" => "提示词列表",
        "variables" => "变量",
        "templates" => "模板",
        "aspect_ratio" => "画面比例",
        "camera" | "camera_movement" => "摄影与运镜",
        "shot_type" => "景别",
        "duration" | "duration_seconds" => "时长（秒）",
        "location" => "位置",
        "setting" => "环境设定",
        "time" | "story_time" => "故事时间",
        "story_order" => "故事顺序",
        "story_time_key" => "故事时间排序",
        "participants" => "参与角色",
        "event" => "事件",
        "events" => "事件列表",
        "event_id" => "事件标识",
        "event_type" => "事件类型",
        "nodes" => "节点",
        "graph_data" => "事件关系",
        "state_changes" => "状态变化",
        "state_change_id" => "状态变化标识",
        "before_snapshot_id" => "变化前快照",
        "after_snapshot_id" => "变化后快照",
        "entities" => "实体",
        "entity" => "实体",
        "entity_id" => "实体标识",
        "entity_type" => "实体类型",
        "target_entity_id" => "目标实体标识",
        "target_type" => "目标类型",
        "target_id" => "目标标识",
        "snapshots" => "状态快照",
        "snapshot_id" => "快照标识",
        "fields" => "属性",
        "aliases" => "别名",
        "alias" => "别名",
        "facts" => "事实",
        "constraints" | "constraint" => "约束",
        "constraint_ids" => "约束标识",
        "term" => "术语",
        "definition" => "定义",
        "key" => "名称",
        "value" => "值",
        "tags" => "标签",
        "evidence_ids" => "证据标识",
        "evidences" | "evidence" => "证据",
        "evidence_id" => "证据标识",
        "created_from_evidence_id" => "来源证据标识",
        "span" => "原文位置",
        "paragraph_index" => "段落序号",
        "source_kind" => "来源类型",
        "source_ref" => "来源引用",
        "confidence" => "可信度",
        "retrieved_memory" => "检索到的记忆",
        "records" => "记忆记录",
        "record_type" => "记忆类型",
        "payload" | "payload_json" => "详细内容",
        "patch" | "patch_json" => "属性变更",
        "changeset_id" => "变更集标识",
        "changesets" => "变更集",
        "review_status" => "审核状态",
        "review_log" | "review_log_json" => "审核记录",
        "reviewer" => "审核者",
        "note" | "resolution_note" => "备注",
        "conflicts" => "冲突",
        "conflict_id" => "冲突标识",
        "conflict_type" => "冲突类型",
        "old_claim" => "已有信息",
        "new_claim" => "新增信息",
        "suggested_actions" => "建议操作",
        "resolved_by" => "处理者",
        "relation" => "关系",
        "from_event_id" => "起始事件标识",
        "to_event_id" => "结束事件标识",
        "event_ids" => "事件标识",
        "anchor_id" => "时间锚点标识",
        "parent_block_id" => "上级时间区块标识",
        "interval" => "时间区间",
        "valid_from_story_order" => "生效故事顺序",
        "valid_to_story_order" => "失效故事顺序",
        "valid_from_story_time_key" => "生效故事时间",
        "valid_to_story_time_key" => "失效故事时间",
        "run_id" | "last_exec_run_id" => "运行标识",
        "workflow" | "action_key" | "current_action_key" | "final_action_key" => "创作任务",
        "stage" => "阶段",
        "plan" => "计划",
        "steps" => "步骤",
        "step_index" | "current_step_index" => "当前步骤",
        "why" | "reason" => "原因",
        "input" | "input_text" | "input_preview" => "输入",
        "output" | "output_text" | "output_preview" => "输出",
        "assistant_message" => "创作回复",
        "request" => "请求",
        "response" => "结果",
        "meta" | "meta_data" => "相关信息",
        "artifacts" | "exec_artifacts" => "创作产物",
        "provided" | "context" => "上下文",
        "derived" => "派生内容",
        "error" => "错误",
        "message" => "说明",
        "level" => "级别",
        "issues" => "问题列表",
        "issue_type" => "问题类型",
        "qc_report" => "质量检查",
        "revision" => "修订",
        "has_revision" => "已完成修订",
        "waiting_for" => "等待处理",
        "resume_state" => "待恢复状态",
        "created_at" | "created_at_ms" => "创建时间",
        "updated_at" | "updated_at_ms" => "更新时间",
        "at_ms" | "ts_ms" => "记录时间",
        "ms" => "耗时（毫秒）",
        "file_path" => "文件路径",
        "file_type" => "文件类型",
        "video_path" => "视频路径",
        "is_favorite" => "已收藏",
        "model" => "模型",
        "provider" => "模型服务",
        "temperature" => "温度",
        "max_tokens" => "最大输出长度",
        "timeout_seconds" => "超时（秒）",
        "score" => "匹配度",
        "priority" => "优先级",
        _ => return separated_words(key),
    };
    label.to_owned()
}

fn separated_words(key: &str) -> String {
    let chars: Vec<char> = key.chars().collect();
    let mut label = String::new();
    for (index, &current) in chars.iter().enumerate() {
        if matches!(current, '_' | '-') {
            if !label.is_empty() && !label.ends_with(' ') {
                label.push(' ');
            }
            continue;
        }
        if current.is_uppercase()
            && index > 0
            && !label.ends_with(' ')
            && (chars[index - 1].is_lowercase()
                || chars[index - 1].is_numeric()
                || chars.get(index + 1).is_some_and(|next| next.is_lowercase())
                    && chars[index - 1].is_uppercase())
        {
            label.push(' ');
        }
        label.push(current);
    }
    let label = label.trim();
    if label.is_empty() {
        key.to_owned()
    } else {
        label.to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn nested_creative_results_have_labels_numbering_and_indentation() {
        let text = readable(
            &json!({"shots":[{"title":"开场","dialogue":"你好。\n再见。","characters":["小晴","阿星"]}]}),
        );
        assert!(text.starts_with("镜头：\n  1.\n"));
        assert!(text.contains("    标题：开场"));
        assert!(text.contains("    对白：\n      你好。\n      再见。"));
        assert!(text.contains("    角色：\n      1. 小晴\n      2. 阿星"));
        assert!(!text.contains(['{', '}', '[', ']', '"']));
        assert!(!text.contains("\\n"));
    }

    #[test]
    fn serialized_json_inside_strings_is_expanded_without_losing_script_lines() {
        let value = json!({"payload_json":"{\"shots\":[{\"action_text\":\"推门。\\n  停顿。\\n\"}],\"approved\":true}"});
        let text = readable(&value);
        assert!(text.contains("详细内容：\n  approved：是\n  镜头：\n    1.\n      动作：\n        推门。\n          停顿。\n        "));
        assert!(!text.contains("\\\""));
        assert!(!text.contains("\\n"));
    }

    #[test]
    fn ordinary_prose_scripts_and_scalar_json_text_are_unchanged() {
        for text in [
            "内景. 客厅 - 夜\n\n  小晴\n  [轻声] 你好。\n",
            "这里出现 {场景} 和 [旁白]，原样显示。",
            "[场景一]\n小晴走进房间。",
            "{\"title\":\"片段\"} 后面还有正文",
            "\"一段对白\"",
            "true",
            "42",
            "",
        ] {
            assert_eq!(readable_text(text), text);
        }
        assert_eq!(readable_text(" \n {\"title\":\"开场\"} \n"), "标题：开场");
        assert_eq!(readable_text("[\"小晴\",\"阿星\"]"), "1. 小晴\n2. 阿星");
    }

    #[test]
    fn unknown_fields_and_empty_values_remain_visible() {
        let text = readable(
            &json!({"customStoryFlag":false,"new_field":23,"emptyList":[],"emptyObject":{},"emptyText":"","optional":null}),
        );
        assert!(text.contains("custom Story Flag：否"));
        assert!(text.contains("new field：23"));
        assert!(text.contains("empty List：（空列表）"));
        assert!(text.contains("empty Object：（无字段）"));
        assert!(text.contains("empty Text：（空文本）"));
        assert!(text.contains("optional：未设置"));
        assert_eq!(field_label("APIResponseCode"), "API Response Code");
        assert_eq!(field_label("未知字段"), "未知字段");
    }

    #[test]
    fn natural_quotes_and_brackets_inside_text_are_not_stripped() {
        let script = "她说：\"再等一会儿。\"\n[风声] {门牌：7}";
        assert_eq!(readable(&json!(script)), script);
        assert_eq!(
            readable(&json!({"text":script})),
            format!("内容：\n  她说：\"再等一会儿。\"\n  [风声] {{门牌：7}}")
        );
    }

    #[test]
    fn complete_json_code_fences_are_expanded_but_explanations_and_other_code_are_kept() {
        let stage = readable(
            &json!({"response":"```json\n{\"shots\":[{\"title\":\"晨光\"}]}\n```","status":"paused"}),
        );
        assert_eq!(
            stage,
            "结果：\n  镜头：\n    1.\n      标题：晨光\n状态：paused"
        );
        assert_eq!(readable_text("```\n[1,2]\n```"), "1. 1\n2. 2");
        for text in [
            "说明：\n```json\n{\"title\":\"开场\"}\n```",
            "```json\n{\"title\":\"开场\"}\n```\n还有说明。",
            "```rust\n[1,2]\n```",
            "```json\n{\"title\":\"不完整\"\n```",
        ] {
            assert_eq!(readable_text(text), text);
        }
    }
}
