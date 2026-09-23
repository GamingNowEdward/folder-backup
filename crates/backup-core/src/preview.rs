use std::collections::HashSet;
use std::path::Path;

use serde::Serialize;

use crate::apply_plan::{build_plan, ApplyPlan};
use crate::error::{BackupError, Result};
use crate::layer::{files_dir, layer_dir, stack_top, LayerStatus};
use crate::project::ProjectMeta;
use crate::walk::{scan_fs, scan_tree};

/// 预览路径列表截断上限（CLI / GUI 同步）。
pub const PREVIEW_PATH_LIMIT: usize = 200;

fn truncate(paths: Vec<String>) -> Vec<String> {
    paths.into_iter().take(PREVIEW_PATH_LIMIT).collect()
}

/// 计算恢复时应清理的「本层新增空目录」候选（深度降序）。
///
/// - `dirs_before = Some(before)`：当前目录 − before（精确）
/// - `dirs_before = None`（旧层）：`meta.added` 中每个文件的全部祖先目录 ∩ 当前目录
///   （启发式：底包原有空目录若非本层新增文件的祖先则不会入选）
pub fn dir_cleanup_candidates(
    dirs_before: &Option<Vec<String>>,
    added: &[String],
    current_dirs: &[String],
) -> Vec<String> {
    let current: HashSet<&str> = current_dirs.iter().map(String::as_str).collect();
    let mut candidates: Vec<String> = match dirs_before {
        Some(before) => {
            let before_set: HashSet<&str> = before.iter().map(String::as_str).collect();
            current_dirs
                .iter()
                .filter(|d| !before_set.contains(d.as_str()))
                .cloned()
                .collect()
        }
        None => {
            let mut anc: HashSet<String> = HashSet::new();
            for file in added {
                // 对 "a/b/c/f.txt" 依次生成 "a", "a/b", "a/b/c"
                let mut prefix = String::new();
                let segs: Vec<&str> = file.split('/').collect();
                // 最后一段是文件名，不含目录
                for seg in segs.iter().take(segs.len().saturating_sub(1)) {
                    if prefix.is_empty() {
                        prefix = seg.to_string();
                    } else {
                        prefix.push('/');
                        prefix.push_str(seg);
                    }
                    anc.insert(prefix.clone());
                }
            }
            anc.into_iter()
                .filter(|d| current.contains(d.as_str()))
                .collect()
        }
    };
    // 深度降序：先删 a/b/c 再删 a/b 再删 a
    candidates.sort_by(|a, b| {
        b.matches('/').count().cmp(&a.matches('/').count()).then(b.cmp(a))
    });
    candidates
}

/// `preview_apply` 结果：计数为全量，路径列表截断 200。
#[derive(Debug, Clone, Serialize)]
pub struct ApplyPreview {
    pub overwrite_count: usize,
    pub add_count: usize,
    pub untouched_count: usize,
    pub overwrite_bytes: u64,
    pub add_bytes: u64,
    pub overwrite_paths: Vec<String>,
    pub add_paths: Vec<String>,
}

impl ApplyPreview {
    pub fn from_plan(plan: &ApplyPlan) -> Self {
        Self {
            overwrite_count: plan.overwritten.len(),
            add_count: plan.added.len(),
            untouched_count: plan.untouched_count,
            overwrite_bytes: plan.stats.overwrite_bytes,
            add_bytes: plan.stats.add_bytes,
            overwrite_paths: truncate(plan.overwritten.clone()),
            add_paths: truncate(plan.added.clone()),
        }
    }
}

/// `preview_rollback` 结果：计数为全量，路径列表截断 200。
#[derive(Debug, Clone, Serialize)]
pub struct RollbackPreview {
    pub layer_seq: u32,
    pub layer_id: String,
    pub restore_count: usize,
    pub delete_count: usize,
    /// 将清理的本层新增空目录数（与 pop_layer 实际清理同一算法）
    pub empty_dirs_count: usize,
    pub restore_paths: Vec<String>,
    pub delete_paths: Vec<String>,
    pub empty_dirs: Vec<String>,
}

/// 只读预览 apply：不加锁、零写入。
pub fn preview_apply(base_dir: &Path, mod_src: &Path) -> Result<ApplyPreview> {
    let plan = build_plan(base_dir, mod_src)?;
    Ok(ApplyPreview::from_plan(&plan))
}

/// 只读预览顶层恢复：不加锁、零写入。
pub fn preview_rollback(root: &Path, project: &ProjectMeta) -> Result<RollbackPreview> {
    let top = stack_top(root, project)?.ok_or(BackupError::StackEmpty)?;
    if !matches!(top.status, LayerStatus::Applied | LayerStatus::Restoring) {
        return Err(BackupError::StatusConflict(format!(
            "栈顶层 {} 状态为 {}，不可恢复（仅 applied/restoring 可恢复）",
            top.seq, top.status
        )));
    }

    let ldir = layer_dir(root, project, &top.id);
    let backup_files = scan_tree(&files_dir(&ldir))?;
    let restore_paths: Vec<String> = backup_files.iter().map(|e| e.rel_path.clone()).collect();

    let base_now = scan_tree(Path::new(&project.base_path))?;
    let before: HashSet<&str> = top
        .structure_before
        .iter()
        .map(|e| e.rel_path.as_str())
        .collect();
    let delete_paths: Vec<String> = base_now
        .iter()
        .filter(|e| !before.contains(e.rel_path.as_str()))
        .map(|e| e.rel_path.clone())
        .collect();

    // 将清理的空目录（与 pop_layer 第 3 步同一函数，保证预览=实际）
    let current_dirs = scan_fs(Path::new(&project.base_path))?.dirs;
    let empty_dirs =
        dir_cleanup_candidates(&top.dirs_before, &top.added, &current_dirs);
    let empty_dirs_count = empty_dirs.len();

    Ok(RollbackPreview {
        layer_seq: top.seq,
        layer_id: top.id,
        restore_count: restore_paths.len(),
        delete_count: delete_paths.len(),
        empty_dirs_count,
        restore_paths: truncate(restore_paths),
        delete_paths: truncate(delete_paths),
        empty_dirs: truncate(empty_dirs),
    })
}
