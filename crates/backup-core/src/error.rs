use thiserror::Error;

#[derive(Debug, Error)]
pub enum BackupError {
    #[error("目录扫描失败：{0}")]
    ScanFailed(String),

    #[error("项目不存在：{0}")]
    ProjectNotFound(String),

    #[error("层不存在：{0}")]
    LayerNotFound(String),

    #[error("层栈为空，无可恢复的层")]
    StackEmpty,

    #[error("状态冲突：{0}")]
    StatusConflict(String),

    #[error("非法相对路径：{0}")]
    InvalidRelPath(String),

    #[error("文件被其他进程占用：{}", .0.join("、"))]
    FileLocked(Vec<String>),

    #[error("IO 错误：{0}")]
    Io(#[from] std::io::Error),

    #[error("JSON 错误：{0}")]
    Json(#[from] serde_json::Error),

    #[error("{0}")]
    Other(String),
}

pub type Result<T> = std::result::Result<T, BackupError>;

/// 是否为文件被占用类错误（ERROR_SHARING_VIOLATION=32 / ERROR_LOCK_VIOLATION=33 / WouldBlock）。
pub fn is_locked_error(err: &std::io::Error) -> bool {
    matches!(err.raw_os_error(), Some(32) | Some(33))
        || err.kind() == std::io::ErrorKind::WouldBlock
}

impl BackupError {
    /// 按 IO 错误上下文分类：文件占用 → `FileLocked`，否则 → `Io`。
    pub fn from_io_at(err: std::io::Error, path: &std::path::Path) -> Self {
        if is_locked_error(&err) {
            BackupError::FileLocked(vec![path.display().to_string()])
        } else {
            BackupError::Io(err)
        }
    }
}
