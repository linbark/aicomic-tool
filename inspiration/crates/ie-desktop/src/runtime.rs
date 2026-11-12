use std::collections::HashMap;
use std::path::{Path, PathBuf};

use iced::widget::image::Handle;
use ie::studio::{StudioAction, StudioConfig};

use crate::data::Snapshot;

#[derive(Clone, Debug)]
pub struct Outcome {
    pub config: StudioConfig,
    pub snapshot: Snapshot,
    pub switched: bool,
    pub note: String,
}
pub fn same_config(a: &StudioConfig, b: &StudioConfig) -> bool {
    a.db == b.db && a.root == b.root && a.recipes == b.recipes
}
pub fn load(config: StudioConfig) -> Result<Outcome, String> {
    if config.db.as_os_str().is_empty() {
        return Ok(Outcome {
            config,
            snapshot: Snapshot::default(),
            switched: false,
            note: "尚未打开数据库。可打开现有库，或明确建立新库。".into(),
        });
    }
    let data = ie::studio::snapshot(&config)
        .map_err(|e| format!("读取失败：{e:#}。原库未迁移；请检查数据库路径和读取权限。"))?;
    let note = if data["missing_database"] == true {
        "指定数据库不存在；没有创建文件。请打开已有库或明确建立新库。"
    } else {
        "本地数据已刷新。"
    };
    Ok(Outcome {
        config,
        snapshot: Snapshot::from_value(data),
        switched: false,
        note: note.into(),
    })
}
pub fn run(config: StudioConfig, action: StudioAction) -> Result<Outcome, String> {
    let value = ie::studio::execute(&config, &action).map_err(|e| format!("操作未完成：{e:#}"))?;
    let mut next = config;
    let switched = match &action {
        StudioAction::CreateCopy { target } => {
            next.db = target.clone();
            true
        }
        StudioAction::CreateEmpty { target } => {
            next.db = target.clone();
            if let Some(root) = value["switch_root"]
                .as_str()
                .map(PathBuf::from)
                .filter(|p| p.is_absolute())
            {
                next.root = root;
            }
            true
        }
        _ => false,
    };
    let note = match &action {
        StudioAction::CreateCopy { .. } => {
            "工作副本已建立。接下来的写入只发生在副本中。".to_owned()
        }
        StudioAction::CreateEmpty { .. } => {
            "新库已建立。可以在这个明确创建的数据库中写入。".to_owned()
        }
        StudioAction::RefineLocal { .. } => {
            "本地炼药完成：仅计算图像像素的 palette + spacing，无模型调用。".to_owned()
        }
        StudioAction::BrewLocal { .. } | StudioAction::BrewRecipe { .. } => {
            "丹药已炼制；打开「丹药」检查冻结规格并导出 brief。".to_owned()
        }
        StudioAction::ExportBrief { .. } => format!(
            "Brief 已导出到 {}（brief.md + tokens.json）",
            value["out_dir"]
                .as_str()
                .unwrap_or("后端返回的独立导出目录")
        ),
        StudioAction::RecordPreference { .. } => preference_note(&value),
    };
    // Refresh is part of the same blocking operation, keeping actions serialized.
    match ie::studio::snapshot(&next) {
        Ok(snapshot) => Ok(Outcome {
            config: next,
            snapshot: Snapshot::from_value(snapshot),
            switched,
            note,
        }),
        Err(error) => Err(format!(
            "操作已执行，但刷新结果失败：{error:#}。请刷新检查，勿盲目重复执行。返回：{}",
            value
        )),
    }
}

fn preference_note(value: &serde_json::Value) -> String {
    // The engine may force demo or only partially complete the personal bridge.
    // Never infer success or source from the requested action.
    let message = value["message"]
        .as_str()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or("视觉选择已记录；请核对返回的来源与个人桥接状态。");
    let mut note = match value["source"].as_str().filter(|s| !s.is_empty()) {
        Some(source) => format!("source={source} · {message}"),
        None => message.to_owned(),
    };
    if let Some(reason) = value["bridge_reason"]
        .as_str()
        .filter(|s| !s.trim().is_empty())
    {
        note.push_str(&format!(
            " 个人偏好桥接未完成：{reason}；本次未拟合或启用个人内核。"
        ));
    }
    note
}

#[derive(Clone, Debug)]
pub struct Asset {
    pub handle: Option<Handle>,
    pub error: Option<String>,
}
pub fn load_images(paths: Vec<PathBuf>) -> HashMap<PathBuf, Asset> {
    paths
        .into_iter()
        .map(|path| {
            let result = decode(&path);
            let asset = match result {
                Ok(handle) => Asset {
                    handle: Some(handle),
                    error: None,
                },
                Err(error) => Asset {
                    handle: None,
                    error: Some(error),
                },
            };
            (path, asset)
        })
        .collect()
}
fn decode(path: &Path) -> Result<Handle, String> {
    let image = image::ImageReader::open(path)
        .map_err(|e| format!("无法打开图片：{e}"))?
        .with_guessed_format()
        .map_err(|e| e.to_string())?
        .decode()
        .map_err(|e| format!("图像解码失败：{e}"))?;
    let rgba = image.thumbnail(720, 720).to_rgba8();
    Ok(Handle::from_rgba(
        rgba.width(),
        rgba.height(),
        rgba.into_raw(),
    ))
}
pub fn save_screenshot(path: &Path, shot: iced::window::Screenshot) -> Result<PathBuf, String> {
    use image::ImageEncoder;
    // create_new is atomic: neither a symlink nor an existing unknown file is overwritten.
    let file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|e| format!("截图未保存（不覆盖已有文件）：{e}"))?;
    image::codecs::png::PngEncoder::new(std::io::BufWriter::new(file))
        .write_image(
            &shot.rgba,
            shot.size.width,
            shot.size.height,
            image::ExtendedColorType::Rgba8,
        )
        .map_err(|e| format!("PNG 编码失败：{e}"))?;
    Ok(path.to_path_buf())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn config_guard_includes_assets_and_recipes() {
        let a = StudioConfig {
            db: "/tmp/a.db".into(),
            root: "/tmp/assets".into(),
            recipes: "/tmp/recipes".into(),
        };
        let mut b = a.clone();
        assert!(same_config(&a, &b));
        b.root = "/tmp/other".into();
        assert!(!same_config(&a, &b));
    }
    #[test]
    fn personal_bridge_failure_retains_backend_message_and_reason() {
        let note = preference_note(
            &serde_json::json!({"message":"视觉评审已记录，但个人偏好桥接未完成","source":"human","bridge_created":false,"bridge_reason":"manifest 缺少可验证的个人候选"}),
        );
        assert!(note.contains("视觉评审已记录，但个人偏好桥接未完成"));
        assert!(note.contains("manifest 缺少可验证的个人候选"));
        assert!(note.contains("未拟合或启用"));
        assert!(!note.contains("私有意图随记录保存"));
    }
    #[test]
    fn preference_feedback_uses_engine_demo_source_not_requested_source() {
        let note = preference_note(
            &serde_json::json!({"message":"已记录 demo，未写入真人偏好","source":"demo","bridge_created":false}),
        );
        assert!(note.starts_with("source=demo"));
        assert!(note.contains("未写入真人偏好"));
        assert!(!note.contains("source=human"));
    }
    #[test]
    fn image_error_is_explicit() {
        let images = load_images(vec![PathBuf::from(
            "/definitely-missing-ie-desktop-fixture.png",
        )]);
        let asset = images.values().next().unwrap();
        assert!(asset.handle.is_none());
        assert!(asset.error.is_some());
    }
}
