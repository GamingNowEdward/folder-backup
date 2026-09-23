use std::collections::HashSet;
use std::fs;
use std::path::Path;

use crate::error::{BackupError, Result};
use crate::layer::{files_dir, layer_dir, set_status, stack_top, LayerMeta, LayerStatus};
use crate::platform;
use crate::preview::dir_cleanup_candidates;
use crate::project::ProjectMeta;
use crate::walk::{scan_fs, scan_tree, validate_rel_path};
use crate::Progress;

fn status_conflict(top: &LayerMeta) -> BackupError {
    BackupError::StatusConflict(format!(
        "栈顶层 {}（{}）状态为 {}，不可恢复（仅 applied/restoring 可恢复）",
        top.seq, top.id, top.status
    ))
}

/// 恢复顶层：`files/` 拷回底包 → 删除本层新增文件 → 清理本层新增空目录 → `rolled_back`。
/// **假定调用者已持有项目 `.lock`。** 层目录保留（可审计）。
///
/// 个别文件失败逐条收集、不强行覆盖；全部成功才推进到 `rolled_back`，
/// 否则停留在 `restoring`（幂等可重入继续）。
/// 旧层无 `dirs_before` 时按 `meta.added` 祖先目录启发式清理（见 `dir_cleanup_candidates`）。
pub fn rollback_top(
    root: &Path,
    project: &ProjectMeta,
    mut on_progress: impl FnMut(Progress),
) -> Result<LayerMeta> {
    let top = stack_top(root, project)?.ok_or(BackupError::StackEmpty)?;

    let ldir = layer_dir(root, project, &top.id);
    let mut meta = top;
    match meta.status {
        LayerStatus::Applied => {
            set_status(&ldir, &mut meta, LayerStatus::Restoring)?;
        }
        LayerStatus::Restoring => {}
        _ => return Err(status_conflict(&meta)),
    }

    // 结构清单路径再校验一次（防御）
    for e in &meta.structure_before {
        validate_rel_path(&e.rel_path)?;
    }

    let mut failures: Vec<String> = Vec::new();

    // 1) 恢复被覆盖的原文件
    let dst_root = Path::new(&project.base_path);
    let backup_files = scan_tree(&files_dir(&ldir))?;
    let restore_total = backup_files.len();
    for (i, e) in backup_files.iter().enumerate() {
        if e.rel_path.is_empty() {
            continue;
        }
        let src = platform::join_rel(&files_dir(&ldir), &e.rel_path);
        let dst = platform::join_rel(dst_root, &e.rel_path);
        if let Some(parent) = dst.parent() {
            if let Err(err) = fs::create_dir_all(platform::to_long_path(parent)) {
                failures.push(format!("{}: {}", e.rel_path, err));
                continue;
            }
        }
        if let Err(err) = platform::copy_atomic(&src, &dst) {
            failures.push(format!("{}: {}", e.rel_path, err));
        }
        on_progress(Progress {
            op: "rollback",
            stage: "恢复文件",
            done: i + 1,
            total: restore_total,
        });
    }

    // 2) 删除本层新增文件 = structure_now − structure_before
    let before: HashSet<&str> = meta
        .structure_before
        .iter()
        .map(|e| e.rel_path.as_str())
        .collect();
    let structure_now = scan_tree(dst_root)?;
    let to_delete: Vec<String> = structure_now
        .iter()
        .filter(|e| !before.contains(e.rel_path.as_str()))
        .map(|e| e.rel_path.clone())
        .collect();
    let delete_total = to_delete.len();
    for (i, rel) in to_delete.iter().enumerate() {
        let p = platform::join_rel(dst_root, rel);
        if let Err(err) = fs::remove_file(platform::to_long_path(&p)) {
            failures.push(format!("{}: {}", rel, err));
        }
        on_progress(Progress {
            op: "rollback",
            stage: "删除新增",
            done: i + 1,
            total: delete_total,
        });
    }

    // 3) 清理本层新增空目录（与 preview 同一函数；旧层用 added 祖先启发式）
    let current_dirs = scan_fs(dst_root)?.dirs;
    let candidates = dir_cleanup_candidates(&meta.dirs_before, &meta.added, &current_dirs);
    let clean_total = candidates.len();
    for (i, rel) in candidates.iter().enumerate() {
        let p = platform::join_rel(dst_root, rel);
        // 仅空目录成功；非空（步骤 2 失败连带 / 用户后放文件）→ 记入失败报告
        let long_p = platform::to_long_path(&p);
        if let Err(err) = fs::remove_dir(&long_p) {
            let not_empty = err.raw_os_error() == Some(41) /* ERROR_DIR_NOT_EMPTY */
                || err.kind() == std::io::ErrorKind::DirectoryNotEmpty;
            if !not_empty {
                // 其他错误（权限/不存在等）：若目录确实为空则无碍；否则上报
                if p.is_dir() && fs::read_dir(&long_p).map(|mut d| d.next().is_some()).unwrap_or(false) {
                    failures.push(format!("{}: {}", rel, err));
                }
            }
            // 非空且无文件级失败 → 尝试 remove_dir_all（候选目录整棵为本层产物）
            else if failures.is_empty() {
                if let Err(err2) = fs::remove_dir_all(&long_p) {
                    failures.push(format!("{}: {}", rel, err2));
                }
            }
        }
        on_progress(Progress {
            op: "rollback",
            stage: "清理目录",
            done: i + 1,
            total: clean_total,
        });
    }

    if !failures.is_empty() {
        // 个别失败：逐条报告，不强行覆盖；留在 restoring 可重入
        let all_locked = failures.iter().all(|f| {
            // 失败串行如 "path: OS error 32 ..."，按内容判断锁错误较难，直接汇总
            f.contains("OS error 32") || f.contains("OS error 33")
        });
        let msg = format!("恢复部分失败（{} 项），已保留 restoring 状态，可关闭占用程序后重试：", failures.len());
        let detail = failures.join("; ");
        return if all_locked {
            Err(BackupError::FileLocked(vec![detail]))
        } else {
            Err(BackupError::Other(format!("{msg}{detail}")))
        };
    }

    on_progress(Progress {
        op: "rollback",
        stage: "完成",
        done: 0,
        total: 0,
    });
    set_status(&ldir, &mut meta, LayerStatus::RolledBack)?;
    Ok(meta)
}
