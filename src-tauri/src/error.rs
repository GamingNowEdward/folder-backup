use serde::Serialize;

use backup_core::error::BackupError;

/// 统一错误结构（GUI）：按 kind 给中文操作建议。
#[derive(Debug, Serialize)]
pub struct AppError {
    pub message: String,
    pub kind: String,
    pub hint: String,
}

fn kind_and_hint(err: &BackupError) -> (&'static str, &'static str) {
    match err {
        BackupError::FileLocked(_) => (
            "locked",
            "请先关闭游戏或其他占用该文件的程序，然后重试。",
        ),
        BackupError::StackEmpty => ("stack_empty", "当前层栈为空，没有可恢复的层。"),
        BackupError::StatusConflict(_) => (
            "status_conflict",
            "仅栈顶 applied 层可恢复；请刷新列表查看当前状态。",
        ),
        BackupError::ProjectNotFound(_) | BackupError::LayerNotFound(_) => (
            "not_found",
            "对象不存在，可能已被删除；请刷新列表。",
        ),
        BackupError::InvalidRelPath(_) => (
            "invalid_path",
            "mod 目录内出现非法相对路径，已中止且未写入任何文件。",
        ),
        BackupError::ScanFailed(_) => (
            "scan",
            "请检查目录是否可访问、是否包含无法读取的条目。",
        ),
        BackupError::Io(_) => ("io", "请检查磁盘空间、文件权限与路径长度。"),
        BackupError::Json(_) => (
            "json",
            "备份元数据损坏，请检查对应 meta.json / projects.json。",
        ),
        BackupError::Other(_) => ("other", ""),
    }
}

impl From<BackupError> for AppError {
    fn from(err: BackupError) -> Self {
        let (kind, hint) = kind_and_hint(&err);
        AppError {
            message: err.to_string(),
            kind: kind.to_string(),
            hint: hint.to_string(),
        }
    }
}
