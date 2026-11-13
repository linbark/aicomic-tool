//! Native Iced implementation of the approved v2 two-scene atelier.
use crate::app::{Dialog, Message, Studio};
use crate::atelier::{self, Field, Kind, ModelField, RunState};
use crate::data;
use crate::options::Screen;
use crate::style::game;
use iced::widget::{
    Column, Row, Space, button, checkbox, column, container, image, pick_list, row, scrollable,
    stack, text, text_editor, text_input,
};
use iced::{Alignment, Element, Fill, Font, Length};
use serde_json::{Value, json};
use std::sync::OnceLock;

fn msg(m: atelier::Message) -> Message {
    Message::Atelier(m)
}
fn small<'a>(s: impl Into<String>) -> iced::widget::Text<'a> {
    text(s.into()).size(12).color(game::MUTED)
}
fn paper_small<'a>(s: impl Into<String>) -> iced::widget::Text<'a> {
    text(s.into()).size(12).color(game::PAPER_MUTED)
}
fn title<'a>(s: impl Into<String>, size: u32) -> iced::widget::Text<'a> {
    text(s.into()).font(Font::with_name("Songti SC")).size(size)
}
fn action<'a>(
    s: impl Into<String>,
    m: Option<Message>,
    primary: bool,
) -> iced::widget::Button<'a, Message> {
    button(text(s.into()).size(13))
        .padding([7, 12])
        .height(34)
        .on_press_maybe(m)
        .style(if primary { game::primary } else { game::button })
}
fn paper_action<'a>(s: impl Into<String>, m: Option<Message>) -> iced::widget::Button<'a, Message> {
    action(s, m, false).style(game::paper_button)
}
fn trim(s: &str, n: usize) -> String {
    if s.chars().count() > n {
        format!("{}…", s.chars().take(n).collect::<String>())
    } else {
        s.into()
    }
}
fn field<'a>(
    label: &str,
    value: &'a str,
    placeholder: &str,
    on_input: impl Fn(String) -> Message + 'a,
) -> Element<'a, Message> {
    column![
        paper_small(label),
        text_input(placeholder, value)
            .on_input(on_input)
            .style(game::input)
            .padding(8)
            .size(13)
    ]
    .spacing(5)
    .into()
}
fn editor<'a>(
    content: &'a text_editor::Content,
    height: f32,
    on_action: impl Fn(text_editor::Action) -> Message + 'a,
) -> Element<'a, Message> {
    text_editor(content)
        .on_action(on_action)
        .height(height)
        .padding(12)
        .size(14)
        .style(game::editor)
        .into()
}
fn bg() -> image::Handle {
    static IMAGE: OnceLock<image::Handle> = OnceLock::new();
    IMAGE
        .get_or_init(|| {
            let pixels = ::image::load_from_memory(include_bytes!("../assets/workshop-bg.jpg"))
                .expect("bundled workshop image is valid")
                .to_rgba8();
            image::Handle::from_rgba(pixels.width(), pixels.height(), pixels.into_raw())
        })
        .clone()
}

impl Studio {
    pub(crate) fn atelier_page(&self) -> Element<'_, Message> {
        let treasury = self.screen == Screen::Treasury;
        let a = &self.atelier;
        let editable = self.can_write() && a.integrity_error.is_none();
        let mut top = row![
            action("导航", Some(Message::ToggleSidebar), false),
            column![
                title(
                    if treasury {
                        "藏经阁"
                    } else {
                        "炼丹工坊"
                    },
                    30
                ),
                small(if treasury {
                    "手法谱 / 配方卷 / 丹方录 · 修订后保存不可变版本"
                } else {
                    "取用保存版本 · 装配依赖 · 运行本地模型"
                })
            ]
            .spacing(3),
            Space::new().width(Fill)
        ]
        .spacing(12)
        .align_y(Alignment::Center);
        if treasury {
            top = top
                .push(action(
                    "送往工坊",
                    a.draft()
                        .filter(|d| !d.dirty())
                        .map(|_| msg(atelier::Message::Send)),
                    false,
                ))
                .push(action(
                    if a.save.is_some() {
                        "正在保存…"
                    } else {
                        "保存新版本 · ⌘S"
                    },
                    (editable && a.draft().is_some()).then_some(msg(atelier::Message::Save)),
                    true,
                ));
        } else {
            top = top
                .push(action(
                    "保存丹方 · ⌘S",
                    editable.then_some(msg(atelier::Message::SavePlan)),
                    false,
                ))
                .push(action(
                    if a.running() {
                        "请求停止"
                    } else {
                        "开炉"
                    },
                    if a.running() {
                        Some(msg(atelier::Message::Stop))
                    } else {
                        editable.then_some(msg(atelier::Message::Run))
                    },
                    true,
                ));
        }
        let mut feedback = column![].spacing(5);
        if let Some(error) = &a.integrity_error {
            feedback=feedback.push(text(format!("版本库完整性错误：{error}。已停用保存、取用和开炉，旧素材 / 丹药 / 偏好仍可浏览；建立副本不代表篡改已修复。")).size(13).color(game::ERROR));
        } else if !self.writable {
            feedback = feedback.push(
                row![
                    small(
                        if self.data.needs_migration || (!a.available && self.data.schema.is_some())
                        {
                            "旧库只读浏览 · 建立副本才升级，不改原库"
                        } else {
                            "原数据库只读 · 保存和开炉需要工作副本 / 新库"
                        }
                    )
                    .width(Fill),
                    action(
                        "建立工作副本…",
                        (self.busy.is_none()
                            && !a.running()
                            && a.save.is_none()
                            && !self.config.db.as_os_str().is_empty())
                        .then_some(Message::Dialog(Dialog::Copy)),
                        false
                    ),
                    action(
                        "建立新库…",
                        (self.busy.is_none() && !a.running() && a.save.is_none())
                            .then_some(Message::Dialog(Dialog::New)),
                        false
                    )
                ]
                .spacing(8)
                .align_y(Alignment::Center),
            );
        }
        if let Some(busy) = &self.busy {
            feedback = feedback.push(small(busy.clone()));
        } else {
            feedback = feedback.push(text(&a.note).size(12).color(if a.error {
                game::ERROR
            } else {
                game::MUTED
            }));
        }
        if self.error {
            feedback = feedback.push(text(&self.status).size(12).color(game::ERROR));
        }
        let body = column![
            container(top).padding([18, 22]),
            container(feedback).padding([0, 22]),
            Space::new().height(12),
            if treasury {
                self.treasury_body()
            } else {
                self.workshop_body()
            }
        ]
        .height(Fill)
        .width(Fill);
        let mut layout = Row::new().height(Fill);
        if self.sidebar {
            layout = layout.push(self.atelier_rail());
        }
        container(layout.push(body))
            .style(game::root)
            .height(Fill)
            .width(Fill)
            .into()
    }
    fn atelier_rail(&self) -> Element<'_, Message> {
        let mut nav = column![
            title("丹室", 25).color(game::COPPER_LIGHT),
            Space::new().height(20)
        ]
        .spacing(8);
        for (screen, label) in [
            (Screen::Workshop, "工坊  ⌘1"),
            (Screen::Treasury, "藏经阁  ⌘2"),
        ] {
            nav = nav.push(
                action(label, Some(Message::Navigate(screen)), false)
                    .style(if self.screen == screen {
                        game::selected
                    } else {
                        game::button
                    })
                    .width(Fill),
            );
        }
        nav = nav.push(Space::new().height(18)).push(small("资 料"));
        for (screen, label) in [
            (Screen::Library, "素材库"),
            (Screen::Brew, "像素炼制"),
            (Screen::Pills, "丹药"),
            (Screen::Preferences, "我的偏好"),
        ] {
            nav = nav.push(action(label, Some(Message::Navigate(screen)), false).width(Fill));
        }
        let safe = self.busy.is_none() && !self.atelier.running() && self.atelier.save.is_none();
        nav = nav
            .push(Space::new().height(18))
            .push(small(if self.writable {
                "工作副本"
            } else {
                "只读资料库"
            }))
            .push(small(
                self.config
                    .db
                    .file_name()
                    .and_then(|s| s.to_str())
                    .map(|s| trim(s, 12))
                    .unwrap_or("未打开".into()),
            ))
            .push(
                action(
                    "打开库…",
                    safe.then_some(Message::Dialog(Dialog::Open)),
                    false,
                )
                .width(Fill),
            )
            .push(action("刷新资料", safe.then_some(Message::Refresh), false).width(Fill))
            .push(Space::new().height(12))
            .push(small("⌘F 搜索\n切页保留草稿\n引用固定版本\n仅本机模型"));
        container(scrollable(nav))
            .padding([24, 12])
            .width(132)
            .height(Fill)
            .style(game::rail)
            .into()
    }
    fn treasury_body(&self) -> Element<'_, Message> {
        let a = &self.atelier;
        let c = a.catalog();
        let mut tabs = Row::new().spacing(4);
        for kind in Kind::ALL {
            tabs = tabs.push(
                action(kind.short(), Some(msg(atelier::Message::Tab(kind))), false).style(
                    if a.kind == kind {
                        game::selected
                    } else {
                        game::button
                    },
                ),
            );
        }
        let mut catalog = column![
            row![
                title(a.kind.title(), 22).width(Fill),
                action("新建", Some(msg(atelier::Message::New)), false)
            ]
            .align_y(Alignment::Center),
            small(match a.kind {
                Kind::Technique => "一条 Prompt，一份输出契约。",
                Kind::Recipe => "维度、配比与约束，独立合成。",
                Kind::Formula => "节点与依赖，不是一段大 Prompt。",
            }),
            tabs,
            text_input("搜索名称 / 标识", &c.query)
                .id("atelier-search")
                .on_input(|s| msg(atelier::Message::Query(s)))
                .padding(9)
                .size(13)
                .style(game::input)
        ]
        .spacing(12);
        let mut assets = Column::new().spacing(10);
        for id in a.visible_assets(a.kind, &c.query) {
            let d = a.drafts.get(&id);
            let name = d
                .map(|d| d.name.as_str())
                .filter(|s| !s.is_empty())
                .unwrap_or("未命名草稿");
            let versions = a.assets.get(&id).map_or(0, Vec::len);
            let dirty = d.is_some_and(|d| d.dirty());
            let selected = c.selected.as_ref() == Some(&id);
            assets = assets.push(
                button(
                    column![
                        small(format!(
                            "{} · {}",
                            id.kind.short(),
                            if versions == 0 {
                                "尚未保存".into()
                            } else {
                                format!("{versions} 个版本")
                            }
                        )),
                        title(name, 20),
                        small(if dirty {
                            "有未保存草稿"
                        } else {
                            "已保存 · 可修订"
                        })
                    ]
                    .spacing(7),
                )
                .on_press(msg(atelier::Message::Select(id)))
                .padding(14)
                .width(Fill)
                .style(if selected {
                    game::selected
                } else {
                    game::button
                }),
            );
        }
        if a.visible_assets(a.kind, &c.query).is_empty() {
            assets = assets.push(small("暂无匹配资产。\n新建草稿，或清除搜索。"));
        }
        catalog = catalog.push(scrollable(assets).height(Fill)).push(small(
            "草稿属于各资产，切页不丢失。\n保存不会改变工坊绑定。",
        ));
        let mut layout = row![
            container(catalog)
                .padding([12, 16])
                .width(if self.width < 1180. { 210 } else { 238 })
                .height(Fill),
            self.treasury_editor()
        ]
        .spacing(0)
        .height(Fill);
        if self.width >= 1320. {
            layout = layout.push(
                container(self.version_history())
                    .padding([16, 18])
                    .width(210)
                    .height(Fill),
            );
        }
        layout.into()
    }
    fn treasury_editor(&self) -> Element<'_, Message> {
        let a = &self.atelier;
        let Some(id) = a.selected() else {
            return container(
                column![
                    title("编修你的第一份卷宗", 27),
                    paper_small(
                        "新建手法、配方或丹方。只读库中可先编辑草稿；正式保存须建立工作副本。"
                    ),
                    paper_action("新建草稿", Some(msg(atelier::Message::New)))
                ]
                .spacing(18),
            )
            .padding(30)
            .style(game::paper)
            .width(Fill)
            .height(Fill)
            .into();
        };
        let Some(d) = a.draft() else {
            return container(text("此资产没有可读版本")).into();
        };
        let mut body = column![
            row![
                column![
                    paper_small(format!("卷宗编辑 · {}", id.kind.short())),
                    title(
                        if d.name.is_empty() {
                            "未命名草稿"
                        } else {
                            &d.name
                        },
                        28
                    )
                ]
                .spacing(5)
                .width(Fill),
                paper_small(if d.dirty() {
                    "草稿有修改"
                } else {
                    "已保存版本"
                })
            ]
            .align_y(Alignment::Center),
            row![
                field("名称", &d.name, "为这份资产命名", |s| msg(
                    atelier::Message::Edit(Field::Name, s)
                )),
                column![
                    paper_small("输入类型"),
                    pick_list(
                        ["any", "text", "image", "json"],
                        Some(d.input_type.as_str()),
                        |s| msg(atelier::Message::Edit(Field::InputType, s.into()))
                    )
                    .padding(8)
                    .text_size(13)
                ]
                .width(118)
            ]
            .spacing(12),
            paper_small(format!("固定标识 · {}", id.key))
        ]
        .spacing(18);
        if id.kind == Kind::Formula {
            body = body.push(self.formula_fields(d));
        }
        body = body
            .push(paper_small(match id.kind {
                Kind::Technique => "单 Prompt",
                Kind::Recipe => "合成 Prompt",
                Kind::Formula => "任务备注（不是执行 Prompt）",
            }))
            .push(editor(
                &d.prompt,
                if id.kind == Kind::Formula { 116. } else { 212. },
                |action| msg(atelier::Message::Editor(Field::Prompt, action)),
            ))
            .push(paper_small(
                "保存创建新的不可变版本；工坊已取用的版本不会随之改变。",
            ));
        if id.kind == Kind::Recipe {
            body = body.push(self.recipe_fields(d));
        }
        body = body.push(paper_action(
            if d.advanced {
                "收起高级 JSON / 输出契约"
            } else {
                "展开高级 JSON / 输出契约"
            },
            Some(msg(atelier::Message::Advanced)),
        ));
        if d.advanced {
            body = body
                .push(paper_small("输出 JSON Schema · object"))
                .push(editor(&d.schema, 170., |action| {
                    msg(atelier::Message::Editor(Field::Schema, action))
                }))
                .push(paper_small("完整结构 definition · 表单与 JSON 同一草稿"))
                .push(editor(&d.definition, 220., |action| {
                    msg(atelier::Message::Editor(Field::Definition, action))
                }));
        }
        if let Err(e) = d.payload(id) {
            body = body.push(text(e).size(13).color(iced::Color::from_rgb8(147, 56, 39)));
        }
        if self.width < 1320. {
            body = body.push(self.version_history_paper());
        }
        container(scrollable(container(body).padding([24, 24]).width(Fill)))
            .style(game::paper)
            .width(Fill)
            .height(Fill)
            .into()
    }
    fn recipe_fields<'a>(&'a self, d: &'a atelier::Draft) -> Element<'a, Message> {
        let definition = serde_json::from_str::<Value>(&d.definition.text()).unwrap_or(Value::Null);
        let mut rows = column![
            row![
                title("结构维度配比", 20).width(Fill),
                paper_action("添加维度", Some(msg(atelier::Message::AddComponent)))
            ]
            .align_y(Alignment::Center),
            paper_small("input 必须对应执行时该配方节点的输入 ID；权重按合计归一化。")
        ]
        .spacing(10);
        if let Some(components) = definition["components"].as_array() {
            for (i, c) in components.iter().enumerate() {
                let input = c["input"].as_str().unwrap_or("").to_owned();
                let ratio = c["ratio"].as_f64().unwrap_or(0.).to_string();
                rows = rows.push(
                    row![
                        text_input("输入节点 ID", &input)
                            .on_input(move |s| msg(atelier::Message::Component(
                                i,
                                "input".into(),
                                s
                            )))
                            .style(game::input)
                            .padding(8)
                            .size(13),
                        text_input("权重", &ratio)
                            .on_input(move |s| msg(atelier::Message::Component(
                                i,
                                "ratio".into(),
                                s
                            )))
                            .style(game::input)
                            .padding(8)
                            .size(13)
                            .width(86),
                        paper_action("移除", Some(msg(atelier::Message::RemoveComponent(i))))
                    ]
                    .spacing(8)
                    .align_y(Alignment::Center),
                );
            }
        }
        rows = rows
            .push(paper_small("约束 · JSON 数组 / 对象 / 文字"))
            .push(editor(&d.constraints, 105., |action| {
                msg(atelier::Message::Editor(Field::Constraints, action))
            }));
        rows.into()
    }
    fn formula_fields<'a>(&'a self, d: &'a atelier::Draft) -> Element<'a, Message> {
        let definition = serde_json::from_str::<Value>(&d.definition.text()).unwrap_or(Value::Null);
        let mut rows = column![
            row![
                title("节点引用与输入依赖 DAG", 20).width(Fill),
                paper_action("添加节点", Some(msg(atelier::Message::AddFormulaNode)))
            ]
            .spacing(8)
            .align_y(Alignment::Center),
            paper_small("使用明确保存版本，输入以逗号分隔节点 ID。高级 JSON 可精确编辑。")
        ]
        .spacing(12);
        let refs = self.saved_reference_options();
        if let Some(nodes) = definition["nodes"].as_array() {
            for (i, n) in nodes.iter().enumerate() {
                let id = n["id"].as_str().unwrap_or("").to_owned();
                let op = n["op"].as_str().unwrap_or("source").to_owned();
                let inputs = n["inputs"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(Value::as_str)
                    .collect::<Vec<_>>()
                    .join(", ");
                let node_refs: Vec<_> = refs
                    .iter()
                    .filter(|(_, v)| v["kind"].as_str() == Some(op.as_str()))
                    .cloned()
                    .collect();
                let selection = node_refs
                    .iter()
                    .find(|(_, v)| v == &n["asset"])
                    .map(|(s, _)| s.clone());
                let choices: Vec<_> = node_refs.iter().map(|(s, _)| s.clone()).collect();
                let owned_refs = node_refs;
                let mut card = column![
                    row![
                        text_input("节点 ID", &id)
                            .on_input(move |s| msg(atelier::Message::FormulaNode(
                                i,
                                "id".into(),
                                s
                            )))
                            .style(game::input)
                            .padding(7)
                            .size(13),
                        pick_list(
                            vec![
                                "source".to_owned(),
                                "technique".into(),
                                "recipe".into(),
                                "export".into()
                            ],
                            Some(op.clone()),
                            move |s| msg(atelier::Message::FormulaNode(i, "op".into(), s))
                        )
                        .padding(7)
                        .text_size(13),
                        paper_action("移除", Some(msg(atelier::Message::RemoveFormulaNode(i))))
                    ]
                    .spacing(6),
                    text_input("输入依赖，例如 source, node-2", &inputs)
                        .on_input(move |s| msg(atelier::Message::FormulaNode(
                            i,
                            "inputs".into(),
                            s
                        )))
                        .style(game::input)
                        .padding(7)
                        .size(13)
                ]
                .spacing(7);
                if op == "technique" || op == "recipe" {
                    card = card.push(
                        pick_list(choices, selection, move |s| {
                            let v = owned_refs
                                .iter()
                                .find(|(label, _)| label == &s)
                                .map(|(_, v)| v.clone())
                                .unwrap_or(Value::Null);
                            msg(atelier::Message::FormulaNode(
                                i,
                                "asset".into(),
                                v.to_string(),
                            ))
                        })
                        .placeholder("选择已保存的固定版本")
                        .padding(7)
                        .text_size(13)
                        .width(Fill),
                    );
                }
                rows = rows.push(card);
            }
        }
        rows.into()
    }
    fn saved_reference_options(&self) -> Vec<(String, Value)> {
        self.atelier
            .assets
            .iter()
            .filter(|(id, _)| id.kind != Kind::Formula)
            .flat_map(|(id, revs)| {
                revs.iter().map(move |r| {
                    (
                        format!(
                            "{} / {} · v{}",
                            id.kind.short(),
                            r["name"].as_str().unwrap_or(&id.key),
                            r["version"]
                        ),
                        json!({"kind":id.kind.key(),"key":id.key,"version":r["version"]}),
                    )
                })
            })
            .collect()
    }
    fn version_history(&self) -> Element<'_, Message> {
        self.version_history_content(false)
    }
    fn version_history_paper(&self) -> Element<'_, Message> {
        self.version_history_content(true)
    }
    fn version_history_content(&self, paper: bool) -> Element<'_, Message> {
        let a = &self.atelier;
        let mut body = column![
            title("版本沿革", 21),
            text("查看历史，恢复为草稿。\n保存时创建新版本，不改写历史。").size(12)
        ]
        .spacing(12);
        if let Some(id) = a.selected()
            && let Some(revs) = a.assets.get(id)
        {
            for rev in revs.iter().rev() {
                let version = rev["version"].as_u64().unwrap_or(0) as u32;
                body = body.push(
                    action(
                        format!(
                            "v{version} · {}",
                            trim(rev["name"].as_str().unwrap_or(""), 10)
                        ),
                        Some(msg(atelier::Message::Preview(version))),
                        false,
                    )
                    .style(if paper {
                        game::paper_button
                    } else {
                        game::button
                    })
                    .width(Fill),
                );
            }
            let selected = a
                .catalog()
                .preview
                .and_then(|v| a.revision(id, v))
                .or(revs.last());
            if let Some(rev) = selected {
                body = body
                    .push(text(format!("预览 v{}", rev["version"])).size(13))
                    .push(text(data::pretty(&rev["prompt"])).size(13))
                    .push(
                        action(
                            "恢复此版本为草稿",
                            Some(msg(atelier::Message::Restore(
                                rev["version"].as_u64().unwrap_or(0) as u32,
                            ))),
                            false,
                        )
                        .style(if paper {
                            game::paper_button
                        } else {
                            game::button
                        }),
                    )
                    .push(
                        text(format!(
                            "哈希 {}",
                            trim(rev["hash"].as_str().unwrap_or("未返回"), 20)
                        ))
                        .size(12),
                    );
            }
        }
        body = body.push(text("引用不会随编辑漂移。\n草稿未保存时不能送往工坊。").size(12));
        container(scrollable(body))
            .height(if paper { Length::Fixed(290.) } else { Fill })
            .into()
    }
    fn workshop_body(&self) -> Element<'_, Message> {
        let a = &self.atelier;
        let w = &a.workshop;
        let mut controls = row![title("装配棋盘", 22), Space::new().width(Fill)]
            .spacing(8)
            .align_y(Alignment::Center);
        if self.width >= 1320. {
            controls = controls.push(small(format!(
                "{} 个节点 · 点击选择 / 拖动位置",
                w.nodes.len()
            )));
        }
        controls = controls
            .push(action(
                "源料 +",
                (!a.running()).then_some(msg(atelier::Message::AddSource)),
                false,
            ))
            .push(action(
                "导出 +",
                (!a.running()).then_some(msg(atelier::Message::AddExport)),
                false,
            ))
            .push(action(
                if w.picker {
                    "收起取用"
                } else {
                    "从藏经阁取用"
                },
                Some(msg(atelier::Message::Picker)),
                false,
            ));
        let max_x = w
            .nodes
            .iter()
            .map(|n| n.position.0 + 260.)
            .fold(900., f32::max);
        let max_y = w
            .nodes
            .iter()
            .map(|n| n.position.1 + 190.)
            .fold(430., f32::max);
        let board = stack![
            image(bg())
                .content_fit(iced::ContentFit::Cover)
                .width(max_x)
                .height(max_y),
            iced::widget::canvas(crate::board::Board { atelier: a })
                .width(max_x)
                .height(max_y)
        ];
        let canvas = scrollable(board)
            .direction(scrollable::Direction::Both {
                vertical: scrollable::Scrollbar::default(),
                horizontal: scrollable::Scrollbar::default(),
            })
            .height(Fill)
            .width(Fill);
        let mut center = column![container(controls).padding([12, 16]), canvas]
            .spacing(0)
            .height(Fill);
        let mut node_buttons = Row::new().spacing(6);
        for (i, n) in w.nodes.iter().enumerate() {
            node_buttons = node_buttons.push(action(
                format!("{} · {}", n.id, trim(n.name(), 8)),
                Some(msg(atelier::Message::SelectNode(i))),
                false,
            ));
        }
        center = center.push(scrollable(node_buttons).direction(
            scrollable::Direction::Horizontal(scrollable::Scrollbar::default()),
        ));
        if w.picker {
            center = center.push(self.asset_picker());
        }
        center = center.push(self.run_drawer());
        row![
            container(center).width(Fill).height(Fill),
            container(scrollable(self.workshop_inspector()))
                .padding(18)
                .width(if self.width < 1180. { 280 } else { 310 })
                .height(Fill)
                .style(game::paper)
        ]
        .height(Fill)
        .into()
    }
    fn asset_picker(&self) -> Element<'_, Message> {
        let a = &self.atelier;
        let w = &a.workshop;
        let mut tabs = Row::new().spacing(6);
        for kind in Kind::ALL {
            tabs = tabs.push(action(
                kind.short(),
                Some(msg(atelier::Message::PickerTab(kind))),
                false,
            ));
        }
        let mut list = Row::new().spacing(10);
        for id in a.visible_assets(w.picker_kind, &w.picker_query) {
            if let Some(revs) = a.assets.get(&id) {
                for rev in revs.iter().rev() {
                    let rev = rev.clone();
                    list = list.push(
                        container(
                            column![
                                small(format!("{} · 已保存 v{}", id.kind.short(), rev["version"])),
                                title(trim(rev["name"].as_str().unwrap_or(&id.key), 14), 20),
                                row![
                                    action(
                                        if id.kind == Kind::Formula {
                                            "载入丹方…"
                                        } else {
                                            "加入"
                                        },
                                        (!a.running()).then_some(msg(atelier::Message::Bind(
                                            rev.clone(),
                                            false
                                        ))),
                                        true
                                    ),
                                    action(
                                        "替换选中",
                                        (!a.running() && id.kind != Kind::Formula)
                                            .then_some(msg(atelier::Message::Bind(rev, true))),
                                        false
                                    )
                                ]
                                .spacing(6)
                            ]
                            .spacing(7),
                        )
                        .padding(12)
                        .width(258)
                        .style(game::panel),
                    );
                }
            }
        }
        let content = column![
            row![
                small("临时取用 · 不包含未保存草稿"),
                tabs,
                text_input("搜索保存资产", &w.picker_query)
                    .id("atelier-picker-search")
                    .on_input(|s| msg(atelier::Message::PickerQuery(s)))
                    .style(game::input)
                    .padding(7)
                    .size(13)
                    .width(Fill)
            ]
            .spacing(10)
            .align_y(Alignment::Center),
            scrollable(list)
                .direction(scrollable::Direction::Horizontal(
                    scrollable::Scrollbar::default()
                ))
                .height(118)
        ]
        .spacing(10);
        container(content).padding(12).style(game::rail).into()
    }
    fn workshop_inspector(&self) -> Element<'_, Message> {
        let a = &self.atelier;
        let w = &a.workshop;
        let mut body = column![
            paper_small("当前丹方 · 独立装配草稿"),
            text_input("丹方名称", &w.name)
                .on_input(|s| msg(atelier::Message::PlanName(s)))
                .style(game::input)
                .padding(8)
                .size(16),
            row![
                paper_action("节点绑定", Some(msg(atelier::Message::NodeDetails))),
                paper_action(
                    if w.show_config {
                        "收起配置"
                    } else {
                        "输入与模型"
                    },
                    Some(msg(atelier::Message::Configure))
                )
            ]
            .spacing(8)
        ]
        .spacing(14);
        if a.running() {
            body = body.push(paper_small(
                "本次运行已冻结启动时的计划与配置；输入和模型的后续修改只影响下一炉。",
            ));
        }
        if let Some(rev) = &w.pending_formula {
            body = body.push(
                container(
                    column![
                        text(format!(
                            "载入「{}」？",
                            rev["name"].as_str().unwrap_or("丹方")
                        ))
                        .size(15),
                        paper_small("确认后替换棋盘装配；现有藏经阁草稿、输入与模型配置不变。"),
                        row![
                            paper_action(
                                "保留现有",
                                Some(msg(atelier::Message::ConfirmFormula(false)))
                            ),
                            paper_action(
                                "确认载入",
                                Some(msg(atelier::Message::ConfirmFormula(true)))
                            )
                        ]
                        .spacing(6)
                    ]
                    .spacing(10),
                )
                .padding(12)
                .style(game::paper),
            );
        }
        if !w.show_config {
            if let Some(i) = w.selected
                && let Some(n) = w.nodes.get(i)
            {
                body = body
                    .push(title(n.name(), 24))
                    .push(paper_small(format!("节点 {} · {}", n.id, n.op)));
                if let Some(rev) = &n.revision {
                    body = body
                        .push(paper_small(format!(
                            "固定 v{} · {}",
                            rev["version"],
                            rev["input_type"].as_str().unwrap_or("any")
                        )))
                        .push(text(rev["prompt"].as_str().unwrap_or("")).size(14))
                        .push(paper_small(format!(
                            "输出契约 {}",
                            data::short(&rev["output_schema"], 180)
                        )))
                        .push(paper_action(
                            "去藏经阁编辑",
                            Some(msg(atelier::Message::EditBound)),
                        ));
                } else {
                    body = body.push(paper_small(if n.op == "source" {
                        "输入来自下方任务文字或明确选择的素材，不是示例文本。"
                    } else {
                        "将依赖节点的实际结果导出为 JSON 和 Brief。"
                    }));
                }
                body = body
                    .push(paper_small("输入依赖 · 逗号分隔节点 ID"))
                    .push(
                        text_input("例如 source, node-1", &n.inputs)
                            .on_input(|s| msg(atelier::Message::NodeInputs(s)))
                            .style(game::input)
                            .padding(8)
                            .size(13),
                    )
                    .push(paper_action(
                        "移除此节点",
                        (!a.running()).then_some(msg(atelier::Message::RemoveNode)),
                    ));
            }
            body = body.push(paper_small(
                "操作不改写已保存资产。加入节点默认连接当前选中节点，可修改输入依赖。",
            ));
        }
        body = body.push(title("任务输入", 21)).push(
            checkbox(w.use_work)
                .label("使用明确选中的真实素材")
                .text_size(13)
                .on_toggle(|b| msg(atelier::Message::UseWork(b))),
        );
        if w.use_work {
            body = body
                .push(paper_small(
                    w.work
                        .map(|id| format!("已绑定素材 #{id}"))
                        .unwrap_or("尚未绑定真实素材".into()),
                ))
                .push(paper_action(
                    "使用素材页当前选择",
                    self.selection.map(|id| msg(atelier::Message::SetWork(id))),
                ))
                .push(paper_action(
                    "前往素材库选择",
                    Some(Message::Navigate(Screen::Library)),
                ))
                .push(paper_small(
                    "图像需要支持视觉的本地模型，并主动开启 vision；不隐式忽略图片。",
                ));
        } else {
            body = body.push(editor(&w.source, 110., |action| {
                msg(atelier::Message::Source(action))
            }));
        }
        body = body
            .push(title("本地模型", 21))
            .push(paper_small(&w.model.connection));
        if w.show_config {
            body = body.push(self.model_form());
        } else {
            body = body
                .push(paper_small(format!(
                    "{}\n{}",
                    if w.model.name.is_empty() {
                        "尚未选模型"
                    } else {
                        &w.model.name
                    },
                    w.model.endpoint
                )))
                .push(paper_action(
                    "配置服务与模型",
                    Some(msg(atelier::Message::Configure)),
                ));
        }
        body = body
            .push(paper_small("保存为丹方录 · 不存输入文字和模型凭据"))
            .push(field(
                "丹方标识（留空自动新建）",
                &w.formula_key,
                "例如 my-workflow",
                |s| msg(atelier::Message::FormulaKey(s)),
            ));
        // Reserve the vertical scrollbar inside the fixed paper inspector;
        // otherwise a long schema or note can size the child past the viewport.
        body.width(Fill)
            .max_width(if self.width < 1180. { 232 } else { 262 })
            .into()
    }
    fn model_form(&self) -> Element<'_, Message> {
        let m = &self.atelier.workshop.model;
        let mut form = column![
            paper_small("协议 · 仅回环地址，后端强校验"),
            pick_list(["ollama", "openai"], Some(m.protocol.as_str()), |s| msg(
                atelier::Message::Model(ModelField::Protocol, s.into())
            ))
            .padding(8)
            .text_size(13)
            .width(Fill),
            field("Endpoint", &m.endpoint, "http://127.0.0.1:11434", |s| msg(
                atelier::Message::Model(ModelField::Endpoint, s)
            )),
            paper_action(
                if m.loading {
                    "正在刷新…"
                } else {
                    "明确刷新模型列表"
                },
                (!m.loading).then_some(msg(atelier::Message::Discover))
            ),
            field(
                "模型名称",
                &m.name,
                "模型名称，不预设已安装",
                |s| msg(atelier::Message::Model(ModelField::Name, s))
            )
        ]
        .spacing(10);
        if !m.models.is_empty() {
            form = form.push(
                pick_list(m.models.clone(), Some(m.name.clone()), |s| {
                    msg(atelier::Message::Model(ModelField::Name, s))
                })
                .placeholder("选本地服务返回的模型")
                .padding(8)
                .text_size(13)
                .width(Fill),
            );
        }
        form=form.push(row![field("Temperature",&m.temperature,"0.2",|s|msg(atelier::Message::Model(ModelField::Temperature,s))),field("Context",&m.context,"8192",|s|msg(atelier::Message::Model(ModelField::Context,s)))].spacing(8)).push(row![field("最大输出",&m.max_tokens,"2048",|s|msg(atelier::Message::Model(ModelField::MaxTokens,s))),field("超时 / 秒",&m.timeout,"120",|s|msg(atelier::Message::Model(ModelField::Timeout,s)))].spacing(8)).push(checkbox(m.vision).label("此模型支持视觉输入（vision）").text_size(13).on_toggle(|b|msg(atelier::Message::Vision(b)))).push(paper_small("Ollama 的 Context 会传入 num_ctx；OpenAI 兼容协议仅记录 Context，不声称服务已应用。仅本机服务、不调用云端收费 API。停止请求需等待当前 HTTP 返回或超时。"));
        form.into()
    }
    fn run_drawer(&self) -> Element<'_, Message> {
        let a = &self.atelier;
        let w = &a.workshop;
        let state = a
            .run
            .as_ref()
            .map(|r| match &r.state {
                RunState::Running => "后台运行中".to_owned(),
                RunState::StopRequested => "已请求停止 · 等待当前请求返回".into(),
                RunState::Finished(status) => format!("运行结束 · {status}"),
            })
            .unwrap_or("等待开炉 · 尚无真实执行".into());
        let mut header = row![
            action(
                "任务日志",
                Some(msg(atelier::Message::Output(false))),
                false
            ),
            action("实际产物", Some(msg(atelier::Message::Output(true))), false),
            small(state).width(Fill)
        ]
        .spacing(8)
        .align_y(Alignment::Center);
        if !a.runs.is_empty() {
            let options: Vec<String> = a
                .runs
                .iter()
                .map(|r| {
                    format!(
                        "#{} · {} · {}",
                        r["id"],
                        r["name"].as_str().unwrap_or(""),
                        r["status"].as_str().unwrap_or("")
                    )
                })
                .collect();
            let ids: Vec<i64> = a.runs.iter().filter_map(|v| v["id"].as_i64()).collect();
            let selected = w
                .history
                .and_then(|id| ids.iter().position(|n| *n == id))
                .and_then(|i| options.get(i))
                .cloned();
            let values = options.clone();
            header = header.push(
                pick_list(options, selected, move |s| {
                    let index = values.iter().position(|v| v == &s).unwrap_or(0);
                    msg(atelier::Message::History(
                        ids.get(index).copied().unwrap_or_default(),
                    ))
                })
                .placeholder("持久化历史")
                .padding(7)
                .text_size(12)
                .width(220),
            );
        }
        let mut lines = Column::new().spacing(6);
        let record = w
            .history
            .and_then(|id| a.runs.iter().find(|r| r["id"].as_i64() == Some(id)))
            .or_else(|| a.run.as_ref().and_then(|r| r.result.as_ref()));
        if w.show_output {
            if let Some(record) = record {
                if record["artifacts_valid"] == false {
                    lines = lines
                        .push(
                            text(
                                "产物完整性校验失败：历史可读，但不能将这些文件视为可信成功产物。",
                            )
                            .size(13)
                            .color(game::ERROR),
                        )
                        .push(text(data::pretty(&record["artifact_integrity"])).size(12));
                }
                if let Some(error) = record["error"].as_str() {
                    lines = lines.push(text(error).size(13).color(game::ERROR));
                }
                for artifact in record["output"]["artifacts"]
                    .as_array()
                    .into_iter()
                    .flatten()
                {
                    lines = lines.push(
                        text(format!(
                            "JSON · {}\nBrief · {}",
                            artifact["json_path"].as_str().unwrap_or("未返回"),
                            artifact["brief_path"].as_str().unwrap_or("未返回")
                        ))
                        .size(13),
                    );
                }
                lines = lines
                    .push(text(data::pretty(&record["output"])).size(13))
                    .push(small(format!(
                        "冻结快照 {}",
                        record["snapshot_hash"].as_str().unwrap_or("未返回")
                    )));
            } else {
                lines = lines.push(small(
                    "尚无实际产物。运行完成后展示持久化结果、错误、输出目录与 JSON。",
                ));
            }
        } else if w.history.is_some() {
            if let Some(record) = record {
                for event in record["events"].as_array().into_iter().flatten() {
                    lines = lines.push(text(data::pretty(event)).size(12));
                }
                lines = lines.push(text(data::pretty(&record["stages"])).size(12));
            }
        } else if let Some(run) = &a.run {
            for event in run.events.iter().rev().take(80).rev() {
                lines = lines.push(text(data::pretty(event)).size(12));
            }
            if run.events.is_empty() {
                lines = lines.push(small("等待后端返回真实节点事件；不显示随机进度。"));
            }
        } else {
            lines = lines.push(small(
                "仅使用真实后台事件；没有模拟进度，不会自动生成人工评价或拟合偏好。",
            ));
        }
        container(column![header, scrollable(lines).height(116)].spacing(8))
            .padding([12, 16])
            .style(game::rail)
            .into()
    }
}
