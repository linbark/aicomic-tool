//! Behavior tests drive the same public update messages as the native window.
use crate::app::{Dialog, Message, Studio};
use crate::atelier::{self, Field, Kind, RunState, Scope};
use crate::data::Snapshot;
use crate::options::{Options, Screen};
use crate::runtime::Outcome;
use iced::widget::text_editor;
use serde_json::{Value, json};
use std::sync::{Arc, atomic::Ordering};

fn revision(version: u32, prompt: &str) -> Value {
    json!({"kind":"technique","key":"fixture-technique","name":"夹具手法","version":version,"prompt":prompt,"input_type":"any","output_schema":{"type":"object"},"definition":{},"hash":format!("fixture-{version}")})
}
fn app(writable: bool) -> Studio {
    let options = Options::parse(
        ["--db".into(), "fixture.db".into()],
        std::path::Path::new("/isolated-native-fixture"),
    )
    .unwrap();
    let mut app = Studio::new(options).0;
    let config = app.config.clone();
    let rev = revision(1, "保存的 Prompt v1");
    let snapshot = Snapshot::from_value(
        json!({"schema_version":26,"atelier":{"available":true,"assets":[{"kind":"technique","key":"fixture-technique","name":"夹具手法","latest_version":1,"revisions":[rev]}],"runs":[]}}),
    );
    let _ = app.update(Message::Loaded {
        generation: app.generation,
        config: config.clone(),
        result: Ok(Box::new(Outcome {
            config,
            snapshot,
            switched: false,
            note: "isolated metadata fixture".into(),
        })),
    });
    app.writable = writable;
    app
}
fn send(a: &mut Studio, message: atelier::Message) {
    let _ = a.update(Message::Atelier(message));
}
fn scope(a: &Studio) -> Scope {
    Scope {
        generation: a.atelier.generation,
        config: a.config.clone(),
    }
}
fn text_source(a: &mut Studio) {
    send(
        a,
        atelier::Message::Source(text_editor::Action::Edit(text_editor::Edit::Paste(
            Arc::new("真实用户任务文字，不是样例".into()),
        ))),
    );
    send(
        a,
        atelier::Message::Model(atelier::ModelField::Name, "fixture-model".into()),
    );
}
#[test]
fn page_navigation_keeps_catalog_draft_query_and_workshop_configuration() {
    let mut a = app(true);
    let _ = a.update(Message::Navigate(Screen::Treasury));
    send(&mut a, atelier::Message::Query("夹具".into()));
    send(
        &mut a,
        atelier::Message::Edit(Field::Prompt, "尚未保存的草稿".into()),
    );
    let id = a.atelier.selected().cloned();
    let _ = a.update(Message::Navigate(Screen::Workshop));
    text_source(&mut a);
    send(&mut a, atelier::Message::PlanName("用户自建丹方".into()));
    send(&mut a, atelier::Message::SelectNode(0));
    let _ = a.update(Message::Navigate(Screen::Treasury));
    assert_eq!(a.atelier.selected(), id.as_ref());
    assert_eq!(a.atelier.catalog().query, "夹具");
    assert_eq!(a.atelier.draft().unwrap().prompt.text(), "尚未保存的草稿");
    assert!(a.atelier.draft().unwrap().dirty());
    let _ = a.update(Message::Navigate(Screen::Workshop));
    assert_eq!(a.atelier.workshop.name, "用户自建丹方");
    assert_eq!(a.atelier.workshop.model.name, "fixture-model");
    assert!(a.atelier.workshop.source.text().contains("真实用户任务"));
    assert_eq!(a.atelier.workshop.selected, Some(0));
}
#[test]
fn library_save_does_not_rebind_existing_workshop_nodes() {
    let mut a = app(true);
    send(
        &mut a,
        atelier::Message::Bind(revision(1, "不得替代真实版本"), false),
    );
    assert_eq!(
        a.atelier.workshop.nodes[1].revision.as_ref().unwrap()["prompt"],
        "保存的 Prompt v1"
    );
    let _ = a.update(Message::Navigate(Screen::Treasury));
    send(
        &mut a,
        atelier::Message::Edit(Field::Prompt, "第二版".into()),
    );
    send(&mut a, atelier::Message::Save);
    let request = a.atelier.save.as_ref().unwrap().request;
    let scope = scope(&a);
    send(
        &mut a,
        atelier::Message::Saved {
            scope,
            request,
            result: Ok(revision(2, "第二版")),
        },
    );
    assert_eq!(
        a.atelier.workshop.nodes[1].revision.as_ref().unwrap()["version"],
        1
    );
    assert_eq!(
        a.atelier.workshop.nodes[1].revision.as_ref().unwrap()["prompt"],
        "保存的 Prompt v1"
    );
    assert_eq!(a.atelier.assets.values().next().unwrap().len(), 2);
    send(&mut a, atelier::Message::Bind(revision(2, "第二版"), true));
    assert_eq!(
        a.atelier.workshop.nodes[1].revision.as_ref().unwrap()["version"],
        2
    );
}
#[test]
fn save_completion_keeps_newer_edits_and_ignores_old_request_ids() {
    let mut a = app(true);
    send(
        &mut a,
        atelier::Message::Edit(Field::Prompt, "提交的草稿".into()),
    );
    send(&mut a, atelier::Message::Save);
    let request = a.atelier.save.as_ref().unwrap().request;
    let scope = scope(&a);
    send(
        &mut a,
        atelier::Message::Edit(Field::Prompt, "保存期间继续输入".into()),
    );
    send(
        &mut a,
        atelier::Message::Saved {
            scope: scope.clone(),
            request: request + 99,
            result: Ok(revision(99, "过期")),
        },
    );
    assert!(a.atelier.save.is_some());
    send(
        &mut a,
        atelier::Message::Saved {
            scope,
            request,
            result: Ok(revision(2, "提交的草稿")),
        },
    );
    assert_eq!(a.atelier.draft().unwrap().prompt.text(), "保存期间继续输入");
    assert!(a.atelier.draft().unwrap().dirty());
    assert!(a.atelier.save.is_none());
}
#[test]
fn repeated_workshop_save_does_not_replace_the_in_flight_draft() {
    let mut a = app(true);
    send(&mut a, atelier::Message::PlanName("第一次提交".into()));
    send(&mut a, atelier::Message::SavePlan);
    let pending = a.atelier.save.as_ref().unwrap().clone();
    send(
        &mut a,
        atelier::Message::PlanName("保存期间修改装配名".into()),
    );
    send(&mut a, atelier::Message::SavePlan);
    assert_eq!(
        a.atelier.drafts[&pending.id].name, "第一次提交",
        "a duplicate command cannot replace the in-flight asset draft"
    );
    assert_eq!(a.atelier.save.as_ref().unwrap().request, pending.request);
}
#[test]
fn failed_workshop_save_can_retry_corrected_assembly_with_the_same_key() {
    let mut a = app(true);
    send(&mut a, atelier::Message::SavePlan);
    let failed = a.atelier.save.as_ref().unwrap().clone();
    let failed_scope = scope(&a);
    send(
        &mut a,
        atelier::Message::Saved {
            scope: failed_scope.clone(),
            request: failed.request,
            result: Err("DAG 必须包含真实 source 和 export".into()),
        },
    );
    assert!(a.atelier.save.is_none());
    send(&mut a, atelier::Message::AddExport);
    send(&mut a, atelier::Message::SavePlan);
    let retry = a
        .atelier
        .save
        .as_ref()
        .expect("correcting a rejected workshop assembly must allow another save");
    assert_eq!(retry.id, failed.id);
    assert_ne!(retry.request, failed.request);
    let payload = a.atelier.drafts[&retry.id].payload(&retry.id).unwrap();
    assert_eq!(payload["definition"]["nodes"].as_array().unwrap().len(), 2);
    send(
        &mut a,
        atelier::Message::Saved {
            scope: failed_scope,
            request: failed.request,
            result: Err("过期失败不得清除重试".into()),
        },
    );
    assert_ne!(a.atelier.save.as_ref().unwrap().request, failed.request);
}
#[test]
fn workshop_save_preserves_a_manually_created_treasury_draft() {
    let mut a = app(true);
    let _ = a.update(Message::Navigate(Screen::Treasury));
    send(&mut a, atelier::Message::Tab(Kind::Formula));
    send(&mut a, atelier::Message::New);
    let id = a.atelier.selected().unwrap().clone();
    send(
        &mut a,
        atelier::Message::Edit(Field::Name, "藏经阁手工丹方".into()),
    );
    let _ = a.update(Message::Navigate(Screen::Workshop));
    send(&mut a, atelier::Message::FormulaKey(id.key.clone()));
    send(
        &mut a,
        atelier::Message::PlanName("不可覆盖手工丹方".into()),
    );
    send(&mut a, atelier::Message::SavePlan);
    assert!(a.atelier.save.is_none());
    assert!(a.atelier.error);
    assert_eq!(a.atelier.drafts[&id].name, "藏经阁手工丹方");
    assert!(a.atelier.drafts[&id].dirty());
}
#[test]
fn workshop_retry_preserves_treasury_edits_during_or_after_a_failed_save() {
    for edit_during_save in [true, false] {
        let mut a = app(true);
        send(&mut a, atelier::Message::SavePlan);
        let pending = a.atelier.save.as_ref().unwrap().clone();
        let pending_scope = scope(&a);
        for before_failure in [true, false] {
            if before_failure == edit_during_save {
                let _ = a.update(Message::Navigate(Screen::Treasury));
                send(&mut a, atelier::Message::Select(pending.id.clone()));
                send(
                    &mut a,
                    atelier::Message::Editor(
                        Field::Prompt,
                        text_editor::Action::Edit(text_editor::Edit::Paste(Arc::new(
                            "用户实际编辑的丹方草稿".into(),
                        ))),
                    ),
                );
            }
            if before_failure {
                send(
                    &mut a,
                    atelier::Message::Saved {
                        scope: pending_scope.clone(),
                        request: pending.request,
                        result: Err("DAG 必须包含真实 source 和 export".into()),
                    },
                );
            }
        }
        let _ = a.update(Message::Navigate(Screen::Workshop));
        send(&mut a, atelier::Message::AddExport);
        send(&mut a, atelier::Message::SavePlan);
        assert!(a.atelier.save.is_none());
        assert!(a.atelier.error);
        assert_eq!(
            a.atelier.drafts[&pending.id].prompt.text(),
            "用户实际编辑的丹方草稿"
        );
        assert!(a.atelier.drafts[&pending.id].dirty());
    }
}
#[test]
fn original_database_cannot_save_or_start_a_run() {
    let mut a = app(false);
    send(
        &mut a,
        atelier::Message::Edit(Field::Prompt, "可编辑但不写原库".into()),
    );
    send(&mut a, atelier::Message::Save);
    assert!(a.atelier.save.is_none());
    assert!(a.atelier.error);
    text_source(&mut a);
    send(&mut a, atelier::Message::Run);
    assert!(a.atelier.run.is_none());
    assert!(!a.can_write());
    assert!(a.atelier.note.contains("工作副本"));
}
#[test]
fn cancellation_is_requested_not_reported_as_immediate_completion() {
    let mut a = app(true);
    text_source(&mut a);
    send(&mut a, atelier::Message::Run);
    let request = a.atelier.run.as_ref().unwrap().request;
    let scope = scope(&a);
    send(&mut a, atelier::Message::Stop);
    let run = a.atelier.run.as_ref().unwrap();
    assert_eq!(run.state, RunState::StopRequested);
    assert!(run.cancel.load(Ordering::Relaxed));
    assert!(a.atelier.running());
    assert!(!a.can_write());
    let config = a.config.clone();
    let _ = a.update(Message::Dialog(Dialog::Open));
    assert_eq!(a.config.db, config.db);
    assert!(a.busy.is_none());
    let _ = a.update(Message::Navigate(Screen::Treasury));
    assert!(a.atelier.running());
    send(
        &mut a,
        atelier::Message::Finished {
            scope,
            request,
            result: Ok(json!({"id":10,"status":"cancelled","error":"用户停止","output":null})),
        },
    );
    assert!(!a.atelier.running());
    assert_eq!(a.atelier.runs[0]["status"], "cancelled");
}
#[test]
fn database_switch_rejects_prior_save_model_and_run_events() {
    let mut a = app(true);
    text_source(&mut a);
    send(&mut a, atelier::Message::Run);
    let old_request = a.atelier.run.as_ref().unwrap().request;
    let old_scope = scope(&a);
    send(
        &mut a,
        atelier::Message::Finished {
            scope: old_scope.clone(),
            request: old_request,
            result: Err("fixture ended".into()),
        },
    );
    let generation = a.generation;
    let config = a.config.clone();
    let _ = a.update(Message::DialogDone {
        generation,
        config,
        kind: Dialog::Open,
        result: Ok(Some("/isolated-native-fixture/another.db".into())),
    });
    assert!(!a.writable);
    assert!(a.atelier.run.is_none());
    let note = a.atelier.note.clone();
    send(
        &mut a,
        atelier::Message::Event {
            scope: old_scope.clone(),
            request: old_request,
            event: json!({"run_id":1,"node_id":"source","status":"completed"}),
        },
    );
    send(
        &mut a,
        atelier::Message::Finished {
            scope: old_scope.clone(),
            request: old_request,
            result: Ok(json!({"id":1,"status":"completed"})),
        },
    );
    send(
        &mut a,
        atelier::Message::Saved {
            scope: old_scope.clone(),
            request: 1,
            result: Ok(revision(99, "过期保存")),
        },
    );
    send(
        &mut a,
        atelier::Message::Models {
            scope: old_scope,
            request: 1,
            result: Ok(json!({"connected":true,"models":[{"name":"wrong-library"}]})),
        },
    );
    assert!(a.atelier.run.is_none());
    assert!(a.atelier.assets.is_empty());
    assert!(a.atelier.workshop.model.models.is_empty());
    assert_eq!(a.atelier.note, note);
}
#[test]
fn stale_async_results_cannot_clear_the_current_error() {
    let mut a = app(true);
    a.atelier.error = true;
    a.atelier.note = "当前错误必须保留".into();
    let mut old_scope = scope(&a);
    old_scope.generation = old_scope.generation.wrapping_add(99);
    send(
        &mut a,
        atelier::Message::Saved {
            scope: old_scope,
            request: 1,
            result: Ok(revision(99, "过期保存")),
        },
    );
    assert!(
        a.atelier.error,
        "stale results must not clear current error state"
    );
    assert_eq!(a.atelier.note, "当前错误必须保留");
}
#[test]
fn endpoint_edit_rejects_in_flight_model_discovery() {
    let mut a = app(true);
    send(&mut a, atelier::Message::Discover);
    let request = a.atelier.workshop.model.request;
    let scope = scope(&a);
    send(
        &mut a,
        atelier::Message::Model(
            atelier::ModelField::Endpoint,
            "http://localhost:12345".into(),
        ),
    );
    send(
        &mut a,
        atelier::Message::Models {
            scope,
            request,
            result: Ok(json!({"connected":true,"models":[{"name":"stale-model"}]})),
        },
    );
    assert!(a.atelier.workshop.model.models.is_empty());
    assert!(!a.atelier.workshop.model.connection.contains("已连接"));
}
#[test]
fn malformed_advanced_component_remains_recoverable_in_native_form() {
    let mut a = app(true);
    send(&mut a, atelier::Message::Tab(Kind::Recipe));
    send(&mut a, atelier::Message::New);
    send(
        &mut a,
        atelier::Message::Edit(
            Field::Definition,
            r#"{"components":[[]],"constraints":[]}"#.into(),
        ),
    );
    send(
        &mut a,
        atelier::Message::Component(0, "input".into(), "source".into()),
    );
    assert!(
        a.atelier.error,
        "malformed component should report an error, not panic"
    );
}
#[test]
fn advanced_recipe_constraints_are_not_silently_replaced_on_save() {
    let mut a = app(true);
    send(&mut a, atelier::Message::Tab(Kind::Recipe));
    send(&mut a, atelier::Message::New);
    send(
        &mut a,
        atelier::Message::Edit(Field::Name, "自建配方".into()),
    );
    send(
        &mut a,
        atelier::Message::Edit(
            Field::Definition,
            r#"{"components":[{"input":"source","ratio":1}],"constraints":["保留用户约束"]}"#
                .into(),
        ),
    );
    let id = a.atelier.selected().unwrap();
    let payload = a.atelier.draft().unwrap().payload(id).unwrap();
    assert_eq!(
        payload["definition"]["constraints"],
        json!(["保留用户约束"])
    );
}
#[test]
fn atelier_integrity_failure_does_not_hide_legacy_materials_or_offer_false_migration_fix() {
    let mut a = app(true);
    let config = a.config.clone();
    let snapshot = Snapshot::from_value(
        json!({"schema_version":26,"works":[{"id":9,"title":"旧资料仍可读"}],"atelier":{"available":false,"assets":[],"runs":[],"error":"revision hash mismatch","integrity_error":true}}),
    );
    let _ = a.update(Message::Loaded {
        generation: a.generation,
        config: config.clone(),
        result: Ok(Box::new(Outcome {
            config,
            snapshot,
            switched: false,
            note: "isolated integrity fixture".into(),
        })),
    });
    assert_eq!(a.data.works.len(), 1);
    assert_eq!(
        a.atelier.integrity_error.as_deref(),
        Some("revision hash mismatch")
    );
    text_source(&mut a);
    send(&mut a, atelier::Message::Run);
    send(&mut a, atelier::Message::SavePlan);
    assert!(a.atelier.run.is_none());
    assert!(a.atelier.save.is_none());
    assert!(a.atelier.note.contains("完整性错误"));
    assert!(!a.atelier.note.contains("建立工作副本"));
    let _ = a.update(Message::Navigate(Screen::Library));
    assert_eq!(a.data.work(Some(9)).unwrap()["title"], "旧资料仍可读");
}
#[test]
fn historical_restore_creates_draft_without_mutating_saved_revision() {
    let mut a = app(true);
    send(&mut a, atelier::Message::Restore(1));
    send(
        &mut a,
        atelier::Message::Edit(Field::Prompt, "历史恢复后编辑".into()),
    );
    let id = a.atelier.selected().unwrap();
    assert_eq!(
        a.atelier.revision(id, 1).unwrap()["prompt"],
        "保存的 Prompt v1"
    );
    assert!(a.atelier.draft().unwrap().dirty());
}
#[test]
fn formula_loading_requires_confirmation_and_preserves_unrelated_draft() {
    let mut a = app(true);
    send(
        &mut a,
        atelier::Message::Edit(Field::Prompt, "未保存手法".into()),
    );
    let id = a.atelier.selected().cloned().unwrap();
    let formula = json!({"kind":"formula","key":"fixture-formula","version":1,"name":"新丹方","prompt":"","input_type":"any","output_schema":{},"definition":{"nodes":[{"id":"new-source","op":"source","inputs":[]},{"id":"export","op":"export","inputs":["new-source"]}]}});
    a.atelier.assets.insert(
        atelier::AssetId {
            kind: Kind::Formula,
            key: "fixture-formula".into(),
        },
        vec![formula.clone()],
    );
    send(&mut a, atelier::Message::Bind(formula.clone(), false));
    assert_eq!(a.atelier.workshop.nodes[0].id, "source");
    send(&mut a, atelier::Message::ConfirmFormula(false));
    assert_eq!(a.atelier.workshop.nodes[0].id, "source");
    send(&mut a, atelier::Message::Bind(formula, false));
    send(&mut a, atelier::Message::ConfirmFormula(true));
    assert_eq!(a.atelier.workshop.nodes[0].id, "new-source");
    assert_eq!(a.atelier.drafts[&id].prompt.text(), "未保存手法");
}
