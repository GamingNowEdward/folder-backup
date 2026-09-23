use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use crate::error::{BackupError, Result};
use crate::layer::{LayerStats, StructEntry};
use crate::walk::{scan_fs_with, validate_rel_path, FileEntry};

/// 应用计划：两次结构遍历的差分结果，preview 与 apply 共用。
#[derive(Debug, Clone)]
pub struct ApplyPlan {
    pub base_dir: PathBuf,
    pub mod_src: PathBuf,
    /// 应用前底包全量文件清单
    pub structure_before: Vec<StructEntry>,
    /// 应用前底包全量目录清单（恢复时据此清理本层新增空目录）
    pub dirs_before: Vec<String>,
    /// mod 全量文件（按 rel_path 排序）
    pub mod_entries: Vec<FileEntry>,
    /// 将被覆盖（交集）
    pub overwritten: Vec<String>,
    /// 纯新增
    pub added: Vec<String>,
    /// 未触及数量
    pub untouched_count: usize,
    pub stats: LayerStats,
}

/// 对计划内所有相对路径做安全校验（拒绝 `..` 等），防止零写入前就出错。
pub fn validate_plan(plan: &ApplyPlan) -> Result<()> {
    for rel in plan
        .structure_before
        .iter()
        .map(|e| e.rel_path.as_str())
        .chain(plan.overwritten.iter().map(String::as_str))
        .chain(plan.added.iter().map(String::as_str))
        .chain(plan.dirs_before.iter().map(String::as_str))
    {
        validate_rel_path(rel)?;
    }
    for e in &plan.mod_entries {
        validate_rel_path(&e.rel_path)?;
    }
    Ok(())
}

/// 两次结构遍历 + 差分：inter = base ∩ mod，only_mod = mod − base。
pub fn build_plan(base_dir: &Path, mod_src: &Path) -> Result<ApplyPlan> {
    build_plan_with(base_dir, mod_src, |_, _| {})
}

/// 同 `build_plan`；扫描过程回调 `(which, found)`，`which` 为「底包」或「Mod」。
pub fn build_plan_with(
    base_dir: &Path,
    mod_src: &Path,
    mut on_scan: impl FnMut(&'static str, usize),
) -> Result<ApplyPlan> {
    if !base_dir.is_dir() {
        return Err(BackupError::Other(format!(
            "底包目录不存在：{}",
            base_dir.display()
        )));
    }
    if !mod_src.is_dir() {
        return Err(BackupError::Other(format!(
            "mod 源目录不存在：{}",
            mod_src.display()
        )));
    }

    let base_scan = scan_fs_with(base_dir, |n| on_scan("底包", n))?;
    let mod_scan = scan_fs_with(mod_src, |n| on_scan("Mod", n))?;
    let base_entries = base_scan.files;
    let dirs_before = base_scan.dirs;
    let mod_entries = mod_scan.files;
    for e in &mod_entries {
        validate_rel_path(&e.rel_path)?;
    }

    let base_map: HashMap<&str, &FileEntry> = base_entries
        .iter()
        .map(|e| (e.rel_path.as_str(), e))
        .collect();
    let mod_set: HashSet<&str> = mod_entries.iter().map(|e| e.rel_path.as_str()).collect();

    let mut overwritten = Vec::new();
    let mut added = Vec::new();
    let mut overwrite_bytes = 0u64;
    let mut add_bytes = 0u64;

    for e in &mod_entries {
        match base_map.get(e.rel_path.as_str()) {
            Some(base_e) => {
                overwritten.push(e.rel_path.clone());
                overwrite_bytes += base_e.size;
            }
            None => {
                added.push(e.rel_path.clone());
                add_bytes += e.size;
            }
        }
    }

    let untouched_count = base_entries
        .iter()
        .filter(|e| !mod_set.contains(e.rel_path.as_str()))
        .count();

    let structure_before = base_entries
        .into_iter()
        .map(|e| StructEntry {
            rel_path: e.rel_path,
            size: e.size,
            mtime_ns: e.mtime_ns,
        })
        .collect();

    Ok(ApplyPlan {
        base_dir: base_dir.to_path_buf(),
        mod_src: mod_src.to_path_buf(),
        structure_before,
        dirs_before,
        mod_entries,
        overwritten,
        added,
        untouched_count,
        stats: LayerStats {
            overwrite_bytes,
            add_bytes,
        },
    })
}

/// 展示 mod 目录名。
pub fn mod_display_name(mod_src: &Path) -> String {
    mod_src
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| mod_src.to_string_lossy().into_owned())
}
