use std::fs;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Local};
use serde::{Deserialize, Serialize};

use crate::error::{BackupError, Result};
use crate::platform;
use crate::project::{layers_dir, ProjectMeta};

/// 层状态机。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LayerStatus {
    Creating,
    BackedUp,
    Applied,
    Restoring,
    RolledBack,
}

impl std::fmt::Display for LayerStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            LayerStatus::Creating => "creating",
            LayerStatus::BackedUp => "backed_up",
            LayerStatus::Applied => "applied",
            LayerStatus::Restoring => "restoring",
            LayerStatus::RolledBack => "rolled_back",
        };
        f.write_str(s)
    }
}

/// 应用前结构清单条目。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StructEntry {
    pub rel_path: String,
    pub size: u64,
    pub mtime_ns: i64,
}

/// 统计字节数。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct LayerStats {
    pub overwrite_bytes: u64,
    pub add_bytes: u64,
}

/// 层的唯一事实之一：`meta.json`。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LayerMeta {
    pub seq: u32,
    pub id: String,
    pub created_at: DateTime<Local>,
    pub mod_src: String,
    pub mod_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    pub status: LayerStatus,
    pub structure_before: Vec<StructEntry>,
    /// 应用前底包目录清单；旧层（无此字段）为 None，恢复时跳过目录清理
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dirs_before: Option<Vec<String>>,
    pub overwritten: Vec<String>,
    pub added: Vec<String>,
    pub stats: LayerStats,
}

/// 层目录名：`0001_20260923T110500`。
pub fn layer_dir_name(seq: u32, when: DateTime<Local>) -> String {
    format!("{seq:04}_{}", when.format("%Y%m%dT%H%M%S"))
}

pub fn layer_dir(root: &Path, project: &ProjectMeta, dir_name: &str) -> PathBuf {
    layers_dir(root, &project.id).join(dir_name)
}

pub fn meta_path(layer_dir: &Path) -> PathBuf {
    layer_dir.join("meta.json")
}

pub fn files_dir(layer_dir: &Path) -> PathBuf {
    layer_dir.join("files")
}

pub fn read_meta(layer_dir: &Path) -> Result<LayerMeta> {
    let path = meta_path(layer_dir);
    let text = fs::read_to_string(platform::to_long_path(&path))
        .map_err(|err| BackupError::from_io_at(err, &path))?;
    Ok(serde_json::from_str(&text)?)
}

/// meta.json 原子写：临时文件 + 原子 rename。
pub fn write_meta(layer_dir: &Path, meta: &LayerMeta) -> Result<()> {
    let text = serde_json::to_string_pretty(meta)?;
    platform::write_atomic(&meta_path(layer_dir), text.as_bytes())?;
    Ok(())
}

fn transition_ok(from: LayerStatus, to: LayerStatus) -> bool {
    matches!(
        (from, to),
        (LayerStatus::Creating, LayerStatus::BackedUp)
            | (LayerStatus::BackedUp, LayerStatus::Applied)
            | (LayerStatus::Applied, LayerStatus::Restoring)
            | (LayerStatus::Restoring, LayerStatus::Restoring)
            | (LayerStatus::Restoring, LayerStatus::RolledBack)
    )
}

/// 校验状态机转换并原子写回 meta.json。
pub fn set_status(layer_dir: &Path, meta: &mut LayerMeta, to: LayerStatus) -> Result<()> {
    let from = meta.status;
    if !transition_ok(from, to) {
        return Err(BackupError::StatusConflict(format!(
            "不允许从 {from} 变为 {to}"
        )));
    }
    meta.status = to;
    write_meta(layer_dir, meta)
}

/// 列出项目全部层，按 seq 升序；层目录不存在视为空栈。
pub fn list_layers(root: &Path, project: &ProjectMeta) -> Result<Vec<LayerMeta>> {
    let dir = layers_dir(root, &project.id);
    if !dir.is_dir() {
        return Ok(Vec::new());
    }
    let mut out = Vec::new();
    let entries = fs::read_dir(platform::to_long_path(&dir))?;
    for entry in entries {
        let entry = entry?;
        if !entry.file_type()?.is_dir() {
            continue;
        }
        let path = entry.path();
        match read_meta(&path) {
            Ok(meta) => out.push(meta),
            Err(BackupError::Io(err)) if err.kind() == std::io::ErrorKind::NotFound => continue,
            Err(err) => return Err(err),
        }
    }
    out.sort_by_key(|m| m.seq);
    Ok(out)
}

/// 逻辑栈顶：seq 最大且未 `rolled_back` 的层（回滚后层目录保留但出栈）。
/// 空栈返回 `None`。
pub fn stack_top(root: &Path, project: &ProjectMeta) -> Result<Option<LayerMeta>> {
    Ok(list_layers(root, project)?
        .into_iter()
        .rev()
        .find(|m| m.status != LayerStatus::RolledBack))
}

/// 下一个层序号：基于全部层（含已回滚）的最大 seq + 1，空栈为 1。
pub fn next_seq(layers: &[LayerMeta]) -> u32 {
    layers.iter().map(|m| m.seq).max().unwrap_or(0) + 1
}

/// 删除一个已恢复（`rolled_back`）的层目录（meta.json + files/ 一并清除）。
/// **假定调用者已持有项目 `.lock`。** 不影响活跃栈（rolled_back 已出栈）。
pub fn delete_layer(root: &Path, project: &ProjectMeta, seq: u32) -> Result<LayerMeta> {
    let layers = list_layers(root, project)?;
    let meta = layers
        .into_iter()
        .find(|m| m.seq == seq)
        .ok_or_else(|| BackupError::LayerNotFound(seq.to_string()))?;
    if meta.status != LayerStatus::RolledBack {
        return Err(BackupError::StatusConflict(format!(
            "仅「已恢复」的层可删除；第 {seq} 层状态为 {}",
            meta.status
        )));
    }
    let ldir = layer_dir(root, project, &meta.id);
    fs::remove_dir_all(platform::to_long_path(&ldir)).map_err(|err| {
        BackupError::from_io_at(err, &ldir)
    })?;
    Ok(meta)
}
