use std::path::PathBuf;

use iced::widget::{
    Column, Row, Space, button, checkbox, column, container, image, row, rule, scrollable, slider,
    text, text_input,
};
use iced::{Alignment, Element, Fill, FillPortion, Length};
use serde_json::Value;

use crate::app::{Dialog, Message, PAGE_SIZE, Studio};
use crate::data::{self, dimension, flag, id, label};
use crate::options::Screen;
use crate::style;

fn auxiliary<'a>(value: impl Into<String>) -> iced::widget::Text<'a> {
    text(value.into()).size(12).style(style::secondary)
}
fn heading<'a>(value: impl Into<String>) -> iced::widget::Text<'a> {
    text(value.into()).size(17)
}
fn action<'a>(
    value: impl Into<String>,
    message: Option<Message>,
    primary: bool,
) -> iced::widget::Button<'a, Message> {
    button(text(value.into()).size(13))
        .padding([6, 12])
        .height(32)
        .on_press_maybe(message)
        .style(if primary {
            style::primary
        } else {
            style::normal
        })
}
fn trim(value: &str, count: usize) -> String {
    if value.chars().count() > count {
        format!("{}…", value.chars().take(count).collect::<String>())
    } else {
        value.into()
    }
}
fn line<'a>() -> iced::widget::Rule<'a> {
    rule::horizontal(1)
}
fn scroll_gutter() -> iced::Padding {
    iced::Padding {
        right: 16.,
        ..Default::default()
    }
}
fn detail_row<'a>(name: impl Into<String>, value: impl Into<String>) -> Element<'a, Message> {
    row![
        auxiliary(name).width(110),
        text(value.into()).size(13).width(Fill)
    ]
    .spacing(12)
    .into()
}

impl Studio {
    pub fn view(&self) -> Element<'_, Message> {
        let body = column![self.toolbar(), self.feedback(), self.content()]
            .height(Fill)
            .width(Fill)
            .spacing(0);
        let mut layout = Row::new().height(Fill);
        if self.sidebar {
            layout = layout.push(self.navigation()).push(rule::vertical(1));
        }
        layout = layout.push(body);
        container(layout)
            .width(Fill)
            .height(Fill)
            .style(style::root)
            .into()
    }
    fn navigation(&self) -> Element<'_, Message> {
        let mut nav = column![
            text("炼丹炉").size(22),
            auxiliary("本地视觉合成"),
            Space::new().height(16),
        ]
        .spacing(8);
        for screen in Screen::ALL {
            let count = match screen {
                Screen::Library => self.data.works.len(),
                Screen::Brew => self.data.extracts.len(),
                Screen::Pills => self.data.pills.len(),
                Screen::Preferences => self.data.experiments.len(),
            };
            nav = nav.push(
                button(
                    row![
                        text(screen.title()).size(13).width(Fill),
                        auxiliary(count.to_string())
                    ]
                    .align_y(Alignment::Center),
                )
                .width(Fill)
                .height(36)
                .padding([8, 10])
                .on_press(Message::Navigate(screen))
                .style(if self.screen == screen {
                    style::selected
                } else {
                    style::quiet
                }),
            );
        }
        let db_name = self
            .config
            .db
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("尚未打开数据库");
        nav = nav
            .push(Space::new().height(16))
            .push(line())
            .push(Space::new().height(8))
            .push(auxiliary(if self.writable {
                "当前工作副本"
            } else {
                "只读资料库"
            }))
            .push(text(trim(db_name, 20)).size(13))
            .push(
                action(
                    "打开数据库…",
                    self.busy.is_none().then_some(Message::Dialog(Dialog::Open)),
                    false,
                )
                .width(Fill),
            )
            .push(
                action(
                    "建立工作副本…",
                    (self.busy.is_none() && !self.config.db.as_os_str().is_empty())
                        .then_some(Message::Dialog(Dialog::Copy)),
                    false,
                )
                .width(Fill),
            )
            .push(
                action(
                    "建立新库…",
                    self.busy.is_none().then_some(Message::Dialog(Dialog::New)),
                    false,
                )
                .width(Fill),
            )
            .push(Space::new().height(Fill))
            .push(auxiliary("⌘/Ctrl 1–4 切换页面\n⌘/Ctrl F 搜索 · R 刷新"));
        container(nav)
            .padding([24, 16])
            .width(184)
            .height(Fill)
            .style(style::sidebar)
            .into()
    }
    fn toolbar(&self) -> Element<'_, Message> {
        let primary = match self.screen {
            Screen::Library => action(
                "本地炼药",
                (self.can_write()
                    && self
                        .work()
                        .and_then(|w| data::path(w, "image_path"))
                        .is_some())
                .then_some(Message::Refine),
                true,
            ),
            Screen::Brew => {
                if self.recipe.is_some() || self.custom_recipe.is_some() {
                    action(
                        "按配方炼制",
                        self.can_write().then_some(Message::BrewRecipe),
                        true,
                    )
                } else {
                    action(
                        "炼制丹药",
                        (self.can_write()
                            && self.selection.is_some()
                            && !self
                                .data
                                .extracts_for(self.selection.unwrap_or_default())
                                .is_empty())
                        .then_some(Message::Brew),
                        true,
                    )
                }
            }
            Screen::Pills => action(
                "导出 Brief…",
                (self.can_write()
                    && self
                        .data
                        .pill(self.pill)
                        .is_some_and(|p| flag(p, "snapshot")))
                .then_some(Message::Export),
                true,
            ),
            Screen::Preferences => action(
                "刷新比较",
                self.busy.is_none().then_some(Message::Refresh),
                false,
            ),
        };
        let search = text_input(
            match self.screen {
                Screen::Library => "搜索标题、领域或集合",
                Screen::Brew => "搜索本地配方",
                Screen::Pills => "搜索丹药或配方",
                Screen::Preferences => "搜索比较内容",
            },
            &self.query,
        )
        .id("studio-search")
        .on_input(Message::Search)
        .size(13)
        .padding([7, 10])
        .width(if self.width < 1120. { 180 } else { 224 });
        let mut controls = row![
            action("导航", Some(Message::ToggleSidebar), false),
            text(self.screen.title()).size(19),
            Space::new().width(Fill),
            search
        ]
        .spacing(8)
        .align_y(Alignment::Center);
        if !self.query.is_empty() {
            controls = controls.push(action("清除", Some(Message::ClearSearch), false));
        }
        if self.width >= 1120. {
            controls = controls.push(action(
                "刷新",
                self.busy.is_none().then_some(Message::Refresh),
                false,
            ));
        }
        controls = controls
            .push(action(
                if self.inspector {
                    "收起检查"
                } else {
                    "显示检查"
                },
                Some(Message::ToggleInspector),
                false,
            ))
            .push(primary);
        container(controls).padding([16, 20]).width(Fill).into()
    }
    fn feedback(&self) -> Element<'_, Message> {
        let mut feedback = Column::new().spacing(8);
        if self.effective_demo() {
            feedback = feedback.push(
                container(text("隔离体验数据，不记录真人 · 所有偏好动作强制标记为 demo").size(12))
                    .padding([8, 12])
                    .width(Fill)
                    .style(style::inset),
            );
        }
        if !self.writable && !self.config.db.as_os_str().is_empty() {
            feedback = feedback.push(
                row![
                    column![
                        text(if self.data.needs_migration {
                            "旧版数据库 · 安全浏览"
                        } else {
                            "原数据库只读"
                        })
                        .size(13),
                        auxiliary("炼药、炼制与偏好记录将在独立工作副本中进行，不静默迁移原库。")
                    ]
                    .spacing(3)
                    .width(Fill),
                    action(
                        "建立工作副本…",
                        self.busy.is_none().then_some(Message::Dialog(Dialog::Copy)),
                        true
                    )
                ]
                .align_y(Alignment::Center)
                .spacing(12),
            );
        }
        let status = self.busy.as_deref().unwrap_or(&self.status);
        feedback = feedback.push(text(trim(status, 260)).size(12).style(if self.error {
            style::error_text
        } else {
            style::secondary
        }));
        if let Some(warning) = self.data.warnings.first() {
            feedback = feedback.push(auxiliary(format!("资料提示：{}", trim(warning, 150))));
        }
        container(feedback)
            .padding(iced::Padding {
                top: 0.,
                right: 20.,
                bottom: 12.,
                left: 20.,
            })
            .width(Fill)
            .into()
    }
    fn content(&self) -> Element<'_, Message> {
        let center = match self.screen {
            Screen::Library => self.library(),
            Screen::Brew => self.brew(),
            Screen::Pills => self.pills(),
            Screen::Preferences => self.preferences(),
        };
        let mut content = row![
            container(center)
                .padding(iced::Padding {
                    top: 8.,
                    right: 20.,
                    bottom: 20.,
                    left: 20.
                })
                .width(Fill)
                .height(Fill)
        ]
        .height(Fill);
        if self.inspector {
            content = content.push(rule::vertical(1)).push(
                container(self.inspection())
                    .padding([20, 16])
                    .width(if self.width < 1120. { 260 } else { 288 })
                    .height(Fill)
                    .style(style::sidebar),
            );
        }
        content.into()
    }
    fn empty<'a>(
        &self,
        title: &'static str,
        detail: &'static str,
        cta: &'static str,
        message: Option<Message>,
    ) -> Element<'a, Message> {
        container(
            column![
                heading(title),
                text(detail).size(13).width(Length::Fill),
                action(cta, message, false)
            ]
            .spacing(12)
            .max_width(460),
        )
        .center_x(Fill)
        .center_y(Fill)
        .height(Fill)
        .into()
    }
    fn library(&self) -> Element<'_, Message> {
        if self.data.works.is_empty() {
            return self.empty(
                if self.loaded {
                    "从你的素材开始"
                } else {
                    "正在读取素材"
                },
                "打开已有素材数据库即可浏览。当前不会创建默认用户库，也不会发起模型请求。",
                "打开数据库…",
                self.busy.is_none().then_some(Message::Dialog(Dialog::Open)),
            );
        }
        let works = self.data.filtered_works(&self.query);
        if works.is_empty() {
            return self.empty(
                "没有匹配的素材",
                "试试标题、集合、领域或素材编号。不会改变原有素材。",
                "清除搜索",
                Some(Message::ClearSearch),
            );
        }
        let total = works.len();
        let pages = total.div_ceil(PAGE_SIZE);
        let list_controls = row![
            auxiliary(format!("{} 份素材", total)),
            Space::new().width(Fill),
            action(
                if self.list {
                    "缩略图"
                } else {
                    "对象列表"
                },
                Some(Message::ToggleList),
                false
            ),
            action(
                "上一页",
                (self.page > 0).then_some(Message::Page(self.page.saturating_sub(1))),
                false
            ),
            auxiliary(format!("{} / {}", self.page + 1, pages)),
            action(
                "下一页",
                (self.page + 1 < pages).then_some(Message::Page(self.page + 1)),
                false
            )
        ]
        .align_y(Alignment::Center)
        .spacing(8);
        let visible: Vec<_> = works
            .into_iter()
            .skip(self.page * PAGE_SIZE)
            .take(PAGE_SIZE)
            .collect();
        let mut items = Column::new().spacing(16);
        if self.list {
            items = items.push(
                row![
                    auxiliary("素材").width(Fill),
                    auxiliary("领域 / 集合").width(168),
                    auxiliary("药液").width(60)
                ]
                .padding([0, 8]),
            );
            for work in visible {
                let selected = self.selection == Some(id(work));
                items = items.push(
                    button(
                        row![
                            self.preview(data::path(work, "image_path"), 72., 54.),
                            column![
                                text(trim(&data::title(work), 32)).size(13),
                                auxiliary(format!("编号 {}", id(work)))
                            ]
                            .spacing(4)
                            .width(Fill),
                            auxiliary(format!(
                                "{} / {}",
                                label(work, "domain"),
                                label(work, "collection")
                            ))
                            .width(168),
                            auxiliary(
                                work["extract_count"]
                                    .as_i64()
                                    .unwrap_or_default()
                                    .to_string()
                            )
                            .width(60)
                        ]
                        .align_y(Alignment::Center)
                        .spacing(12),
                    )
                    .width(Fill)
                    .padding(8)
                    .on_press(Message::SelectWork(id(work)))
                    .style(if selected {
                        style::selected
                    } else {
                        style::normal
                    }),
                );
            }
        } else {
            let available = self.width
                - if self.sidebar { 184. } else { 0. }
                - if self.inspector {
                    if self.width < 1120. { 260. } else { 288. }
                } else {
                    0.
                }
                - 42.;
            let columns = ((available / 220.).floor() as usize).clamp(2, 5);
            for group in visible.chunks(columns) {
                let mut grid = Row::new().spacing(16);
                for work in group {
                    let selected = self.selection == Some(id(work));
                    let content = column![
                        self.preview(
                            data::path(work, "image_path"),
                            Fill,
                            if self.width < 1120. { 140. } else { 168. }
                        ),
                        column![
                            text(trim(&data::title(work), 28)).size(13),
                            auxiliary(format!(
                                "{} · {}",
                                label(work, "domain"),
                                label(work, "collection")
                            )),
                            auxiliary(format!(
                                "{} 份药液",
                                work["extract_count"].as_i64().unwrap_or_default()
                            ))
                        ]
                        .spacing(5)
                        .padding([10, 10]),
                    ]
                    .spacing(0);
                    grid = grid.push(
                        button(content)
                            .padding(0)
                            .width(FillPortion(1))
                            .on_press(Message::SelectWork(id(work)))
                            .style(if selected {
                                style::selected
                            } else {
                                style::normal
                            }),
                    );
                }
                for _ in group.len()..columns {
                    grid = grid.push(Space::new().width(FillPortion(1)));
                }
                items = items.push(grid);
            }
        }
        column![
            list_controls,
            scrollable(container(items).padding(scroll_gutter()).width(Fill)).height(Fill)
        ]
        .spacing(16)
        .into()
    }
    fn preview<'a>(
        &self,
        path: Option<PathBuf>,
        width: impl Into<Length>,
        height: f32,
    ) -> Element<'a, Message> {
        let width = width.into();
        let asset = path.as_ref().and_then(|p| self.assets.get(p));
        let content: Element<'a, Message> =
            if let Some(handle) = asset.and_then(|a| a.handle.clone()) {
                image(handle)
                    .content_fit(iced::ContentFit::Contain)
                    .width(Fill)
                    .height(Fill)
                    .into()
            } else {
                container(auxiliary(
                    if let Some(error) = asset.and_then(|a| a.error.as_ref()) {
                        trim(error, 56)
                    } else if path.is_some() {
                        "正在载入图像…".into()
                    } else {
                        "无本地预览图".into()
                    },
                ))
                .padding(12)
                .center_x(Fill)
                .center_y(Fill)
                .into()
            };
        container(content)
            .width(width)
            .height(height)
            .style(style::inset)
            .into()
    }
    fn trajectory(&self, work: &Value) -> Element<'_, Message> {
        let extracts = self.data.extracts_for(id(work));
        let current = extracts.iter().filter(|e| flag(e, "current")).count();
        let colors = extracts
            .iter()
            .flat_map(|e| data::palette(&e["content"]))
            .take(6)
            .collect::<Vec<_>>();
        let swatches = self.swatches(colors, 18.);
        let goal = if self.recipe.is_some() || self.custom_recipe.is_some() {
            "本地配方".into()
        } else {
            format!(
                "配色 {:.0}% / 间距 {:.0}%",
                self.palette_ratio * 100.,
                (1. - self.palette_ratio) * 100.
            )
        };
        container(
            column![
                auxiliary("合成轨迹"),
                row![
                    column![
                        text("选材").size(13),
                        auxiliary(trim(&data::title(work), 14))
                    ]
                    .spacing(6)
                    .width(FillPortion(1)),
                    text("→").size(16),
                    column![
                        text("药液").size(13),
                        auxiliary(format!("{} 份当前版本", current)),
                        swatches
                    ]
                    .spacing(6)
                    .width(FillPortion(1)),
                    text("→").size(16),
                    column![text("配方").size(13), auxiliary(goal)]
                        .spacing(6)
                        .width(FillPortion(1))
                ]
                .spacing(12)
                .align_y(Alignment::Start)
            ]
            .spacing(12),
        )
        .padding(16)
        .width(Fill)
        .style(style::inset)
        .into()
    }
    fn extract_swatches<'a>(&self, extract: &Value, height: f32) -> Element<'a, Message> {
        if extract["dimension"] != "palette" {
            return Space::new().height(0).into();
        }
        self.swatches(data::palette(&extract["content"]), height)
    }
    fn swatches<'a>(
        &self,
        colors: Vec<(iced::Color, String)>,
        height: f32,
    ) -> Element<'a, Message> {
        if colors.is_empty() {
            return auxiliary("未提取配色").into();
        }
        let mut swatches = Row::new().spacing(4);
        for (color, hex) in colors {
            swatches = swatches.push(
                container(Space::new().width(Fill))
                    .height(height)
                    .width(FillPortion(1))
                    .style(move |_| iced::widget::container::Style {
                        background: Some(color.into()),
                        border: iced::Border {
                            radius: 3.into(),
                            ..Default::default()
                        },
                        ..Default::default()
                    }),
            );
            let _ = hex;
        }
        swatches.into()
    }
    fn brew(&self) -> Element<'_, Message> {
        let Some(work) = self.work() else {
            return self.empty("先选一份素材", "本地炼药将图片转成 palette 与 spacing 两种药液。只有像素计算，不会调用视觉模型或 API。", "前往素材库", Some(Message::Navigate(Screen::Library)));
        };
        let mut body = column![
            heading(data::title(work)), self.trajectory(work),
            row![heading("当前选材").width(Fill), action("重新选材", Some(Message::Navigate(Screen::Library)), false), action("本地炼药", (self.can_write() && data::path(work, "image_path").is_some()).then_some(Message::Refine), false)].align_y(Alignment::Center).spacing(8),
            row![self.preview(data::path(work, "image_path"), 180., 132.), column![text("仅计算像素，不调用模型").size(13), auxiliary("palette：颜色分布\nspacing：留白与间距代理\n像素代理不是语义理解，也不是审美评分。")].spacing(8).width(Fill)].spacing(16),
            line(), heading("配方与比例"),
            row![action("像素配方", Some(Message::SelectRecipe(None)), false), Space::new().width(Fill), action("选择本地配方…", self.busy.is_none().then_some(Message::Dialog(Dialog::Recipe)), false)].spacing(8),
        ].spacing(16);
        if self.recipe.is_none() && self.custom_recipe.is_none() {
            body = body
                .push(
                    row![
                        text("配色液").size(13),
                        Space::new().width(Fill),
                        text(format!("{:.0}%", self.palette_ratio * 100.)).size(13)
                    ]
                    .align_y(Alignment::Center),
                )
                .push(
                    slider(0. ..=1., self.palette_ratio, Message::Ratio)
                        .step(0.05)
                        .height(24),
                )
                .push(auxiliary(format!(
                    "间距液 {:.0}% · 同一份素材的当前药液将被冻结为本地配方规格",
                    (1. - self.palette_ratio) * 100.
                )));
        }
        if let Some(path) = &self.custom_recipe {
            body = body.push(
                container(
                    column![
                        text("已选外部本地配方").size(13),
                        auxiliary(path.display().to_string()),
                        auxiliary("仅接受本地配方执行。缺少药液或不兼容时，后端将返回明确错误。")
                    ]
                    .spacing(6),
                )
                .padding(12)
                .width(Fill)
                .style(style::inset),
            );
        }
        for (index, recipe) in self
            .data
            .recipes
            .iter()
            .enumerate()
            .filter(|(_, r)| data::matches(r, &self.query, &["name", "key", "goal"]))
        {
            body = body.push(
                button(
                    column![
                        row![
                            text(label(recipe, "name").to_owned()).size(13).width(Fill),
                            auxiliary(if self.recipe == Some(index) {
                                "已选择"
                            } else if data::path(recipe, "path").is_none() {
                                "仅浏览 · 无配方文件"
                            } else {
                                "本地配方"
                            })
                        ],
                        auxiliary(label(recipe, "goal").to_owned()),
                        auxiliary(format!(
                            "所需维度：{}",
                            data::short(&recipe["required_dimensions"], 120)
                        ))
                    ]
                    .spacing(6),
                )
                .width(Fill)
                .padding(12)
                .on_press_maybe(
                    data::path(recipe, "path").map(|_| Message::SelectRecipe(Some(index))),
                )
                .style(if self.recipe == Some(index) {
                    style::selected
                } else {
                    style::normal
                }),
            );
        }
        body = body.push(line()).push(heading("药液与历史版本"));
        let extracts = self.data.extracts_for(id(work));
        if extracts.is_empty() {
            body = body.push(text("尚无药液。建立工作副本后，点击上方「本地炼药」。").size(13));
        }
        for extract in extracts {
            body = body.push(
                container(
                    column![
                        row![
                            text(format!("{} · #{}", dimension(extract), id(extract)))
                                .size(13)
                                .width(Fill),
                            auxiliary(if flag(extract, "current") {
                                "当前"
                            } else {
                                "历史版本"
                            })
                        ],
                        auxiliary(format!(
                            "技术版本 {} · 质量元数据 {}",
                            label(extract, "technique_version"),
                            data::short(&extract["quality"], 80)
                        )),
                        self.extract_swatches(extract, 22.),
                        text(data::pretty(&extract["content"])).size(13)
                    ]
                    .spacing(8),
                )
                .padding(12)
                .width(Fill)
                .style(style::surface),
            );
        }
        scrollable(container(body).padding(scroll_gutter()).width(Fill))
            .height(Fill)
            .into()
    }
    fn pills(&self) -> Element<'_, Message> {
        if self.data.pills.is_empty() {
            return self.empty(
                "还没有炼制的丹药",
                "先在炼丹台选择素材、提取药液并确定比例。丹药保留冻结规格，导出后可追溯。",
                "前往炼丹台",
                Some(Message::Navigate(Screen::Brew)),
            );
        }
        let pills = self.data.filtered_pills(&self.query);
        if pills.is_empty() {
            return self.empty(
                "没有匹配的丹药",
                "搜索名称、配方或创建时间。",
                "清除搜索",
                Some(Message::ClearSearch),
            );
        }
        let mut body = column![auxiliary(format!(
            "{} 份丹药 · 选择对象查看冻结规格",
            pills.len()
        ))]
        .spacing(12);
        for pill in pills {
            body = body.push(
                button(
                    row![
                        column![
                            text(data::pill_title(pill)).size(14),
                            auxiliary(format!(
                                "{} · {}",
                                label(pill, "recipe"),
                                label(pill, "created_at")
                            ))
                        ]
                        .spacing(5)
                        .width(Fill),
                        auxiliary(if flag(pill, "snapshot") {
                            "规格已冻结"
                        } else {
                            "规格待核对"
                        })
                    ]
                    .spacing(12)
                    .align_y(Alignment::Center),
                )
                .padding(14)
                .width(Fill)
                .on_press(Message::SelectPill(id(pill)))
                .style(if self.pill == Some(id(pill)) {
                    style::selected
                } else {
                    style::normal
                }),
            );
        }
        if let Some(pill) = self.data.pill(self.pill) {
            body = body
                .push(Space::new().height(8))
                .push(line())
                .push(self.pill_summary(pill));
        }
        scrollable(container(body).padding(scroll_gutter()).width(Fill))
            .height(Fill)
            .into()
    }
    fn pill_summary<'a>(&'a self, pill: &'a Value) -> Element<'a, Message> {
        let output = &pill["output"];
        let frozen = flag(pill, "snapshot");
        let mut summary = column![
            row![heading(format!("冻结规格 · #{}", id(pill))).width(Fill), action(if self.show_raw_pill { "收起原始冻结数据" } else { "展开原始冻结数据" }, Some(Message::TogglePillRaw), false)].align_y(Alignment::Center).spacing(12),
            text(if frozen { "已冻结输入与配方。下面只读取这份丹药保存的内容，不追随当前药液变化。" } else { "旧版记录 · 缺少完整冻结快照，历史不可重放。不会从当前药液或配方补造字段，导出已禁用。" }).size(13),
        ].spacing(12);
        if self.show_raw_pill {
            summary = summary.push(
                container(text(data::pretty(output)).size(13))
                    .padding(16)
                    .width(Fill)
                    .style(style::surface),
            );
        }
        for (name, value) in data::recipe_rows(pill) {
            summary = summary.push(detail_row(name, value));
        }
        summary = summary.push(line()).push(heading(if frozen {
            "冻结组件"
        } else {
            "已保存的组件字段"
        }));
        if let Some(components) = output["components"].as_array().filter(|v| !v.is_empty()) {
            for component in components {
                let ratio =
                    data::percent(&component["ratio"]).unwrap_or_else(|| "比例未记录".into());
                let mut detail = column![
                    row![
                        text(format!(
                            "{}{}",
                            dimension(component),
                            if label(component, "slot").is_empty() {
                                String::new()
                            } else {
                                format!(" · {}", label(component, "slot"))
                            }
                        ))
                        .size(14)
                        .width(Fill),
                        text(ratio).size(13)
                    ]
                    .spacing(12),
                    auxiliary(data::component_origin(component)),
                ]
                .spacing(8);
                let material = label(component, "material");
                if !material.is_empty() || component["work_id"].as_i64().is_some() {
                    detail = detail.push(auxiliary(format!(
                        "原素材 {}{}",
                        material,
                        component["work_id"]
                            .as_i64()
                            .map(|n| format!(" · #{n}"))
                            .unwrap_or_default()
                    )));
                }
                let content = &component["content"];
                if label(component, "dimension") == "palette" {
                    let colors = data::palette(content);
                    if colors.is_empty() {
                        detail = detail.push(auxiliary("此记录未保存可解析的配色值。"));
                    } else {
                        detail = detail.push(self.swatches(colors, 32.));
                    }
                    if let Some(colors) = content["colors"].as_array() {
                        let legend: Vec<_> = colors
                            .iter()
                            .filter_map(|color| {
                                color["hex"].as_str().map(|hex| {
                                    format!(
                                        "{hex}{}",
                                        data::percent(&color["share"])
                                            .map(|p| format!(" · {p}"))
                                            .unwrap_or_default()
                                    )
                                })
                            })
                            .collect();
                        if !legend.is_empty() {
                            detail = detail.push(auxiliary(legend.join("     ")));
                        }
                    }
                    for (key, name) in [("bg", "背景"), ("text", "正文"), ("accent", "强调")]
                    {
                        if let Some(color) = content["roles"][key].as_str() {
                            detail = detail.push(detail_row(name, color));
                        }
                    }
                } else if label(component, "dimension") == "spacing" {
                    let rows = data::spacing_rows(content);
                    if rows.is_empty() {
                        detail = detail.push(auxiliary(
                            "此记录未保存结构化间距值，可展开原始数据查看已有字段。",
                        ));
                    }
                    for (name, value) in rows {
                        detail = detail.push(detail_row(name, value));
                    }
                    detail = detail.push(auxiliary("像素留白代理，不代表语义版式判断。"));
                } else {
                    detail = detail.push(auxiliary("此维度的已保存内容可在原始冻结数据中查看。"));
                }
                summary = summary.push(
                    container(detail)
                        .padding(14)
                        .width(Fill)
                        .style(style::surface),
                );
            }
        } else {
            summary = summary.push(text("没有保存组件内容；不以当前库的药液补齐历史。").size(13));
        }
        summary = summary.push(line()).push(heading("规格文字"));
        summary = summary.push(
            text(
                output["prompt"]
                    .as_str()
                    .filter(|p| !p.trim().is_empty())
                    .unwrap_or("未保存规格文字，不从当前配方补写。"),
            )
            .size(13),
        );
        summary.into()
    }
    fn preferences(&self) -> Element<'_, Message> {
        if self.data.experiments.is_empty() {
            return self.empty("还没有可评的视觉实验", "需要已有匿名 A/B 图与完整 manifest，才能记录视觉偏好。文字评价或结构分数不会被当作视觉证据。", "刷新本地实验", self.busy.is_none().then_some(Message::Refresh));
        }
        let experiments = self.data.filtered_experiments(&self.query);
        let Some(experiment) = self.active_experiment() else {
            return self.empty(
                "没有匹配的视觉实验",
                "试试实验编号、比较内容或 manifest；清除搜索可返回已有比较。",
                "清除搜索",
                Some(Message::ClearSearch),
            );
        };
        let active_id = id(experiment);
        let mut chooser = Row::new().spacing(8);
        for experiment in experiments {
            chooser = chooser.push(
                action(
                    format!(
                        "比较 #{}{}",
                        id(experiment),
                        if flag(experiment, "ready") {
                            ""
                        } else {
                            " · 待就绪"
                        }
                    ),
                    Some(Message::SelectExperiment(id(experiment))),
                    false,
                )
                .style(if id(experiment) == active_id {
                    style::selected
                } else {
                    style::normal
                }),
            );
        }
        let mut body = column![
            heading(format!("匿名视觉比较 · #{active_id}")),
            auxiliary("只依据你实际看到的两张图选择；A / B 不显示来源偏向。"),
            scrollable(chooser).direction(scrollable::Direction::Horizontal(
                scrollable::Scrollbar::default()
            )),
            row![
                text_input(
                    if self.effective_demo() {
                        "体验模式不记录真人身份"
                    } else {
                        "person 标识（留空不记 human）"
                    },
                    &self.person
                )
                .on_input(Message::Person)
                .size(13)
                .padding([7, 10])
                .width(Fill),
                auxiliary(format!("source: {}", self.preference_source()))
            ]
            .align_y(Alignment::Center)
            .spacing(12),
        ]
        .spacing(12);
        let toggle = checkbox(self.personal)
            .label("我的私人偏好")
            .size(16)
            .text_size(13);
        body = body.push(
            if !self.effective_demo() && !self.person.trim().is_empty() && self.busy.is_none() {
                toggle.on_toggle(Message::Personal)
            } else {
                toggle
            },
        );
        body = body.push(auxiliary("默认关闭。只有明确身份并主动开启，才表达私有训练意图；未启用或样本不足时保持冷启动，不冒充个人内核。"));
        if !self.effective_demo() && self.person.trim().is_empty() {
            body = body.push(auxiliary(
                "填写稳定的本机 person 标识，或在下方选择已有身份，才能提交；留空不会记为 human。",
            ));
        }
        body = body.push(
            row![
                column![
                    text("A").size(14),
                    self.preview(
                        data::path(experiment, "A"),
                        Fill,
                        if self.width < 1120. { 210. } else { 264. }
                    )
                ]
                .spacing(8)
                .width(FillPortion(1)),
                column![
                    text("B").size(14),
                    self.preview(
                        data::path(experiment, "B"),
                        Fill,
                        if self.width < 1120. { 210. } else { 264. }
                    )
                ]
                .spacing(8)
                .width(FillPortion(1))
            ]
            .spacing(16),
        );
        let enabled = self.can_rate();
        let decision: Element<'_, Message> = if let Some(pending) = &self.confirmation {
            container(
                column![
                    text(&pending.summary).size(13),
                    row![
                        action("取消 · Escape", Some(Message::Cancel), false),
                        action(
                            "确认记录",
                            self.busy.is_none().then_some(Message::ConfirmPreference),
                            true
                        )
                    ]
                    .spacing(8)
                ]
                .spacing(10),
            )
            .padding(12)
            .width(Fill)
            .style(style::inset)
            .into()
        } else {
            row![
                action(
                    "A 更合适",
                    enabled.then_some(Message::Preference("A".into())),
                    false
                )
                .width(FillPortion(1)),
                action(
                    "B 更合适",
                    enabled.then_some(Message::Preference("B".into())),
                    false
                )
                .width(FillPortion(1)),
                action(
                    "平手",
                    enabled.then_some(Message::Preference("tie".into())),
                    false
                )
                .width(FillPortion(1)),
                action(
                    "弃权",
                    enabled.then_some(Message::Preference("abstain".into())),
                    false
                )
                .width(FillPortion(1))
            ]
            .spacing(8)
            .into()
        };
        if !flag(experiment, "ready") {
            body = body.push(
                text("比较尚未就绪：需要完整 manifest 与两张本地图像；不会用文字分替代视觉比较。")
                    .size(13)
                    .style(style::error_text),
            );
        } else if !self.ready_experiment() {
            body = body.push(auxiliary(
                "两张图均实际载入后才能记录。若路径失效，请检查素材根目录并刷新。",
            ));
        }
        body = body.push(line()).push(heading("个人内核状态"));
        if self.data.people.is_empty() {
            body = body.push(
                text("冷启动 · 尚无可用的个人内核。请先明确身份，并积累真实的视觉选择。").size(13),
            );
        }
        for person in &self.data.people {
            body = body.push(
                container(
                    column![
                        row![
                            text(format!(
                                "{} · {}",
                                label(person, "label"),
                                data::short(&person["id"], 40)
                            ))
                            .size(13)
                            .width(Fill),
                            auxiliary(if flag(person, "active") {
                                "已启用"
                            } else {
                                "未启用"
                            }),
                            action(
                                "使用此身份",
                                (!self.effective_demo() && self.busy.is_none())
                                    .then_some(Message::Person(label(person, "id").to_owned())),
                                false
                            )
                        ],
                        auxiliary(format!(
                            "状态 {} · 原始 {} · 可用 {}",
                            label(person, "status"),
                            data::short(&person["raw"], 20),
                            data::short(&person["usable"], 20)
                        ))
                    ]
                    .spacing(6),
                )
                .padding(12)
                .width(Fill)
                .style(style::surface),
            );
        }
        column![
            decision,
            scrollable(container(body).padding(scroll_gutter()).width(Fill)).height(Fill)
        ]
        .spacing(16)
        .into()
    }
    fn inspection(&self) -> Element<'_, Message> {
        let mut body = column![
            row![
                heading("检查").width(Fill),
                action("收起", Some(Message::ToggleInspector), false)
            ]
            .align_y(Alignment::Center)
        ]
        .spacing(16);
        match self.screen {
            Screen::Library | Screen::Brew => {
                if let Some(work) = self.work() {
                    let extracts = self.data.extracts_for(id(work));
                    body = body
                        .push(self.preview(data::path(work, "image_path"), Fill, 196.))
                        .push(heading(data::title(work)))
                        .push(auxiliary(format!(
                            "素材 #{} · {}",
                            id(work),
                            label(work, "collection")
                        )))
                        .push(text(format!("领域  {}", label(work, "domain"))).size(13))
                        .push(line())
                        .push(text("药液维度").size(13));
                    if extracts.is_empty() {
                        body = body.push(auxiliary(
                            "尚未提取。使用上方本地炼药，生成配色与间距药液。",
                        ));
                    }
                    for extract in extracts
                        .iter()
                        .filter(|e| flag(e, "current") || self.data.needs_migration)
                    {
                        body = body.push(
                            column![
                                text(format!("{} · #{}", dimension(extract), id(extract))).size(13),
                                auxiliary(format!(
                                    "技术版本 {}",
                                    label(extract, "technique_version")
                                )),
                                self.extract_swatches(extract, 24.),
                                auxiliary(data::short(&extract["content"], 220))
                            ]
                            .spacing(8),
                        );
                    }
                    body = body.push(
                        action(
                            if self.screen == Screen::Library {
                                "放入炼丹台"
                            } else {
                                "返回素材库"
                            },
                            Some(Message::Navigate(if self.screen == Screen::Library {
                                Screen::Brew
                            } else {
                                Screen::Library
                            })),
                            false,
                        )
                        .width(Fill),
                    );
                    if self.screen == Screen::Library && extracts.len() > 2 {
                        body = body.push(auxiliary(format!(
                            "共 {} 份药液。炼丹台可查看完整内容与历史版本。",
                            extracts.len()
                        )));
                    }
                } else {
                    body = body.push(auxiliary("选择素材后查看图像与药液维度。"));
                }
            }
            Screen::Pills => {
                if let Some(pill) = self.data.pill(self.pill) {
                    body = body
                        .push(heading(data::pill_title(pill)))
                        .push(
                            text(if flag(pill, "snapshot") {
                                "冻结规格 · 已保存"
                            } else {
                                "旧版记录 · 历史不可重放"
                            })
                            .size(13),
                        )
                        .push(auxiliary(format!(
                            "配方 {}\n创建 {}",
                            label(pill, "recipe"),
                            label(pill, "created_at")
                        )))
                        .push(line())
                        .push(text("结构预检总览").size(13));
                    let rows = data::preflight_rows(pill);
                    if rows.is_empty() {
                        body = body.push(auxiliary("尚无结构预检结果；未执行的检查不会记为通过。"));
                    }
                    for (name, value) in rows {
                        body = body.push(detail_row(name, value));
                    }
                    let issues = data::preflight_issues(pill);
                    for issue in issues {
                        body = body.push(text(issue).size(13).style(style::error_text));
                    }
                    body = body.push(auxiliary("只衡量配方合规与材料可用性，不是美感分。结构或文字检查不能替代实际 A/B 视觉比较。"));
                } else {
                    body = body.push(auxiliary("选择一份丹药查看规格与检查状态。"));
                }
            }
            Screen::Preferences => {
                body = body
                    .push(text("记录来源").size(13))
                    .push(auxiliary(format!("source: {}", self.preference_source())))
                    .push(auxiliary(format!(
                        "私有意图：{}",
                        if self.personal {
                            "主动开启"
                        } else {
                            "关闭（默认）"
                        }
                    )))
                    .push(line())
                    .push(text("视觉证据").size(13));
                if let Some(experiment) = self.active_experiment() {
                    body = body
                        .push(auxiliary(format!(
                            "实验 #{}\nmanifest {}",
                            id(experiment),
                            trim(label(experiment, "manifest_hash"), 24)
                        )))
                        .push(auxiliary(if flag(experiment, "formal") {
                            "正式视觉对照"
                        } else {
                            "非正式 / 体验对照"
                        }))
                        .push(text(data::short(&experiment["content_summary"], 600)).size(13));
                }
                body = body.push(line()).push(text("冷启动保护").size(13)).push(auxiliary("未填写 person 时不记 human；体验数据始终记 demo。私人偏好不默认打开，界面不自动拟合、激活或调用模型。"));
            }
        }
        body = body
            .push(Space::new().height(8))
            .push(line())
            .push(auxiliary(format!(
                "数据库版本 {} · {}",
                self.data
                    .schema
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| "未知 / 空".into()),
                if self.writable {
                    "独立工作库"
                } else {
                    "只读"
                }
            )))
            .push(auxiliary("所有数据库、图像解码与动作均在后台执行。"));
        scrollable(container(body).padding(scroll_gutter()).width(Fill))
            .height(Fill)
            .into()
    }
}
