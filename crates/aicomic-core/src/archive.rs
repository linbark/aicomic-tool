//! Single-file project transfer.
//!
//! An archive is a stored ZIP containing a manifest, a normalized project
//! document and the media files referenced by that document. Import creates a
//! new project identity and rewrites every database id, media path and
//! production-gate episode key so an archive can be imported repeatedly without
//! touching an existing project.

use crate::{
    Asset, AssetItem, AssetItemUpdate, AssetTarget, Episode, EpisodeUpdate, Error, Event,
    EventNode, EventUpdate, Project, Result, Scene, SceneUpdate, Shot, ShotUpdate, Store,
};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::fs::File;
use std::io::{Read, Seek, Write};
use std::path::{Path, PathBuf};
use zip::{CompressionMethod, ZipArchive, ZipWriter, write::SimpleFileOptions};

pub const PROJECT_ARCHIVE_FORMAT: &str = "xingqing.project";
pub const PROJECT_ARCHIVE_VERSION: u32 = 1;
const MANIFEST_ENTRY: &str = "manifest.json";
const PROJECT_ENTRY: &str = "project.json";
const MAX_ARCHIVE_BYTES: u64 = 4 * 1024 * 1024 * 1024;
const MAX_JSON_BYTES: u64 = 64 * 1024 * 1024;
const MAX_MEDIA_BYTES: u64 = 2 * 1024 * 1024 * 1024;
const MAX_ENTRIES: usize = 100_000;
const MAX_WARNINGS: usize = 100;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ArchiveManifest {
    format: String,
    schema_version: u32,
    app_version: String,
    exported_at_ms: u64,
    project_name: String,
    episodes: usize,
    scenes: usize,
    shots: usize,
    asset_items: usize,
    assets: usize,
    media: usize,
    warnings: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ArchiveMedia {
    #[serde(default)]
    asset_id: Option<i64>,
    #[serde(default)]
    shot_id: Option<i64>,
    archive_path: String,
    original_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ProjectArchive {
    schema_version: u32,
    exported_at_ms: u64,
    project: Project,
    #[serde(default)]
    episodes: Vec<Episode>,
    #[serde(default)]
    scenes: Vec<Scene>,
    #[serde(default)]
    shots: Vec<Shot>,
    #[serde(default)]
    asset_items: Vec<AssetItem>,
    #[serde(default)]
    assets: Vec<Asset>,
    #[serde(default)]
    events: Vec<Event>,
    #[serde(default)]
    event_nodes: Vec<EventNode>,
    #[serde(default)]
    contexts: BTreeMap<String, Value>,
    #[serde(default)]
    media: Vec<ArchiveMedia>,
    #[serde(default)]
    warnings: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct ExportSummary {
    pub project_name: String,
    pub episodes: usize,
    pub scenes: usize,
    pub shots: usize,
    pub asset_items: usize,
    pub assets: usize,
    pub media: usize,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct ImportSummary {
    pub project_id: i64,
    pub project_name: String,
    pub episodes: usize,
    pub asset_items: usize,
    pub assets: usize,
    pub media: usize,
    pub warnings: Vec<String>,
}

/// Export one project to a single `.xqproj` file.
///
/// The archive intentionally excludes run history, the memory database and
/// machine-local settings/keys. It contains the creative project and every file
/// needed to continue editing it.
pub fn export_project(store: &Store, project_id: i64, destination: &Path) -> Result<ExportSummary> {
    let project = store.project(project_id)?;
    let mut episodes = vec![];
    let mut scenes = vec![];
    let mut shots = vec![];
    let mut assets: BTreeMap<i64, Asset> = BTreeMap::new();
    for tree in store.script(project_id)? {
        episodes.push(tree.episode);
        for scene_tree in tree.scenes {
            scenes.push(scene_tree.scene);
            for shot_tree in scene_tree.shots {
                for asset in &shot_tree.assets {
                    assets.insert(asset.id, asset.clone());
                }
                shots.push(shot_tree.shot);
            }
        }
    }
    let asset_items = store.asset_items(project_id)?;
    for item in &asset_items {
        for asset in store.assets_for_item(item.id)? {
            assets.insert(asset.id, asset);
        }
    }
    let matrix = store.event_matrix(project_id)?;
    let mut warnings = vec![];
    let contexts = export_contexts(store, project_id, &mut warnings)?;

    let mut media = vec![];
    let mut files: BTreeMap<String, PathBuf> = BTreeMap::new();
    let mut by_original: BTreeMap<String, String> = BTreeMap::new();
    let mut media_index = 0usize;
    let mut media_bytes = 0u64;
    for asset in assets.values() {
        collect_media(
            store,
            &asset.file_path,
            Some(asset.id),
            None,
            &mut media,
            &mut files,
            &mut by_original,
            &mut media_index,
            &mut media_bytes,
            &mut warnings,
        )?;
    }
    for shot in &shots {
        if let Some(path) = &shot.video_path {
            collect_media(
                store,
                path,
                None,
                Some(shot.id),
                &mut media,
                &mut files,
                &mut by_original,
                &mut media_index,
                &mut media_bytes,
                &mut warnings,
            )?;
        }
    }
    let archive = ProjectArchive {
        schema_version: PROJECT_ARCHIVE_VERSION,
        exported_at_ms: now_ms(),
        project: project.clone(),
        episodes,
        scenes,
        shots,
        asset_items,
        assets: assets.into_values().collect(),
        events: matrix.events,
        event_nodes: matrix.nodes,
        contexts,
        media,
        warnings: warnings.clone(),
    };
    let manifest = ArchiveManifest {
        format: PROJECT_ARCHIVE_FORMAT.into(),
        schema_version: PROJECT_ARCHIVE_VERSION,
        app_version: env!("CARGO_PKG_VERSION").into(),
        exported_at_ms: archive.exported_at_ms,
        project_name: project.name.clone(),
        episodes: archive.episodes.len(),
        scenes: archive.scenes.len(),
        shots: archive.shots.len(),
        asset_items: archive.asset_items.len(),
        assets: archive.assets.len(),
        media: archive.media.len(),
        warnings: warnings.len(),
    };
    let parent = destination
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    std::fs::create_dir_all(parent)?;
    let mut temp = tempfile::NamedTempFile::new_in(parent)?;
    {
        let mut zip = ZipWriter::new(temp.as_file_mut());
        write_json_entry(&mut zip, MANIFEST_ENTRY, &manifest)?;
        write_json_entry(&mut zip, PROJECT_ENTRY, &archive)?;
        for (archive_path, source) in &files {
            zip.start_file(archive_path.clone(), zip_options())
                .map_err(zip_error)?;
            let mut source = File::open(source)?;
            std::io::copy(&mut source, &mut zip)?;
        }
        zip.finish().map_err(zip_error)?;
    }
    temp.as_file().sync_all()?;
    if destination.exists() {
        std::fs::remove_file(destination)?;
    }
    temp.persist(destination)
        .map_err(|error| Error::Io(error.error))?;
    Ok(ExportSummary {
        project_name: project.name,
        episodes: manifest.episodes,
        scenes: manifest.scenes,
        shots: manifest.shots,
        asset_items: manifest.asset_items,
        assets: manifest.assets,
        media: manifest.media,
        warnings,
    })
}

/// Import a project archive as a new project.
///
/// The imported project gets a new numeric id and UUID. A duplicate project
/// name is suffixed with `（导入）` so the source and imported copy remain
/// distinguishable. Any failure after the new project row is created attempts
/// to delete the partial project and its media.
pub fn import_project(store: &Store, source: &Path) -> Result<ImportSummary> {
    let metadata = std::fs::metadata(source)?;
    if !metadata.is_file() {
        return Err(Error::Invalid("项目归档不是文件".into()));
    }
    if metadata.len() > MAX_ARCHIVE_BYTES {
        return Err(Error::Invalid("项目归档超过大小上限".into()));
    }
    let file = File::open(source)?;
    let mut zip = ZipArchive::new(file).map_err(zip_error)?;
    if zip.len() > MAX_ENTRIES {
        return Err(Error::Invalid("项目归档条目过多".into()));
    }
    let manifest: ArchiveManifest = read_zip_json(&mut zip, MANIFEST_ENTRY, MAX_JSON_BYTES)?;
    if manifest.format != PROJECT_ARCHIVE_FORMAT {
        return Err(Error::Invalid("不是星晴项目归档".into()));
    }
    if manifest.schema_version != PROJECT_ARCHIVE_VERSION {
        return Err(Error::Invalid(format!(
            "不支持的项目归档版本：{}",
            manifest.schema_version
        )));
    }
    let archive: ProjectArchive = read_zip_json(&mut zip, PROJECT_ENTRY, MAX_JSON_BYTES)?;
    if archive.schema_version != PROJECT_ARCHIVE_VERSION {
        return Err(Error::Invalid(format!(
            "不支持的项目文档版本：{}",
            archive.schema_version
        )));
    }
    validate_manifest(&manifest, &archive)?;
    validate_ids(&archive)?;
    let requested = archive.project.name.trim();
    if requested.is_empty() {
        return Err(Error::Invalid("项目名称不能为空".into()));
    }
    let project_name = unique_project_name(store, requested)?;
    let project = store.create_project(&project_name, archive.project.description.as_deref())?;
    match import_into(store, &mut zip, &archive, project.id) {
        Ok(mut summary) => {
            summary.project_name = project_name;
            Ok(summary)
        }
        Err(error) => {
            let _ = store.delete_project(project.id);
            Err(error)
        }
    }
}

fn import_into(
    store: &Store,
    zip: &mut ZipArchive<File>,
    archive: &ProjectArchive,
    project_id: i64,
) -> Result<ImportSummary> {
    let mut warnings = archive.warnings.clone();
    let mut episode_map = BTreeMap::new();
    let mut episodes = archive.episodes.iter().collect::<Vec<_>>();
    episodes.sort_by_key(|episode| (episode.order, episode.id));
    for old in episodes {
        if old.title.trim().is_empty() {
            return Err(Error::Invalid("分集标题不能为空".into()));
        }
        let new = store.create_episode(project_id, &old.title, old.description.as_deref())?;
        store.patch_episode(
            new.id,
            EpisodeUpdate {
                order: Some(old.order),
                action_text: old.action_text.clone(),
                prompt: old.prompt.clone(),
                script_locked: Some(old.script_locked),
                script_locked_at: old.script_locked_at.clone(),
                last_exec_run_id: None,
                exec_status: Some("idle".into()),
                exec_artifacts: old.exec_artifacts.clone(),
                ..EpisodeUpdate::default()
            },
        )?;
        episode_map.insert(old.id, new.id);
    }

    let mut scene_map = BTreeMap::new();
    let mut scenes = archive.scenes.iter().collect::<Vec<_>>();
    scenes.sort_by_key(|scene| (scene.episode_id, scene.sequence_number, scene.id));
    for old in scenes {
        let episode = episode_map
            .get(&old.episode_id)
            .copied()
            .ok_or_else(|| Error::Invalid("分镜场景引用了不存在的分集".into()))?;
        let new = store.create_scene(episode, &old.title, old.sequence_number)?;
        store.update_scene(
            new.id,
            SceneUpdate {
                title: Some(old.title.clone()),
                sequence_number: Some(old.sequence_number),
                description: old.description.clone(),
                action_text: old.action_text.clone(),
                dialogue: old.dialogue.clone(),
                prompt: old.prompt.clone(),
            },
        )?;
        scene_map.insert(old.id, new.id);
    }

    let mut shot_map = BTreeMap::new();
    let mut shots = archive.shots.iter().collect::<Vec<_>>();
    shots.sort_by_key(|shot| (shot.scene_id, shot.sequence_number, shot.id));
    for old in &shots {
        let scene = scene_map
            .get(&old.scene_id)
            .copied()
            .ok_or_else(|| Error::Invalid("镜头引用了不存在的场景".into()))?;
        let new = store.create_shot(scene, &old.title, old.sequence_number)?;
        store.update_shot(
            new.id,
            ShotUpdate {
                title: Some(old.title.clone()),
                sequence_number: Some(old.sequence_number),
                action_text: old.action_text.clone(),
                dialogue: old.dialogue.clone(),
                prompt: old.prompt.clone(),
                negative_prompt: old.negative_prompt.clone(),
                status: Some(old.status.clone()),
            },
        )?;
        shot_map.insert(old.id, new.id);
    }

    let mut item_map = BTreeMap::new();
    let mut items = archive.asset_items.iter().collect::<Vec<_>>();
    items.sort_by_key(|item| item.id);
    for old in &items {
        if old.name.trim().is_empty() {
            return Err(Error::Invalid("资产名称不能为空".into()));
        }
        let new = store.create_asset_item(
            project_id,
            &old.name,
            &old.category,
            old.description.as_deref(),
        )?;
        store.update_asset_item(
            new.id,
            AssetItemUpdate {
                base_prompt: old.base_prompt.clone(),
                negative_prompt: old.negative_prompt.clone(),
                ..AssetItemUpdate::default()
            },
        )?;
        item_map.insert(old.id, new.id);
    }

    let assets_by_id = archive
        .assets
        .iter()
        .map(|asset| (asset.id, asset))
        .collect::<BTreeMap<_, _>>();
    let mut asset_map = BTreeMap::new();
    let mut media_count = 0usize;
    for entry in &archive.media {
        if let Some(old_asset_id) = entry.asset_id {
            let Some(asset) = assets_by_id.get(&old_asset_id) else {
                push_warning(&mut warnings, format!("跳过不存在的资产 {}", old_asset_id));
                continue;
            };
            let target = if let Some(character) = asset.character_id {
                item_map.get(&character).copied().map(AssetTarget::Item)
            } else if let Some(shot) = asset.shot_id {
                shot_map.get(&shot).copied().map(AssetTarget::Shot)
            } else {
                None
            };
            let Some(target) = target else {
                push_warning(
                    &mut warnings,
                    format!("跳过没有有效归属的资产 {}", old_asset_id),
                );
                continue;
            };
            let temp = extract_media(zip, entry, MAX_MEDIA_BYTES)?;
            let imported = store.import_asset(temp.path(), target)?;
            if asset.is_favorite {
                let _ = store.set_asset_favorite(imported.id, true);
            }
            asset_map.insert(old_asset_id, imported.id);
            media_count += 1;
        } else if let Some(old_shot_id) = entry.shot_id {
            let Some(new_shot) = shot_map.get(&old_shot_id).copied() else {
                push_warning(
                    &mut warnings,
                    format!("跳过没有有效镜头的视频 {}", old_shot_id),
                );
                continue;
            };
            let temp = extract_media(zip, entry, MAX_MEDIA_BYTES)?;
            store.import_shot_video(new_shot, temp.path())?;
            media_count += 1;
        } else {
            push_warning(&mut warnings, "跳过没有目标的项目素材".into());
        }
    }

    for old in &archive.shots {
        let Some(new_shot) = shot_map.get(&old.id).copied() else {
            continue;
        };
        if let Some(old_asset) = old.selected_asset_id
            && let Some(new_asset) = asset_map.get(&old_asset).copied()
        {
            let _ = store.select_shot_asset(new_shot, Some(new_asset));
        }
    }
    for old in &items {
        if let Some(old_asset) = old.avatar_asset_id
            && let (Some(new_item), Some(new_asset)) = (
                item_map.get(&old.id).copied(),
                asset_map.get(&old_asset).copied(),
            )
        {
            let _ = store.set_item_avatar(new_item, Some(new_asset));
        }
    }

    let mut event_map = BTreeMap::new();
    let mut events = archive.events.iter().collect::<Vec<_>>();
    events.sort_by_key(|event| (event.start_time_sort_key, event.id));
    for old in events {
        if old.name.trim().is_empty() {
            return Err(Error::Invalid("事件名称不能为空".into()));
        }
        let new = store.create_event(project_id, &old.name, &old.color)?;
        store.update_event(
            new.id,
            EventUpdate {
                name: Some(old.name.clone()),
                color: Some(old.color.clone()),
                start_time_sort_key: Some(old.start_time_sort_key),
                description: old.description.clone(),
                graph_data: old.graph_data.clone(),
            },
        )?;
        event_map.insert(old.id, new.id);
    }
    for old in &archive.event_nodes {
        let Some(new_event) = event_map.get(&old.event_id).copied() else {
            continue;
        };
        let target = match old.target_type.as_str() {
            "episode" => episode_map.get(&old.target_id).copied(),
            "scene" => scene_map.get(&old.target_id).copied(),
            "shot" => shot_map.get(&old.target_id).copied(),
            _ => None,
        };
        let Some(target) = target else {
            push_warning(
                &mut warnings,
                format!("跳过无法映射的事件关联：{}", old.target_type),
            );
            continue;
        };
        store.upsert_event_node(new_event, &old.target_type, target, &old.description)?;
    }

    let imported_assets = asset_map.len();
    import_contexts(
        store,
        project_id,
        &archive.contexts,
        &episode_map,
        &item_map,
        &mut warnings,
    )?;

    Ok(ImportSummary {
        project_id,
        project_name: archive.project.name.clone(),
        episodes: episode_map.len(),
        asset_items: item_map.len(),
        assets: imported_assets,
        media: media_count,
        warnings,
    })
}

fn import_contexts(
    store: &Store,
    project_id: i64,
    contexts: &BTreeMap<String, Value>,
    episodes: &BTreeMap<i64, i64>,
    items: &BTreeMap<i64, i64>,
    warnings: &mut Vec<String>,
) -> Result<()> {
    let directory = store.paths().context_dir(project_id)?;
    std::fs::create_dir_all(&directory)?;
    for (name, value) in contexts {
        let Some(mapped_name) = map_context_name(name, items) else {
            push_warning(warnings, format!("跳过无法映射的上下文 {name}"));
            continue;
        };
        if crate::paths::component(&mapped_name).is_err() {
            push_warning(warnings, format!("跳过不安全的上下文名称 {name}"));
            continue;
        }
        let mut value = value.clone();
        if mapped_name == "production_state.v1.json" {
            rewrite_gate_episodes(&mut value, episodes);
        }
        crate::paths::write_json(&directory.join(mapped_name), &value)?;
    }
    Ok(())
}

fn map_context_name(name: &str, items: &BTreeMap<i64, i64>) -> Option<String> {
    if let Some(rest) = name.strip_prefix("visual_dna.asset_item_") {
        let (id, suffix) = rest.split_once('.')?;
        let old = id.parse::<i64>().ok()?;
        let new = items.get(&old)?;
        return Some(format!("visual_dna.asset_item_{new}.{suffix}"));
    }
    Some(name.to_owned())
}

fn rewrite_gate_episodes(value: &mut Value, episodes: &BTreeMap<i64, i64>) {
    let Some(map) = value.get_mut("episodes").and_then(Value::as_object_mut) else {
        return;
    };
    let old = std::mem::take(map);
    for (key, gate) in old {
        let Ok(old_id) = key.parse::<i64>() else {
            continue;
        };
        if let Some(new_id) = episodes.get(&old_id) {
            map.insert(new_id.to_string(), gate);
        }
    }
}

fn export_contexts(
    store: &Store,
    project_id: i64,
    warnings: &mut Vec<String>,
) -> Result<BTreeMap<String, Value>> {
    let directory = store.paths().context_dir(project_id)?;
    let mut contexts = BTreeMap::new();
    let Ok(entries) = std::fs::read_dir(&directory) else {
        return Ok(contexts);
    };
    let mut paths = entries
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| path.extension().and_then(|ext| ext.to_str()) == Some("json"))
        .collect::<Vec<_>>();
    paths.sort();
    for path in paths {
        let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        if crate::paths::component(name).is_err() {
            push_warning(warnings, format!("跳过不安全的上下文文件名 {name}"));
            continue;
        }
        if std::fs::metadata(&path).map(|meta| meta.len()).unwrap_or(0) > MAX_JSON_BYTES {
            push_warning(warnings, format!("跳过过大的上下文文件 {name}"));
            continue;
        }
        match crate::paths::read_json(&path) {
            Ok(Some(value)) => {
                contexts.insert(name.to_owned(), value);
            }
            Ok(None) => {}
            Err(error) => push_warning(warnings, format!("跳过无法读取的上下文 {name}: {error}")),
        }
    }
    Ok(contexts)
}

#[allow(clippy::too_many_arguments)]
fn collect_media(
    store: &Store,
    original: &str,
    asset_id: Option<i64>,
    shot_id: Option<i64>,
    media: &mut Vec<ArchiveMedia>,
    files: &mut BTreeMap<String, PathBuf>,
    by_original: &mut BTreeMap<String, String>,
    index: &mut usize,
    total_bytes: &mut u64,
    warnings: &mut Vec<String>,
) -> Result<()> {
    let original = original.trim();
    if original.is_empty() {
        return Ok(());
    }
    if let Some(archive_path) = by_original.get(original) {
        media.push(ArchiveMedia {
            asset_id,
            shot_id,
            archive_path: archive_path.clone(),
            original_path: original.into(),
        });
        return Ok(());
    }
    let source = match store.asset_path(original) {
        Ok(path) => path,
        Err(error) => {
            push_warning(warnings, format!("跳过不安全素材路径 {original}: {error}"));
            return Ok(());
        }
    };
    if !source.is_file() {
        push_warning(warnings, format!("素材文件不存在：{original}"));
        return Ok(());
    }
    let size = std::fs::metadata(&source)?.len();
    *total_bytes = total_bytes.saturating_add(size);
    if *total_bytes > MAX_ARCHIVE_BYTES {
        return Err(Error::Invalid("项目素材超过归档大小上限".into()));
    }
    *index += 1;
    let extension = Path::new(original)
        .extension()
        .and_then(|extension| extension.to_str())
        .filter(|extension| extension.chars().all(|ch| ch.is_ascii_alphanumeric()))
        .map(|extension| extension.to_lowercase())
        .unwrap_or_else(|| "bin".into());
    let archive_path = format!("media/{:05}.{extension}", *index);
    files.insert(archive_path.clone(), source);
    by_original.insert(original.into(), archive_path.clone());
    media.push(ArchiveMedia {
        asset_id,
        shot_id,
        archive_path,
        original_path: original.into(),
    });
    Ok(())
}

fn validate_manifest(manifest: &ArchiveManifest, archive: &ProjectArchive) -> Result<()> {
    if manifest.project_name != archive.project.name {
        return Err(Error::Invalid("归档清单与项目名称不一致".into()));
    }
    if manifest.episodes != archive.episodes.len()
        || manifest.scenes != archive.scenes.len()
        || manifest.shots != archive.shots.len()
        || manifest.asset_items != archive.asset_items.len()
        || manifest.assets != archive.assets.len()
        || manifest.media != archive.media.len()
    {
        return Err(Error::Invalid("归档清单计数与项目内容不一致".into()));
    }
    Ok(())
}

fn validate_ids(archive: &ProjectArchive) -> Result<()> {
    unique_ids(archive.episodes.iter().map(|episode| episode.id), "分集")?;
    unique_ids(archive.scenes.iter().map(|scene| scene.id), "场景")?;
    unique_ids(archive.shots.iter().map(|shot| shot.id), "镜头")?;
    unique_ids(archive.asset_items.iter().map(|item| item.id), "资产条目")?;
    unique_ids(archive.assets.iter().map(|asset| asset.id), "素材")?;
    unique_ids(archive.events.iter().map(|event| event.id), "事件")?;
    unique_ids(archive.event_nodes.iter().map(|node| node.id), "事件关联")?;
    for entry in &archive.media {
        ensure_archive_path(&entry.archive_path)?;
    }
    Ok(())
}

fn unique_ids(ids: impl Iterator<Item = i64>, label: &str) -> Result<()> {
    let mut seen = BTreeSet::new();
    for id in ids {
        if id <= 0 || !seen.insert(id) {
            return Err(Error::Invalid(format!("{label} id 重复或无效")));
        }
    }
    Ok(())
}

fn unique_project_name(store: &Store, requested: &str) -> Result<String> {
    let base = requested.chars().take(120).collect::<String>();
    let projects = store.projects()?;
    if !projects.iter().any(|project| project.name == base) {
        return Ok(base);
    }
    for index in 1..1_000 {
        let name = if index == 1 {
            format!("{base}（导入）")
        } else {
            format!("{base}（导入 {index}）")
        };
        if !projects.iter().any(|project| project.name == name) {
            return Ok(name);
        }
    }
    Err(Error::Invalid("无法为导入项目生成唯一名称".into()))
}

fn extract_media(
    zip: &mut ZipArchive<File>,
    entry: &ArchiveMedia,
    max_bytes: u64,
) -> Result<tempfile::NamedTempFile> {
    ensure_archive_path(&entry.archive_path)?;
    let file = zip.by_name(&entry.archive_path).map_err(zip_error)?;
    if file.size() > max_bytes {
        return Err(Error::Invalid("单个媒体文件超过大小上限".into()));
    }
    let extension = Path::new(&entry.original_path)
        .extension()
        .and_then(|extension| extension.to_str())
        .filter(|extension| extension.chars().all(|ch| ch.is_ascii_alphanumeric()))
        .map(|extension| extension.to_lowercase())
        .unwrap_or_else(|| "bin".into());
    let mut temp = tempfile::Builder::new()
        .suffix(&format!(".{extension}"))
        .tempfile()?;
    let copied = std::io::copy(&mut file.take(max_bytes + 1), temp.as_file_mut())?;
    if copied > max_bytes {
        return Err(Error::Invalid("单个媒体文件超过大小上限".into()));
    }
    temp.as_file_mut().sync_all()?;
    Ok(temp)
}

fn read_zip_json<T: DeserializeOwned>(
    zip: &mut ZipArchive<File>,
    name: &str,
    max_bytes: u64,
) -> Result<T> {
    let mut file = zip.by_name(name).map_err(zip_error)?;
    if file.size() > max_bytes {
        return Err(Error::Invalid(format!("归档文件 {name} 过大")));
    }
    let mut bytes = Vec::new();
    file.by_ref().take(max_bytes + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > max_bytes {
        return Err(Error::Invalid(format!("归档文件 {name} 过大")));
    }
    Ok(serde_json::from_slice(&bytes)?)
}

fn write_json_entry<W: Write + Seek>(
    zip: &mut ZipWriter<W>,
    name: &str,
    value: &impl Serialize,
) -> Result<()> {
    zip.start_file(name.to_owned(), zip_options())
        .map_err(zip_error)?;
    serde_json::to_writer_pretty(&mut *zip, value)?;
    zip.write_all(b"\n")?;
    Ok(())
}

fn zip_options() -> SimpleFileOptions {
    SimpleFileOptions::default().compression_method(CompressionMethod::Stored)
}

fn ensure_archive_path(name: &str) -> Result<()> {
    if name.is_empty()
        || name.starts_with('/')
        || name.contains('\\')
        || name.contains('\0')
        || name
            .split('/')
            .any(|part| part.is_empty() || part == "." || part == "..")
    {
        return Err(Error::Invalid(format!("不安全的归档路径：{name}")));
    }
    Ok(())
}

fn push_warning(warnings: &mut Vec<String>, warning: String) {
    if warnings.len() < MAX_WARNINGS {
        warnings.push(warning);
    }
}

fn zip_error(error: zip::result::ZipError) -> Error {
    Error::Invalid(format!("项目归档格式错误：{error}"))
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn project_round_trip_restores_media_events_and_gate_ids() {
        let source_root = tempfile::tempdir().unwrap();
        let store = Store::open(source_root.path()).unwrap();
        // Shift the source ids so the test also proves id remapping instead of
        // accidentally reusing the destination's first ids.
        let placeholder = store.create_project("占位", None).unwrap();
        store
            .create_episode(placeholder.id, "占位集", None)
            .unwrap();
        store
            .create_asset_item(placeholder.id, "占位资产", "persona_visual", None)
            .unwrap();
        let project = store
            .create_project("测试项目", Some("导入导出测试"))
            .unwrap();
        let episode = store
            .create_episode(
                project.id,
                "第一集",
                Some("第 1 场 · 雨夜\n\n林夏站在月台。"),
            )
            .unwrap();
        store
            .patch_episode(
                episode.id,
                EpisodeUpdate {
                    script_locked: Some(true),
                    exec_artifacts: Some(json!({"shots": [{"title": "雨夜"}]})),
                    ..EpisodeUpdate::default()
                },
            )
            .unwrap();
        let scene = store.create_scene(episode.id, "月台", 1).unwrap();
        let shot = store.create_shot(scene.id, "林夏抬头", 1).unwrap();
        store
            .update_shot(
                shot.id,
                ShotUpdate {
                    action_text: Some("林夏抬头".into()),
                    dialogue: Some("林夏：你来了。".into()),
                    status: Some("approved".into()),
                    ..ShotUpdate::default()
                },
            )
            .unwrap();
        let item = store
            .create_asset_item(project.id, "林夏", "persona_visual", Some("主角"))
            .unwrap();
        let media_source = source_root.path().join("avatar.txt");
        std::fs::write(&media_source, b"avatar-bytes").unwrap();
        let asset = store
            .import_asset(&media_source, AssetTarget::Item(item.id))
            .unwrap();
        store.set_asset_favorite(asset.id, true).unwrap();
        store.set_item_avatar(item.id, Some(asset.id)).unwrap();
        let video_source = source_root.path().join("shot.mp4");
        std::fs::write(&video_source, b"video-bytes").unwrap();
        store.import_shot_video(shot.id, &video_source).unwrap();
        let event = store.create_event(project.id, "重逢", "#E6B36B").unwrap();
        store
            .upsert_event_node(event.id, "scene", scene.id, "在月台重逢")
            .unwrap();
        let contexts = store.paths().context_dir(project.id).unwrap();
        crate::paths::write_json(
            &contexts.join("series_bible.v1.json"),
            &json!({"world": "旧车站"}),
        )
        .unwrap();
        crate::paths::write_json(
            &contexts.join("production_state.v1.json"),
            &json!({
                "schema_version": "production.v1",
                "episodes": { episode.id.to_string(): {"structure_approved": true} }
            }),
        )
        .unwrap();
        crate::paths::write_json(
            &contexts.join(format!("visual_dna.asset_item_{}.v1.json", item.id)),
            &json!({"name": "林夏", "visual_dna": {"hair": "black"}}),
        )
        .unwrap();

        let archive_path = source_root.path().join("测试项目.xqproj");
        let exported = export_project(&store, project.id, &archive_path).unwrap();
        assert_eq!(exported.episodes, 1);
        assert_eq!(exported.asset_items, 1);
        assert_eq!(exported.media, 2);
        assert!(archive_path.is_file());

        let destination_root = tempfile::tempdir().unwrap();
        let destination = Store::open(destination_root.path()).unwrap();
        let imported = import_project(&destination, &archive_path).unwrap();
        assert_eq!(imported.episodes, 1);
        assert_eq!(imported.asset_items, 1);
        assert_eq!(imported.assets, 1);
        assert_eq!(imported.media, 2);

        let script = destination.script(imported.project_id).unwrap();
        assert_eq!(script.len(), 1);
        assert!(script[0].episode.script_locked);
        assert_eq!(script[0].scenes.len(), 1);
        assert_eq!(script[0].scenes[0].shots.len(), 1);
        assert_eq!(
            script[0].scenes[0].shots[0].shot.dialogue.as_deref(),
            Some("林夏：你来了。")
        );
        let video_path = script[0].scenes[0].shots[0]
            .shot
            .video_path
            .as_deref()
            .unwrap();
        assert_eq!(
            std::fs::read(destination.asset_path(video_path).unwrap()).unwrap(),
            b"video-bytes"
        );
        let new_episode = script[0].episode.id;
        let new_scene = script[0].scenes[0].scene.id;
        let new_item = destination.asset_items(imported.project_id).unwrap()[0].clone();
        let new_asset = destination.assets_for_item(new_item.id).unwrap()[0].clone();
        assert!(new_asset.is_favorite);
        assert_eq!(new_item.avatar_asset_id, Some(new_asset.id));
        let copied = destination.asset_path(&new_asset.file_path).unwrap();
        assert_eq!(std::fs::read(copied).unwrap(), b"avatar-bytes");
        let matrix = destination.event_matrix(imported.project_id).unwrap();
        assert_eq!(matrix.events.len(), 1);
        assert_eq!(matrix.nodes.len(), 1);
        assert_eq!(matrix.nodes[0].target_type, "scene");
        assert_eq!(matrix.nodes[0].target_id, new_scene);

        let contexts = destination
            .paths()
            .context_dir(imported.project_id)
            .unwrap();
        let gates = crate::paths::read_json(&contexts.join("production_state.v1.json"))
            .unwrap()
            .unwrap();
        assert!(gates["episodes"][new_episode.to_string()].is_object());
        assert!(gates["episodes"][episode.id.to_string()].is_null());
        assert!(
            contexts
                .join(format!("visual_dna.asset_item_{}.v1.json", new_item.id))
                .is_file()
        );
        assert!(
            crate::paths::read_json(&contexts.join("series_bible.v1.json"))
                .unwrap()
                .is_some()
        );
    }

    #[test]
    fn importing_the_same_archive_twice_creates_distinct_projects() {
        let source_root = tempfile::tempdir().unwrap();
        let store = Store::open(source_root.path()).unwrap();
        let project = store.create_project("同名项目", None).unwrap();
        store.create_episode(project.id, "第一集", None).unwrap();
        let archive = source_root.path().join("same.xqproj");
        export_project(&store, project.id, &archive).unwrap();

        let destination_root = tempfile::tempdir().unwrap();
        let destination = Store::open(destination_root.path()).unwrap();
        let first = import_project(&destination, &archive).unwrap();
        let second = import_project(&destination, &archive).unwrap();
        assert_ne!(first.project_id, second.project_id);
        assert_eq!(first.project_name, "同名项目");
        assert_eq!(second.project_name, "同名项目（导入）");
    }

    #[test]
    fn archive_paths_cannot_escape_the_media_directory() {
        assert!(ensure_archive_path("media/00001.png").is_ok());
        assert!(ensure_archive_path("../media/00001.png").is_err());
        assert!(ensure_archive_path("media/../../secret").is_err());
        assert!(ensure_archive_path("/absolute").is_err());
        assert!(ensure_archive_path("media\\00001.png").is_err());
    }

    #[test]
    fn invalid_archives_do_not_create_projects() {
        let root = tempfile::tempdir().unwrap();
        let store = Store::open(root.path()).unwrap();
        let bad = root.path().join("bad.xqproj");
        std::fs::write(&bad, b"not a zip").unwrap();
        assert!(import_project(&store, &bad).is_err());
        assert!(store.projects().unwrap().is_empty());
    }
}
