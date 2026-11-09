use crate::editor::dark_editor;
use iced::widget::{
    Column, button, checkbox, column, container, pick_list, row, space, text, text_editor,
    text_input,
};
use iced::{Element, Length, Padding};
use serde_json::{Map, Number, Value};

use crate::presentation::field_label;

/// An editable, typed document. Display labels never become storage keys.
pub struct ContextForm {
    root: Node,
    next_id: u64,
}

#[derive(Debug, Clone)]
pub enum ContextFormMessage {
    Text(u64, text_editor::Action),
    Number(u64, String),
    Boolean(u64, bool),
    Toggle(u64),
    FieldName(u64, String),
    NewType(u64, ValueType),
    AddField(u64),
    AddItem(u64),
    Remove(u64),
    SetType(u64, ValueType),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ValueType {
    Text,
    Number,
    Boolean,
    Object,
    Array,
    Null,
}

impl ValueType {
    const ALL: [Self; 6] = [
        Self::Text,
        Self::Number,
        Self::Boolean,
        Self::Object,
        Self::Array,
        Self::Null,
    ];

    fn empty(self) -> Value {
        match self {
            Self::Text => Value::String(String::new()),
            Self::Number => Value::Number(0.into()),
            Self::Boolean => Value::Bool(false),
            Self::Object => Value::Object(Map::new()),
            Self::Array => Value::Array(Vec::new()),
            Self::Null => Value::Null,
        }
    }
}

impl std::fmt::Display for ValueType {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::Text => "文本",
            Self::Number => "数字",
            Self::Boolean => "开关",
            Self::Object => "分组",
            Self::Array => "列表",
            Self::Null => "未设置",
        })
    }
}

struct Node {
    id: u64,
    expanded: bool,
    content: Content,
    new_name: String,
    new_type: ValueType,
    error: Option<String>,
}

enum Content {
    Text(text_editor::Content),
    Number(String),
    Boolean(bool),
    Object(Vec<(String, Node)>),
    Array(Vec<Node>),
    Null,
}

impl Node {
    fn new(value: Value, next_id: &mut u64) -> Self {
        let id = *next_id;
        *next_id += 1;
        let content = match value {
            Value::String(value) => Content::Text(text_editor::Content::with_text(&value)),
            Value::Number(value) => Content::Number(value.to_string()),
            Value::Bool(value) => Content::Boolean(value),
            Value::Object(fields) => Content::Object(
                fields
                    .into_iter()
                    .map(|(key, value)| (key, Self::new(value, next_id)))
                    .collect(),
            ),
            Value::Array(items) => Content::Array(
                items
                    .into_iter()
                    .map(|value| Self::new(value, next_id))
                    .collect(),
            ),
            Value::Null => Content::Null,
        };
        Self {
            id,
            expanded: true,
            content,
            new_name: String::new(),
            new_type: ValueType::Text,
            error: None,
        }
    }

    fn find_mut(&mut self, id: u64) -> Option<&mut Self> {
        if self.id == id {
            return Some(self);
        }
        match &mut self.content {
            Content::Object(fields) => fields.iter_mut().find_map(|(_, node)| node.find_mut(id)),
            Content::Array(items) => items.iter_mut().find_map(|node| node.find_mut(id)),
            _ => None,
        }
    }

    fn remove(&mut self, id: u64) {
        match &mut self.content {
            Content::Object(fields) => {
                fields.retain(|(_, node)| node.id != id);
                for (_, node) in fields {
                    node.remove(id);
                }
            }
            Content::Array(items) => {
                items.retain(|node| node.id != id);
                for node in items {
                    node.remove(id);
                }
            }
            _ => {}
        }
    }

    fn value(&self, path: &str) -> Result<Value, String> {
        Ok(match &self.content {
            Content::Text(content) => Value::String(content.text()),
            Content::Number(draft) => Value::Number(
                parse_number(draft)
                    .map_err(|_| format!("“{path}”请填写有效数字，当前内容未保存。"))?,
            ),
            Content::Boolean(value) => Value::Bool(*value),
            Content::Null => Value::Null,
            Content::Object(fields) => {
                let mut object = Map::new();
                for (key, node) in fields {
                    object.insert(
                        key.clone(),
                        node.value(&format!("{path} › {}", field_label(key)))?,
                    );
                }
                Value::Object(object)
            }
            Content::Array(items) => Value::Array(
                items
                    .iter()
                    .enumerate()
                    .map(|(index, node)| node.value(&format!("{path} › 第 {} 项", index + 1)))
                    .collect::<Result<_, _>>()?,
            ),
        })
    }

    fn kind(&self) -> ValueType {
        match self.content {
            Content::Text(_) => ValueType::Text,
            Content::Number(_) => ValueType::Number,
            Content::Boolean(_) => ValueType::Boolean,
            Content::Object(_) => ValueType::Object,
            Content::Array(_) => ValueType::Array,
            Content::Null => ValueType::Null,
        }
    }
}

fn parse_number(draft: &str) -> Result<Number, serde_json::Error> {
    serde_json::from_str(draft.trim())
}

impl ContextForm {
    pub fn new(value: Value) -> Self {
        let mut next_id = 0;
        Self {
            root: Node::new(value, &mut next_id),
            next_id,
        }
    }

    pub fn load(&mut self, value: Value) {
        // IDs are not reused when documents change, so delayed UI messages are harmless.
        self.root = Node::new(value, &mut self.next_id);
    }

    pub fn value(&self) -> Result<Value, String> {
        self.root.value("项目上下文")
    }

    #[cfg(test)]
    pub fn field_id(&self, path: &[&str]) -> Option<u64> {
        let mut node = &self.root;
        for segment in path {
            node = match &node.content {
                Content::Object(fields) => &fields.iter().find(|(key, _)| key == segment)?.1,
                Content::Array(items) => items.get(segment.parse::<usize>().ok()?)?,
                _ => return None,
            };
        }
        Some(node.id)
    }

    pub fn update(&mut self, message: ContextFormMessage) {
        if let ContextFormMessage::Remove(id) = message {
            self.root.remove(id);
            return;
        }
        let id = match &message {
            ContextFormMessage::Text(id, _)
            | ContextFormMessage::Number(id, _)
            | ContextFormMessage::Boolean(id, _)
            | ContextFormMessage::FieldName(id, _)
            | ContextFormMessage::NewType(id, _)
            | ContextFormMessage::SetType(id, _)
            | ContextFormMessage::Toggle(id)
            | ContextFormMessage::AddField(id)
            | ContextFormMessage::AddItem(id) => *id,
            ContextFormMessage::Remove(_) => unreachable!(),
        };
        let Some(node) = self.root.find_mut(id) else {
            return;
        };
        match message {
            ContextFormMessage::Text(_, action) => {
                if let Content::Text(content) = &mut node.content {
                    content.perform(action);
                }
            }
            ContextFormMessage::Number(_, draft) => {
                if let Content::Number(value) = &mut node.content {
                    *value = draft;
                }
            }
            ContextFormMessage::Boolean(_, value) => {
                if let Content::Boolean(current) = &mut node.content {
                    *current = value;
                }
            }
            ContextFormMessage::Toggle(_) => node.expanded = !node.expanded,
            ContextFormMessage::FieldName(_, name) => {
                node.new_name = name;
                node.error = None;
            }
            ContextFormMessage::NewType(_, kind) => node.new_type = kind,
            ContextFormMessage::AddField(_) => {
                if let Content::Object(fields) = &mut node.content {
                    let name = node.new_name.trim();
                    if name.is_empty() {
                        node.error = Some("请填写新字段的名称。".into());
                    } else if fields.iter().any(|(key, _)| key == name) {
                        node.error = Some("已存在同名字段，请使用其他名称。".into());
                    } else {
                        fields.push((
                            name.to_owned(),
                            Node::new(node.new_type.empty(), &mut self.next_id),
                        ));
                        node.new_name.clear();
                        node.error = None;
                    }
                }
            }
            ContextFormMessage::AddItem(_) => {
                if let Content::Array(items) = &mut node.content {
                    items.push(Node::new(node.new_type.empty(), &mut self.next_id));
                }
            }
            ContextFormMessage::SetType(_, kind) => {
                if node.kind() != kind {
                    // Changing a null value is an explicit choice. Other existing values keep their type.
                    if matches!(node.content, Content::Null) {
                        let replacement = Node::new(kind.empty(), &mut self.next_id);
                        node.content = replacement.content;
                    }
                }
            }
            ContextFormMessage::Remove(_) => unreachable!(),
        }
    }

    pub fn view(&self) -> Element<'_, ContextFormMessage> {
        let mut fields = Column::new().spacing(10).width(Length::Fill);
        append_node(&self.root, "内容".into(), 0, false, &mut fields);
        fields.into()
    }
}

fn indented<'a>(
    content: impl Into<Element<'a, ContextFormMessage>>,
    depth: usize,
) -> Element<'a, ContextFormMessage> {
    container(content)
        .width(Length::Fill)
        .padding(Padding {
            top: 0.0,
            right: 2.0,
            bottom: 0.0,
            left: (depth.min(6) * 16) as f32,
        })
        .into()
}

fn append_node<'a>(
    node: &'a Node,
    label: String,
    depth: usize,
    removable: bool,
    fields: &mut Column<'a, ContextFormMessage>,
) {
    let id = node.id;
    let grouped = matches!(node.content, Content::Object(_) | Content::Array(_));
    let mut heading = row![].spacing(8).align_y(iced::Alignment::Center);
    if grouped {
        heading = heading.push(
            button(if node.expanded { "收起" } else { "展开" })
                .on_press(ContextFormMessage::Toggle(id)),
        );
    }
    heading = heading
        .push(text(label).size(if grouped { 16 } else { 14 }))
        .push(space::horizontal())
        .push(text(node.kind().to_string()).size(12));
    if removable {
        heading = heading.push(
            button("删除")
                .on_press(ContextFormMessage::Remove(id))
                .style(button::text),
        );
    }
    *fields = std::mem::replace(fields, Column::new()).push(indented(heading, depth));
    if grouped && !node.expanded {
        return;
    }

    match &node.content {
        Content::Text(content) => {
            let text = content.text();
            let estimated_lines = text
                .lines()
                .map(|line| 1 + line.chars().count() / 52)
                .sum::<usize>()
                .clamp(2, 120);
            let height = (estimated_lines * 22 + 18) as f32;
            *fields = std::mem::replace(fields, Column::new()).push(indented(
                dark_editor(content, height, move |action| {
                    ContextFormMessage::Text(id, action)
                }),
                depth,
            ));
        }
        Content::Number(draft) => {
            let mut number = column![
                text_input("填写数字", draft)
                    .on_input(move |value| ContextFormMessage::Number(id, value))
            ]
            .spacing(4);
            if parse_number(draft).is_err() {
                number = number.push(
                    text("请填写有效数字，修正后才能保存。")
                        .size(12)
                        .style(text::danger),
                );
            }
            *fields = std::mem::replace(fields, Column::new()).push(indented(number, depth));
        }
        Content::Boolean(value) => {
            *fields = std::mem::replace(fields, Column::new()).push(indented(
                checkbox(*value)
                    .label(if *value { "已开启" } else { "已关闭" })
                    .on_toggle(move |value| ContextFormMessage::Boolean(id, value)),
                depth,
            ));
        }
        Content::Null => {
            *fields = std::mem::replace(fields, Column::new()).push(indented(
                row![
                    text("尚未设置内容").size(13),
                    pick_list(ValueType::ALL, Some(ValueType::Null), move |kind| {
                        ContextFormMessage::SetType(id, kind)
                    })
                ]
                .spacing(12)
                .align_y(iced::Alignment::Center),
                depth,
            ));
        }
        Content::Object(children) => {
            if children.is_empty() {
                *fields = std::mem::replace(fields, Column::new()).push(indented(
                    text("暂无字段，可在下方添加。").size(13),
                    depth + 1,
                ));
            }
            for (key, child) in children {
                append_node(child, field_label(key), depth + 1, true, fields);
            }
            let mut add = column![
                row![
                    text_input("新字段名称", &node.new_name)
                        .on_input(move |name| ContextFormMessage::FieldName(id, name)),
                    pick_list(ValueType::ALL, Some(node.new_type), move |kind| {
                        ContextFormMessage::NewType(id, kind)
                    }),
                    button("添加字段").on_press(ContextFormMessage::AddField(id))
                ]
                .spacing(8)
            ]
            .spacing(4);
            if let Some(error) = &node.error {
                add = add.push(text(error).size(12).style(text::danger));
            }
            *fields = std::mem::replace(fields, Column::new()).push(indented(add, depth + 1));
        }
        Content::Array(children) => {
            if children.is_empty() {
                *fields = std::mem::replace(fields, Column::new()).push(indented(
                    text("暂无条目，可在下方添加。").size(13),
                    depth + 1,
                ));
            }
            for (index, child) in children.iter().enumerate() {
                append_node(
                    child,
                    format!("第 {} 项", index + 1),
                    depth + 1,
                    true,
                    fields,
                );
            }
            *fields = std::mem::replace(fields, Column::new()).push(indented(
                row![
                    pick_list(ValueType::ALL, Some(node.new_type), move |kind| {
                        ContextFormMessage::NewType(id, kind)
                    }),
                    button("添加条目").on_press(ContextFormMessage::AddItem(id))
                ]
                .spacing(8),
                depth + 1,
            ));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn child(node: &Node, key: &str) -> u64 {
        let Content::Object(fields) = &node.content else {
            panic!("expected object");
        };
        fields.iter().find(|(name, _)| name == key).unwrap().1.id
    }

    #[test]
    fn unchanged_document_preserves_unknown_fields_and_all_value_types() {
        let value = json!({"unknown":{"empty_object":{},"empty_array":[],"nothing":null,"enabled":true,"quantity":-12.5,"large":18446744073709551615u64},"lines":"第一行\n第二行\n","windows_lines":"保留\r\n换行\r\n","items":["",false,0,null,{},[]]});
        let form = ContextForm::new(value.clone());
        assert_eq!(form.value().unwrap(), value);
    }

    #[test]
    fn editing_nested_text_keeps_other_fields_and_storage_keys() {
        let mut form =
            ContextForm::new(json!({"story":{"title":"原题","extra":21},"unknown": [false,null]}));
        let story = form.root.find_mut(child(&form.root, "story")).unwrap();
        let title_id = child(story, "title");
        form.update(ContextFormMessage::Text(
            title_id,
            text_editor::Action::SelectAll,
        ));
        form.update(ContextFormMessage::Text(
            title_id,
            text_editor::Action::Edit(text_editor::Edit::Paste(std::sync::Arc::new(
                "新标题\n第二行".into(),
            ))),
        ));
        assert_eq!(
            form.value().unwrap(),
            json!({"story":{"title":"新标题\n第二行","extra":21},"unknown":[false,null]})
        );
    }

    #[test]
    fn invalid_number_blocks_save_and_can_be_corrected() {
        let mut form = ContextForm::new(json!({"episode_count":12,"kept":"保留"}));
        let id = form.field_id(&["episode_count"]).unwrap();
        for invalid in ["", "NaN", "1.2.3", "null", "\"42\""] {
            form.update(ContextFormMessage::Number(id, invalid.into()));
            assert!(form.value().unwrap_err().contains("有效数字"));
        }
        form.update(ContextFormMessage::Number(id, "24.5".into()));
        assert_eq!(
            form.value().unwrap(),
            json!({"episode_count":24.5,"kept":"保留"})
        );
    }

    #[test]
    fn add_and_remove_fields_and_items_preserve_empty_structures() {
        let mut form = ContextForm::new(json!({"object":{},"items":[]}));
        let object_id = child(&form.root, "object");
        form.update(ContextFormMessage::FieldName(
            object_id,
            "new_unknown_key".into(),
        ));
        form.update(ContextFormMessage::NewType(object_id, ValueType::Boolean));
        form.update(ContextFormMessage::AddField(object_id));
        let field_id = child(form.root.find_mut(object_id).unwrap(), "new_unknown_key");
        form.update(ContextFormMessage::Boolean(field_id, true));
        let array_id = child(&form.root, "items");
        form.update(ContextFormMessage::NewType(array_id, ValueType::Object));
        form.update(ContextFormMessage::AddItem(array_id));
        assert_eq!(
            form.value().unwrap(),
            json!({"object":{"new_unknown_key":true},"items":[{}]})
        );
        form.update(ContextFormMessage::Remove(field_id));
        assert_eq!(form.value().unwrap(), json!({"object":{},"items":[{}]}));
    }

    #[test]
    fn stale_messages_cannot_edit_shifted_items_or_reloaded_document() {
        let mut form = ContextForm::new(json!([1, 2]));
        let Content::Array(items) = &form.root.content else {
            unreachable!()
        };
        let removed = items[0].id;
        let retained = items[1].id;
        form.update(ContextFormMessage::Remove(removed));
        form.update(ContextFormMessage::Number(removed, "999".into()));
        assert_eq!(form.value().unwrap(), json!([2]));
        form.load(json!([3]));
        form.update(ContextFormMessage::Number(retained, "999".into()));
        assert_eq!(form.value().unwrap(), json!([3]));
    }

    #[test]
    fn duplicate_field_name_does_not_replace_existing_value() {
        let mut form = ContextForm::new(json!({"title":"保留"}));
        let id = form.root.id;
        form.update(ContextFormMessage::FieldName(id, "title".into()));
        form.update(ContextFormMessage::AddField(id));
        assert!(form.root.error.is_some());
        assert_eq!(form.value().unwrap(), json!({"title":"保留"}));
    }
}
