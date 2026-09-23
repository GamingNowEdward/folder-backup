use std::fs::{self, File, OpenOptions};
use std::path::{Path, PathBuf};

use chrono::{DateTime, Local};
use fs2::FileExt;
use serde::{Deserialize, Serialize};

use crate::error::{BackupError, Result};
use crate::platform;

/// 默认备份根：`%LOCALAPPDATA%\FolderBackup\backups`。
pub fn default_backup_root() -> PathBuf {
    if let Some(dir) = std::env::var_os("LOCALAPPDATA") {
        return PathBuf::from(dir).join("FolderBackup").join("backups");
    }
    if let Some(home) = std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME")) {
        return PathBuf::from(home).join("FolderBackup").join("backups");
    }
    PathBuf::from("FolderBackupBackups")
}

/// 单个项目的索引记录。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectMeta {
    pub id: String,
    pub name: String,
    pub base_path: String,
    pub created_at: DateTime<Local>,
}

/// `projects.json` 根对象。
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct ProjectsFile {
    pub projects: Vec<ProjectMeta>,
}

pub fn projects_path(root: &Path) -> PathBuf {
    root.join("projects.json")
}

pub fn project_dir(root: &Path, id: &str) -> PathBuf {
    root.join(id)
}

pub fn layers_dir(root: &Path, id: &str) -> PathBuf {
    project_dir(root, id).join("layers")
}

pub fn lock_path(root: &Path, id: &str) -> PathBuf {
    project_dir(root, id).join(".lock")
}

pub fn load_projects(root: &Path) -> Result<ProjectsFile> {
    let path = projects_path(root);
    if !path.exists() {
        return Ok(ProjectsFile::default());
    }
    let text = fs::read_to_string(platform::to_long_path(&path))?;
    Ok(serde_json::from_str(&text)?)
}

pub fn save_projects(root: &Path, data: &ProjectsFile) -> Result<()> {
    fs::create_dir_all(platform::to_long_path(root))?;
    let text = serde_json::to_string_pretty(data)?;
    platform::write_atomic(&projects_path(root), text.as_bytes())?;
    Ok(())
}

fn new_project_id() -> String {
    let now = Local::now();
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!("{}_{}_{}", now.format("%Y%m%dT%H%M%S"), nanos, std::process::id())
}

/// 创建项目：校验底包存在、项目名唯一，建立 `<id>/layers` 目录。
pub fn create_project(root: &Path, name: &str, base_dir: &Path) -> Result<ProjectMeta> {
    if name.trim().is_empty() {
        return Err(BackupError::Other("项目名不能为空".to_string()));
    }
    if !base_dir.is_dir() {
        return Err(BackupError::Other(format!(
            "底包目录不存在：{}",
            base_dir.display()
        )));
    }
    let canonical = fs::canonicalize(base_dir)?;
    let base_norm = platform::strip_verbatim(&canonical);

    let mut data = load_projects(root)?;
    if data.projects.iter().any(|p| p.name == name) {
        return Err(BackupError::Other(format!("项目名已存在：{name}")));
    }

    let meta = ProjectMeta {
        id: new_project_id(),
        name: name.to_string(),
        base_path: base_norm.to_string_lossy().into_owned(),
        created_at: Local::now(),
    };
    fs::create_dir_all(platform::to_long_path(&layers_dir(root, &meta.id)))?;
    data.projects.push(meta.clone());
    save_projects(root, &data)?;
    Ok(meta)
}

pub fn find_project_by_name(root: &Path, name: &str) -> Result<ProjectMeta> {
    let data = load_projects(root)?;
    data.projects
        .into_iter()
        .find(|p| p.name == name)
        .ok_or_else(|| BackupError::ProjectNotFound(name.to_string()))
}

pub fn find_project_by_id(root: &Path, id: &str) -> Result<ProjectMeta> {
    let data = load_projects(root)?;
    data.projects
        .into_iter()
        .find(|p| p.id == id)
        .ok_or_else(|| BackupError::ProjectNotFound(id.to_string()))
}

pub fn list_projects(root: &Path) -> Result<Vec<ProjectMeta>> {
    Ok(load_projects(root)?.projects)
}

/// 删除项目索引；`purge=true` 时同时删除层数据目录。
/// **假定调用者已持有该项目的 `.lock`。**
pub fn remove_project(root: &Path, id: &str, purge: bool) -> Result<ProjectMeta> {
    let mut data = load_projects(root)?;
    let idx = data
        .projects
        .iter()
        .position(|p| p.id == id)
        .ok_or_else(|| BackupError::ProjectNotFound(id.to_string()))?;
    let removed = data.projects.remove(idx);
    save_projects(root, &data)?;

    if purge {
        let dir = project_dir(root, id);
        match fs::remove_dir_all(platform::to_long_path(&dir)) {
            Ok(()) => {}
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
            Err(err) => return Err(BackupError::Io(err)),
        }
    }
    Ok(removed)
}

/// 项目级跨进程排他锁（fs2）。拿锁失败 → `FileLocked`。
/// 引擎修改类操作在调用前必须先持有它；引擎函数内部不重复加锁。
#[derive(Debug)]
pub struct ProjectLock {
    file: File,
}

impl ProjectLock {
    pub fn try_acquire(root: &Path, project_id: &str) -> Result<Self> {
        let path = lock_path(root, project_id);
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(platform::to_long_path(&path))
            .map_err(|err| BackupError::from_io_at(err, &path))?;
        file.try_lock_exclusive().map_err(|err| {
            if crate::error::is_locked_error(&err) {
                BackupError::FileLocked(vec![path.display().to_string()])
            } else {
                BackupError::Io(err)
            }
        })?;
        Ok(Self { file })
    }
}

impl Drop for ProjectLock {
    fn drop(&mut self) {
        let _ = FileExt::unlock(&self.file);
    }
}
