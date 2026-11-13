use crate::app::{Message, Studio};
use crate::atelier::{self, Context, Effect};
use crate::options::Screen;
use iced::Task;

impl Studio {
    pub(crate) fn atelier_update(&mut self, message: atelier::Message) -> Task<Message> {
        let send = matches!(
            message,
            atelier::Message::Send | atelier::Message::Bind(_, _)
        );
        let can_write = self.can_write();
        let effect = self.atelier.update(
            message,
            Context {
                config: &self.config,
                can_write,
            },
        );
        if send && !self.atelier.error {
            self.screen = Screen::Workshop;
        }
        match effect {
            Effect::None => Task::none(),
            Effect::EditAsset(id) => {
                debug_assert_eq!(self.atelier.selected(), Some(&id));
                self.screen = Screen::Treasury;
                Task::none()
            }
            Effect::Save {
                scope,
                request,
                draft,
            } => {
                let config = scope.config.clone();
                Task::perform(
                    async move {
                        tokio::task::spawn_blocking(move || {
                            crate::atelier_adapter::save(&config, &draft)
                        })
                        .await
                        .map_err(|e| format!("保存任务异常：{e}"))?
                    },
                    move |result| {
                        Message::Atelier(atelier::Message::Saved {
                            scope: scope.clone(),
                            request,
                            result,
                        })
                    },
                )
            }
            Effect::Discover {
                scope,
                request,
                model,
            } => Task::perform(
                async move {
                    tokio::task::spawn_blocking(move || crate::atelier_adapter::discover(&model))
                        .await
                        .map_err(|e| format!("模型检测任务异常：{e}"))?
                },
                move |result| {
                    Message::Atelier(atelier::Message::Models {
                        scope: scope.clone(),
                        request,
                        result,
                    })
                },
            ),
            Effect::Run {
                scope,
                request,
                plan,
                model,
                cancel,
                sender,
            } => {
                let config = scope.config.clone();
                Task::perform(
                    async move {
                        tokio::task::spawn_blocking(move || {
                            crate::atelier_adapter::run(
                                &config,
                                &plan,
                                &model,
                                &cancel,
                                &mut |event| {
                                    let _ = sender.send(event);
                                },
                            )
                        })
                        .await
                        .map_err(|e| format!("运行任务异常：{e}"))?
                    },
                    move |result| {
                        Message::Atelier(atelier::Message::Finished {
                            scope: scope.clone(),
                            request,
                            result,
                        })
                    },
                )
            }
        }
    }
}
