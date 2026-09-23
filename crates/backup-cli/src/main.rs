use std::io::{self, Write};
use std::path::PathBuf;
use std::process::ExitCode;

use backup_core::apply::apply;
use backup_core::error::{BackupError, Result};
use backup_core::layer::{delete_layer, list_layers, stack_top, LayerMeta, LayerStatus};
use backup_core::pop_layer::rollback_top;
use backup_core::preview::{preview_apply, preview_rollback, ApplyPreview, RollbackPreview};
use backup_core::project::{
    create_project, default_backup_root, find_project_by_name, list_projects, remove_project,
    ProjectLock, ProjectMeta,
};
use backup_core::Progress;
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(
    name = "backup-cli",
    version,
    about = "Folder Backup — mod 层栈备份 / 恢复工具",
    long_about = "在把 mod 拷进底包之前，仅备份会被覆盖的原文件并记录结构；多层 mod 栈式管理，LIFO 恢复。"
)]
struct Cli {
    /// 备份根目录（默认 %LOCALAPPDATA%\\FolderBackup\\backups）
    #[arg(long, global = true)]
    root: Option<PathBuf>,

    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// 项目管理
    Project {
        #[command(subcommand)]
        cmd: ProjectCmd,
    },
    /// 预览应用 mod 的影响（只读）
    Preview {
        /// 项目名
        project: String,
        /// mod 源目录
        mod_src: PathBuf,
    },
    /// 备份交集并把 mod 拷入底包（先预览，确认后执行）
    Apply {
        /// 项目名
        project: String,
        /// mod 源目录
        mod_src: PathBuf,
        /// 备注（写入层 meta）
        #[arg(long)]
        note: Option<String>,
        /// 跳过交互确认
        #[arg(long)]
        yes: bool,
    },
    /// 查看项目层栈状态
    Status {
        /// 项目名
        project: String,
    },
    /// 恢复栈顶层（先预览，确认后执行）
    Restore {
        /// 项目名
        project: String,
        /// 跳过交互确认
        #[arg(long)]
        yes: bool,
    },
    /// 层信息
    Layer {
        #[command(subcommand)]
        cmd: LayerCmd,
    },
}

#[derive(Subcommand)]
enum ProjectCmd {
    /// 新建项目（绑定一个底包目录）
    Add {
        /// 项目名（唯一）
        name: String,
        /// 底包目录（游戏本体 folder_base）
        base_dir: PathBuf,
    },
    /// 列出全部项目
    List,
    /// 删除项目（默认只删索引；--purge 才删层数据）
    Rm {
        /// 项目名
        name: String,
        /// 跳过交互确认
        #[arg(long)]
        yes: bool,
        /// 同时删除该层栈全部备份数据
        #[arg(long)]
        purge: bool,
    },
}

#[derive(Subcommand)]
enum LayerCmd {
    /// 查看指定层的 meta.json
    Show {
        /// 项目名
        project: String,
        /// 层序号
        seq: u32,
    },
    /// 删除已恢复（rolled_back）的层目录（含备份文件，不可恢复）
    Rm {
        /// 项目名
        project: String,
        /// 层序号
        seq: u32,
        /// 跳过交互确认
        #[arg(long)]
        yes: bool,
    },
}

fn hint(err: &BackupError) -> &'static str {
    match err {
        BackupError::FileLocked(_) => "请先关闭游戏或其他占用该文件的程序，然后重试。",
        BackupError::StackEmpty => "当前层栈为空，没有可恢复的层。",
        BackupError::ProjectNotFound(_) => "用 `project list` 查看可用项目名。",
        BackupError::InvalidRelPath(_) => "mod 目录内出现非法相对路径，已中止且未写入任何文件。",
        BackupError::ScanFailed(_) => "请检查目录是否可访问、是否包含无法读取的条目。",
        BackupError::Io(_) => "请检查磁盘空间、文件权限与路径长度。",
        BackupError::Json(_) => "备份元数据损坏，请检查对应 meta.json / projects.json。",
        BackupError::LayerNotFound(_) => "用 `status` 查看现有层序号。",
        BackupError::StatusConflict(_) => "仅栈顶 applied 层可恢复、仅已恢复层可删除；可用 `status` 查看状态。",
        BackupError::Other(_) => "",
    }
}

fn format_bytes(n: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut v = n as f64;
    let mut i = 0usize;
    while v >= 1024.0 && i + 1 < UNITS.len() {
        v /= 1024.0;
        i += 1;
    }
    if i == 0 {
        format!("{n} B")
    } else {
        format!("{v:.1} {}", UNITS[i])
    }
}

fn confirm(question: &str) -> Result<bool> {
    print!("{question} [y/N]: ");
    io::stdout().flush()?;
    let mut line = String::new();
    io::stdin().read_line(&mut line)?;
    Ok(matches!(line.trim(), "y" | "Y" | "yes" | "YES"))
}

fn print_apply_preview(p: &ApplyPreview) {
    println!("── 预览：应用 mod ─────────────────────────");
    println!(
        "覆盖 {} 个（{}）／新增 {} 个（{}）／未触及 {} 个",
        p.overwrite_count,
        format_bytes(p.overwrite_bytes),
        p.add_count,
        format_bytes(p.add_bytes),
        p.untouched_count,
    );
    if !p.overwrite_paths.is_empty() {
        println!("将被覆盖（最多展示 {} 条）：", p.overwrite_paths.len());
        for path in &p.overwrite_paths {
            println!("  - {path}");
        }
        if p.overwrite_count > p.overwrite_paths.len() {
            println!("  … 另有 {} 条未展示", p.overwrite_count - p.overwrite_paths.len());
        }
    }
    if !p.add_paths.is_empty() {
        println!("将新增（最多展示 {} 条）：", p.add_paths.len());
        for path in &p.add_paths {
            println!("  - {path}");
        }
        if p.add_count > p.add_paths.len() {
            println!("  … 另有 {} 条未展示", p.add_count - p.add_paths.len());
        }
    }
    println!("────────────────────────────────────────────");
}

fn print_restore_preview(p: &RollbackPreview) {
    println!("── 预览：恢复第 {} 层 ──────────────────────", p.layer_seq);
    println!(
        "将恢复 {} 个文件／将删除 {} 个本层新增文件",
        p.restore_count, p.delete_count
    );
    if p.empty_dirs_count > 0 {
        println!(
            "将清理 {} 个本层新增空目录（最多展示 {} 条）：",
            p.empty_dirs_count,
            p.empty_dirs.len()
        );
        for dir in &p.empty_dirs {
            println!("  - {dir}/");
        }
        if p.empty_dirs_count > p.empty_dirs.len() {
            println!(
                "  … 另有 {} 条未展示",
                p.empty_dirs_count - p.empty_dirs.len()
            );
        }
    }
    if !p.restore_paths.is_empty() {
        println!("将恢复（最多展示 {} 条）：", p.restore_paths.len());
        for path in &p.restore_paths {
            println!("  - {path}");
        }
        if p.restore_count > p.restore_paths.len() {
            println!(
                "  … 另有 {} 条未展示",
                p.restore_count - p.restore_paths.len()
            );
        }
    }
    if !p.delete_paths.is_empty() {
        println!("将删除（最多展示 {} 条）：", p.delete_paths.len());
        for path in &p.delete_paths {
            println!("  - {path}");
        }
        if p.delete_count > p.delete_paths.len() {
            println!("  … 另有 {} 条未展示", p.delete_count - p.delete_paths.len());
        }
    }
    println!("────────────────────────────────────────────");
}

fn make_progress_printer() -> impl FnMut(Progress) {
    let mut last_stage = "";
    move |p: Progress| {
        if p.stage != last_stage {
            println!("▶ {}", p.stage);
            last_stage = p.stage;
        }
        if p.total > 0 && p.done == p.total {
            println!("  ✓ {}（{}/{}）", p.stage, p.done, p.total);
        }
    }
}

fn print_status(proj: &ProjectMeta, layers: &[LayerMeta]) {
    println!("项目：{}（id={}）", proj.name, proj.id);
    println!("底包：{}", proj.base_path);
    if layers.is_empty() {
        println!("层栈：空");
        return;
    }
    let active = layers
        .iter()
        .filter(|m| m.status != LayerStatus::RolledBack)
        .count();
    println!("层栈深度：{active}（共 {} 层，含已恢复）", layers.len());
    println!();
    println!(
        "{:<5} {:<13} {:<20} {:<16} {:>8}",
        "seq", "状态", "时间", "mod", "覆盖/新增"
    );
    for m in layers.iter().rev() {
        println!(
            "{:<5} {:<13} {:<20} {:<16} {:>3}/{:<3}",
            m.seq,
            m.status.to_string(),
            m.created_at.format("%Y-%m-%d %H:%M:%S"),
            m.mod_name,
            m.overwritten.len(),
            m.added.len(),
        );
    }
}

fn run(cli: Cli) -> Result<()> {
    let root = cli.root.unwrap_or_else(default_backup_root);

    match cli.cmd {
        Cmd::Project { cmd } => match cmd {
            ProjectCmd::Add { name, base_dir } => {
                let meta = create_project(&root, &name, &base_dir)?;
                println!("已创建项目：{}（id={}）", meta.name, meta.id);
                println!("底包：{}", meta.base_path);
                println!("备份根：{}", root.display());
            }
            ProjectCmd::List => {
                let projects = list_projects(&root)?;
                if projects.is_empty() {
                    println!("暂无项目。用 `project add <名称> <底包目录>` 创建。");
                    return Ok(());
                }
                println!(
                    "{:<16} {:<34} {:<20} {:>4}",
                    "名称", "底包", "创建时间", "层深"
                );
                for p in &projects {
                    let depth = list_layers(&root, p)?
                        .iter()
                        .filter(|m| m.status != LayerStatus::RolledBack)
                        .count();
                    println!(
                        "{:<16} {:<34} {:<20} {:>4}",
                        p.name,
                        p.base_path,
                        p.created_at.format("%Y-%m-%d %H:%M:%S"),
                        depth,
                    );
                }
            }
            ProjectCmd::Rm { name, yes, purge } => {
                let proj = find_project_by_name(&root, &name)?;
                if purge {
                    println!(
                        "将删除项目「{}」的索引及其全部层数据（{}）。",
                        proj.name,
                        root.join(&proj.id).display()
                    );
                } else {
                    println!("将删除项目「{}」的索引（层数据保留在磁盘）。", proj.name);
                }
                if !yes && !confirm("确认删除？")? {
                    println!("已取消。");
                    return Ok(());
                }
                // 删项目必须先拿项目锁
                let _lock = ProjectLock::try_acquire(&root, &proj.id)?;
                let removed = remove_project(&root, &proj.id, purge)?;
                println!(
                    "已删除项目：{}{}",
                    removed.name,
                    if purge { "（含层数据）" } else { "（仅索引）" }
                );
            }
        },

        Cmd::Preview { project, mod_src } => {
            let proj = find_project_by_name(&root, &project)?;
            let preview = preview_apply(std::path::Path::new(&proj.base_path), &mod_src)?;
            print_apply_preview(&preview);
        }

        Cmd::Apply {
            project,
            mod_src,
            note,
            yes,
        } => {
            let proj = find_project_by_name(&root, &project)?;
            let preview = preview_apply(std::path::Path::new(&proj.base_path), &mod_src)?;
            print_apply_preview(&preview);

            println!("提醒：请先关闭游戏，再继续应用。");
            if !yes && !confirm("确认应用该 mod？")? {
                println!("已取消。");
                return Ok(());
            }

            let prev_top_seq = stack_top(&root, &proj)?.map(|t| t.seq).unwrap_or(0);
            let result = {
                let _lock = ProjectLock::try_acquire(&root, &proj.id)?;
                apply(&root, &proj, &mod_src, note.as_deref(), make_progress_printer())
            };

            match result {
                Ok(meta) => {
                    println!("已应用层 {}（{}）", meta.seq, meta.id);
                    println!(
                        "覆盖 {} 个（{}）／新增 {} 个（{}）",
                        meta.overwritten.len(),
                        format_bytes(meta.stats.overwrite_bytes),
                        meta.added.len(),
                        format_bytes(meta.stats.add_bytes),
                    );
                    println!("备份根：{}", root.display());
                }
                Err(err) => {
                    if let Ok(Some(top)) = stack_top(&root, &proj) {
                        if top.seq > prev_top_seq && top.status == LayerStatus::Applied {
                            eprintln!(
                                "提示：apply 中途失败，层 {} 已记为 applied，可执行 `restore {}` 收敛。",
                                top.seq, proj.name
                            );
                        }
                    }
                    return Err(err);
                }
            }
        }

        Cmd::Status { project } => {
            let proj = find_project_by_name(&root, &project)?;
            let layers = list_layers(&root, &proj)?;
            print_status(&proj, &layers);
        }

        Cmd::Restore { project, yes } => {
            let proj = find_project_by_name(&root, &project)?;
            let preview = preview_rollback(&root, &proj)?;
            print_restore_preview(&preview);

            println!("提醒：请先关闭游戏，再继续恢复。");
            if !yes && !confirm("确认恢复栈顶层？")? {
                println!("已取消。");
                return Ok(());
            }

            let result = {
                let _lock = ProjectLock::try_acquire(&root, &proj.id)?;
                rollback_top(&root, &proj, make_progress_printer())
            };
            let meta = result?;
            println!(
                "已恢复第 {} 层（{}），状态 {}；层目录保留可审计。",
                meta.seq, meta.id, meta.status
            );
        }

        Cmd::Layer { cmd } => match cmd {
            LayerCmd::Show { project, seq } => {
                let proj = find_project_by_name(&root, &project)?;
                let layers = list_layers(&root, &proj)?;
                let meta = layers
                    .iter()
                    .find(|m| m.seq == seq)
                    .ok_or_else(|| BackupError::LayerNotFound(seq.to_string()))?;
                println!("{}", serde_json::to_string_pretty(meta)?);
            }
            LayerCmd::Rm { project, seq, yes } => {
                let proj = find_project_by_name(&root, &project)?;
                let layers = list_layers(&root, &proj)?;
                let meta = layers
                    .iter()
                    .find(|m| m.seq == seq)
                    .ok_or_else(|| BackupError::LayerNotFound(seq.to_string()))?;
                if meta.status != LayerStatus::RolledBack {
                    return Err(BackupError::StatusConflict(format!(
                        "仅「已恢复」的层可删除；第 {seq} 层状态为 {}",
                        meta.status
                    )));
                }
                println!(
                    "将删除已恢复层 #{}（{}），含备份文件，不可恢复。",
                    meta.seq, meta.mod_name
                );
                if !yes && !confirm("确认删除？")? {
                    println!("已取消。");
                    return Ok(());
                }
                let _lock = ProjectLock::try_acquire(&root, &proj.id)?;
                let removed = delete_layer(&root, &proj, seq)?;
                println!(
                    "已删除层 #{}（{}），层目录已清理。",
                    removed.seq, removed.mod_name
                );
            }
        },
    }
    Ok(())
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match run(cli) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("错误：{err}");
            let h = hint(&err);
            if !h.is_empty() {
                eprintln!("建议：{h}");
            }
            ExitCode::FAILURE
        }
    }
}
