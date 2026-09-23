use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, State};

use backup_core::apply::apply_with_plan;
use backup_core::apply_plan::{build_plan_with, ApplyPlan};
use backup_core::layer::delete_layer as engine_delete_layer;
use backup_core::layer::list_layers as engine_list_layers;
use backup_core::layer::LayerMeta;
use backup_core::pop_layer::rollback_top as engine_rollback_top;
use backup_core::preview::{
    preview_rollback as engine_preview_rollback, ApplyPreview, RollbackPreview,
};
use backup_core::project::{
    create_project as engine_create_project, default_backup_root,
    list_projects as engine_list_projects, remove_project as engine_remove_project,
    ProjectLock, ProjectMeta,
};

use crate::error::AppError;

/// GUI 设置（settings.json）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Settings {
    pub backup_root: String,
}

/// 应用全局状态。
pub struct AppState {
    pub settings_path: PathBuf,
    pub backup_root: Mutex<PathBuf>,
}

impl AppState {
    pub fn root(&self) -> PathBuf {
        self.backup_root.lock().unwrap().clone()
    }

    pub fn set_root(&self, path: PathBuf) {
        *self.backup_root.lock().unwrap() = path;
    }
}

fn folder_data_dir() -> PathBuf {
    let root = default_backup_root();
    root.parent().map(Path::to_path_buf).unwrap_or(root)
}

pub fn load_or_default(settings_path: &Path) -> Settings {
    if let Ok(text) = std::fs::read_to_string(settings_path) {
        if let Ok(s) = serde_json::from_str::<Settings>(&text) {
            if !s.backup_root.trim().is_empty() {
                return s;
            }
        }
    }
    Settings {
        backup_root: default_backup_root().to_string_lossy().into_owned(),
    }
}

fn find_project(root: &Path, project_id: &str) -> Result<ProjectMeta, AppError> {
    backup_core::project::find_project_by_id(root, project_id).map_err(AppError::from)
}

fn progress_emitter(app: AppHandle) -> impl FnMut(backup_core::Progress) {
    move |p: backup_core::Progress| {
        let _ = app.emit(
            "progress",
            serde_json::json!({
                "op": p.op,
                "stage": p.stage,
                "done": p.done,
                "total": p.total,
            }),
        );
    }
}

fn scan_emitter(app: AppHandle, op: &'static str) -> impl FnMut(&'static str, usize) {
    move |which, n| {
        let _ = app.emit(
            "progress",
            serde_json::json!({
                "op": op,
                "stage": "扫描",
                "done": n,
                "total": 0,
                "detail": which,
            }),
        );
    }
}

fn join_err<E: std::fmt::Display>(e: E) -> AppError {
    AppError::from(backup_core::BackupError::Other(format!(
        "后台任务线程失败：{e}"
    )))
}

fn build_plan_bg(
    base: PathBuf,
    mod_src: PathBuf,
    app: AppHandle,
    op: &'static str,
) -> Result<ApplyPlan, AppError> {
    build_plan_with(&base, &mod_src, scan_emitter(app, op)).map_err(AppError::from)
}

// ── settings ──────────────────────────────────────────────

#[tauri::command]
pub fn get_settings(state: State<'_, AppState>) -> Result<Settings, AppError> {
    Ok(Settings {
        backup_root: state.root().to_string_lossy().into_owned(),
    })
}

#[tauri::command]
pub fn set_settings(state: State<'_, AppState>, backup_root: String) -> Result<Settings, AppError> {
    let trimmed = backup_root.trim();
    if trimmed.is_empty() {
        return Err(AppError::from(backup_core::BackupError::Other(
            "备份根目录不能为空".to_string(),
        )));
    }
    let path = PathBuf::from(trimmed);
    std::fs::create_dir_all(&path).map_err(backup_core::BackupError::from)?;
    let settings = Settings {
        backup_root: path.to_string_lossy().into_owned(),
    };
    let text =
        serde_json::to_string_pretty(&settings).map_err(backup_core::BackupError::from)?;
    if let Some(parent) = state.settings_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    std::fs::write(&state.settings_path, text).map_err(backup_core::BackupError::from)?;
    state.set_root(path);
    Ok(settings)
}

// ── projects ──────────────────────────────────────────────

#[tauri::command]
pub fn list_projects(state: State<'_, AppState>) -> Result<Vec<ProjectMeta>, AppError> {
    engine_list_projects(&state.root()).map_err(AppError::from)
}

#[tauri::command]
pub fn create_project(
    state: State<'_, AppState>,
    name: String,
    base_dir: String,
) -> Result<ProjectMeta, AppError> {
    engine_create_project(&state.root(), &name, Path::new(&base_dir)).map_err(AppError::from)
}

#[tauri::command]
pub fn remove_project(
    state: State<'_, AppState>,
    project_id: String,
    purge: bool,
) -> Result<(), AppError> {
    let root = state.root();
    let _lock = ProjectLock::try_acquire(&root, &project_id).map_err(AppError::from)?;
    engine_remove_project(&root, &project_id, purge).map_err(AppError::from)?;
    Ok(())
}

// ── apply ─────────────────────────────────────────────────

#[tauri::command]
pub async fn preview_apply(
    app: AppHandle,
    state: State<'_, AppState>,
    project_id: String,
    mod_src: String,
) -> Result<ApplyPreview, AppError> {
    let root = state.root();
    let proj = find_project(&root, &project_id)?;
    let base = PathBuf::from(&proj.base_path);
    let mod_path = PathBuf::from(&mod_src);

    tauri::async_runtime::spawn_blocking(move || {
        let plan = build_plan_bg(base, mod_path, app, "apply")?;
        Ok(ApplyPreview::from_plan(&plan))
    })
    .await
    .map_err(join_err)?
}

#[tauri::command]
pub async fn apply_mod(
    app: AppHandle,
    state: State<'_, AppState>,
    project_id: String,
    mod_src: String,
    note: Option<String>,
) -> Result<LayerMeta, AppError> {
    let root = state.root();
    let proj = find_project(&root, &project_id)?;
    let base = PathBuf::from(&proj.base_path);
    let mod_path = PathBuf::from(&mod_src);
    let pid = project_id.clone();
    let app2 = app.clone();

    tauri::async_runtime::spawn_blocking(move || {
        let plan = build_plan_bg(base, mod_path, app2.clone(), "apply")?;
        let _lock = ProjectLock::try_acquire(&root, &pid).map_err(AppError::from)?;
        apply_with_plan(
            &root,
            &proj,
            &plan,
            note.as_deref(),
            progress_emitter(app2),
        )
        .map_err(AppError::from)
    })
    .await
    .map_err(join_err)?
}

// ── layers / 恢复 ─────────────────────────────────────────

#[tauri::command]
pub fn list_layers(
    state: State<'_, AppState>,
    project_id: String,
) -> Result<Vec<LayerMeta>, AppError> {
    let root = state.root();
    let proj = find_project(&root, &project_id)?;
    engine_list_layers(&root, &proj).map_err(AppError::from)
}

#[tauri::command]
pub async fn preview_rollback(
    state: State<'_, AppState>,
    project_id: String,
) -> Result<RollbackPreview, AppError> {
    let root = state.root();
    let proj = find_project(&root, &project_id)?;
    tauri::async_runtime::spawn_blocking(move || {
        engine_preview_rollback(&root, &proj).map_err(AppError::from)
    })
    .await
    .map_err(join_err)?
}

#[tauri::command]
pub async fn rollback_top(
    app: AppHandle,
    state: State<'_, AppState>,
    project_id: String,
) -> Result<LayerMeta, AppError> {
    let root = state.root();
    let proj = find_project(&root, &project_id)?;
    let pid = project_id.clone();

    tauri::async_runtime::spawn_blocking(move || {
        let _lock = ProjectLock::try_acquire(&root, &pid).map_err(AppError::from)?;
        engine_rollback_top(&root, &proj, progress_emitter(app)).map_err(AppError::from)
    })
    .await
    .map_err(join_err)?
}

#[tauri::command]
pub async fn remove_layer(
    state: State<'_, AppState>,
    project_id: String,
    seq: u32,
) -> Result<LayerMeta, AppError> {
    let root = state.root();
    let proj = find_project(&root, &project_id)?;
    tauri::async_runtime::spawn_blocking(move || {
        let _lock = ProjectLock::try_acquire(&root, &project_id).map_err(AppError::from)?;
        engine_delete_layer(&root, &proj, seq).map_err(AppError::from)
    })
    .await
    .map_err(join_err)?
}

pub fn init_state(settings_path: PathBuf) -> AppState {
    let settings = load_or_default(&settings_path);
    AppState {
        settings_path,
        backup_root: Mutex::new(PathBuf::from(settings.backup_root)),
    }
}

pub fn settings_default_path() -> PathBuf {
    folder_data_dir().join("settings.json")
}
