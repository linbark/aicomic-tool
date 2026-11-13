//! The only native module that knows the atelier backend transport contract.
use ie::studio::StudioConfig;
use serde_json::Value;
use std::sync::atomic::AtomicBool;

pub fn save(config: &StudioConfig, draft: &Value) -> Result<Value, String> {
    ie::atelier::save_asset(config, draft).map_err(|e| format!("保存未完成：{e:#}"))
}
pub fn discover(model: &Value) -> Result<Value, String> {
    ie::atelier::discover_models(model).map_err(|e| format!("本地服务检测失败：{e:#}"))
}
pub fn run(
    config: &StudioConfig,
    plan: &Value,
    model: &Value,
    cancel: &AtomicBool,
    event: &mut dyn FnMut(Value),
) -> Result<Value, String> {
    ie::atelier::run(config, plan, model, cancel, event).map_err(|e| format!("开炉未完成：{e:#}"))
}
