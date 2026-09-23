use std::fs;
use std::path::Path;

use chrono::Local;

use crate::apply_plan::{build_plan, mod_display_name, validate_plan, ApplyPlan};
use crate::error::{BackupError, Result};
use crate::layer::{
    files_dir, layer_dir, layer_dir_name, list_layers, next_seq, set_status, write_meta,
    LayerMeta, LayerStatus,
};
use crate::platform;
use crate::project::ProjectMeta;
use crate::Progress;

/// 创建层目录并写入 `status=creating` 的 meta。
fn create_layer(
    root: &Path,
    project: &ProjectMeta,
    plan: &ApplyPlan,
    note: Option<&str>,
) -> Result<(LayerMeta, std::path::PathBuf)> {
    let layers = list_layers(root, project)?;
    let seq = next_seq(&layers);
    let now = Local::now();
    let dir_name = layer_dir_name(seq, now);
    let ldir = layer_dir(root, project, &dir_name);

    fs::create_dir_all(platform::to_long_path(&files_dir(&ldir)))?;

    let meta = LayerMeta {
        seq,
        id: dir_name,
        created_at: now,
        mod_src: plan.mod_src.to_string_lossy().into_owned(),
        mod_name: mod_display_name(&plan.mod_src),
        note: note.map(str::to_string),
        status: LayerStatus::Creating,
        structure_before: plan.structure_before.clone(),
        dirs_before: Some(plan.dirs_before.clone()),
        overwritten: plan.overwritten.clone(),
        added: plan.added.clone(),
        stats: plan.stats.clone(),
    };
    write_meta(&ldir, &meta)?;
    Ok((meta, ldir))
}

/// 备份交集原文件到层 `files/`。任意失败 → 严格失败（清理层目录，不拷 mod）。
fn backup_inter(
    plan: &ApplyPlan,
    ldir: &Path,
    mut on_progress: impl FnMut(Progress),
) -> Result<()> {
    let total = plan.overwritten.len();
    if total == 0 {
        return Ok(());
    }

    let dst_root = files_dir(ldir);
    let mut locked: Vec<String> = Vec::new();
    let mut io_err: Option<std::io::Error> = None;

    for (i, rel) in plan.overwritten.iter().enumerate() {
        let src = platform::join_rel(&plan.base_dir, rel);
        let dst = platform::join_rel(&dst_root, rel);
        if let Some(parent) = dst.parent() {
            fs::create_dir_all(platform::to_long_path(parent))?;
        }
        if let Err(err) = platform::copy_atomic(&src, &dst) {
            if crate::error::is_locked_error(&err) {
                locked.push(rel.clone());
            } else if io_err.is_none() {
                io_err = Some(err);
            }
        }
        on_progress(Progress {
            op: "apply",
            stage: "备份交集",
            done: i + 1,
            total,
        });
    }

    if !locked.is_empty() || io_err.is_some() {
        // 严格失败：删除半成品层目录（best effort），不拷 mod
        let _ = fs::remove_dir_all(platform::to_long_path(ldir));
        if !locked.is_empty() {
            return Err(BackupError::FileLocked(locked));
        }
        return Err(BackupError::Io(io_err.expect("io_err checked above")));
    }
    Ok(())
}

/// 把 mod 全量拷入底包；中途失败也要把状态推到 `applied`（保证可回滚收敛）。
fn copy_mod_into_base(plan: &ApplyPlan, mut on_progress: impl FnMut(Progress)) -> Result<()> {
    let total = plan.mod_entries.len();
    if total == 0 {
        return Ok(());
    }

    let mut first_err: Option<BackupError> = None;
    for (i, e) in plan.mod_entries.iter().enumerate() {
        if first_err.is_none() {
            let src = platform::join_rel(&plan.mod_src, &e.rel_path);
            let dst = platform::join_rel(&plan.base_dir, &e.rel_path);
            if let Some(parent) = dst.parent() {
                if let Err(err) = fs::create_dir_all(platform::to_long_path(parent)) {
                    first_err = Some(BackupError::from_io_at(err, &dst));
                }
            }
            if first_err.is_none() {
                if let Err(err) = platform::copy_atomic(&src, &dst) {
                    first_err = Some(BackupError::from_io_at(err, &dst));
                }
            }
        }
        on_progress(Progress {
            op: "apply",
            stage: "拷入 Mod",
            done: i + 1,
            total,
        });
    }

    match first_err {
        Some(err) => Err(err),
        None => Ok(()),
    }
}

/// Apply 管线。**假定调用者已持有项目 `.lock`。**
///
/// 写序：拷交集 → `backed_up` → 拷 mod → `applied`。
/// 备份阶段失败严格终止（清理层、不拷 mod、列出锁定路径）；
/// 拷 mod 阶段失败则先标记 `applied` 再返回错误，保证可回滚收敛。
pub fn apply(
    root: &Path,
    project: &ProjectMeta,
    mod_src: &Path,
    note: Option<&str>,
    on_progress: impl FnMut(Progress),
) -> Result<LayerMeta> {
    let plan = build_plan(&std::path::PathBuf::from(&project.base_path), mod_src)?;
    apply_with_plan(root, project, &plan, note, on_progress)
}

/// 用已计算的计划执行 apply（会先全量校验 rel_path，零写入前拒绝非法路径）。
/// **假定调用者已持有项目 `.lock`。**
pub fn apply_with_plan(
    root: &Path,
    project: &ProjectMeta,
    plan: &ApplyPlan,
    note: Option<&str>,
    mut on_progress: impl FnMut(Progress),
) -> Result<LayerMeta> {
    validate_plan(plan)?;

    on_progress(Progress {
        op: "apply",
        stage: "扫描",
        done: 0,
        total: 0,
    });

    let (mut meta, ldir) = create_layer(root, project, plan, note)?;

    backup_inter(plan, &ldir, &mut on_progress)?;

    on_progress(Progress {
        op: "apply",
        stage: "写入记录",
        done: 0,
        total: 0,
    });
    set_status(&ldir, &mut meta, LayerStatus::BackedUp)?;

    let copy_result = copy_mod_into_base(plan, &mut on_progress);

    // 无论拷 mod 成败，只要有备份就必须推到 applied（部分成功也保证可回滚收敛）。
    // 状态推进失败比拷贝失败更严重（不变式风险），`?` 会优先返回它。
    set_status(&ldir, &mut meta, LayerStatus::Applied)?;

    on_progress(Progress {
        op: "apply",
        stage: "完成",
        done: 0,
        total: 0,
    });

    copy_result?;
    Ok(meta)
}
