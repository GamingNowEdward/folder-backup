pub mod apply;
pub mod apply_plan;
pub mod error;
pub mod layer;
pub mod platform;
pub mod pop_layer;
pub mod preview;
pub mod project;
pub mod walk;

pub use error::{BackupError, Result};
pub use layer::{LayerMeta, LayerStatus};
pub use project::{ProjectLock, ProjectMeta};
pub use preview::{ApplyPreview, RollbackPreview, PREVIEW_PATH_LIMIT};

/// 进度事件，对应 GUI `progress` 事件载荷 `{ op, stage, done, total }`。
/// `total = 0` 表示该阶段无分母。
#[derive(Debug, Clone, Copy)]
pub struct Progress {
    pub op: &'static str,
    pub stage: &'static str,
    pub done: usize,
    pub total: usize,
}
