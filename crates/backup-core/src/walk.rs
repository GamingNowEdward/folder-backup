use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use jwalk::WalkDir;

use crate::error::{BackupError, Result};

/// 结构遍历收集到的单个文件条目。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileEntry {
    /// 正斜杠相对路径
    pub rel_path: String,
    pub size: u64,
    pub mtime_ns: i64,
}

/// 校验相对路径：拒绝空段、`.`、`..`、绝对路径与非法分隔符。
pub fn validate_rel_path(rel: &str) -> std::result::Result<(), BackupError> {
    fn bad(rel: &str) -> BackupError {
        BackupError::InvalidRelPath(rel.to_string())
    }

    if rel.is_empty() {
        return Err(bad(rel));
    }
    let p = Path::new(rel);
    if p.is_absolute() || p.has_root() {
        return Err(bad(rel));
    }
    for seg in rel.split('/') {
        if seg.is_empty() || seg == "." || seg == ".." {
            return Err(bad(rel));
        }
        if seg.contains('\\') || seg.contains(':') {
            return Err(bad(rel));
        }
    }
    Ok(())
}

fn system_time_to_ns(t: SystemTime) -> i64 {
    match t.duration_since(UNIX_EPOCH) {
        Ok(d) => d.as_nanos() as i64,
        Err(e) => -(e.duration().as_nanos() as i64),
    }
}

fn rel_to_slash(rel: &Path) -> String {
    rel.components()
        .map(|c| c.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}

/// 一趟扫描的完整结果：普通文件 + 目录（均正斜杠 rel、跳符号链接）。
#[derive(Debug, Default)]
pub struct FsScan {
    pub files: Vec<FileEntry>,
    pub dirs: Vec<String>,
}

/// 结构遍历：收集根目录下所有普通文件的 rel_path / size / mtime，跳过符号链接。
/// 错误聚合，任意一条失败则整体 `ScanFailed`。jwalk 传普通路径（不预加 `\\?\`）。
pub fn scan_tree(root: &Path) -> Result<Vec<FileEntry>> {
    scan_tree_with(root, |_| {})
}

/// 同 `scan_tree`，每发现 500 个文件及结束时回调当前计数（供进度展示）。
pub fn scan_tree_with(root: &Path, on_count: impl FnMut(usize)) -> Result<Vec<FileEntry>> {
    Ok(scan_fs_with(root, on_count)?.files)
}

/// 仅收集目录清单（恢复时用于清理本层新增空目录）。
pub fn scan_dirs(root: &Path) -> Result<Vec<String>> {
    Ok(scan_fs(root)?.dirs)
}

/// 同一趟遍历同时收集文件与目录。
pub fn scan_fs(root: &Path) -> Result<FsScan> {
    scan_fs_with(root, |_| {})
}

/// 同 `scan_fs`，每 500 个文件回调一次当前文件计数。
pub fn scan_fs_with(root: &Path, mut on_count: impl FnMut(usize)) -> Result<FsScan> {
    let mut errors: Vec<String> = Vec::new();
    let mut files: Vec<FileEntry> = Vec::new();
    let mut dirs: Vec<String> = Vec::new();

    for entry in WalkDir::new(root).sort(true) {
        let entry = match entry {
            Ok(e) => e,
            Err(err) => {
                errors.push(err.to_string());
                continue;
            }
        };
        if entry.depth() == 0 {
            continue;
        }
        let file_type = entry.file_type();
        if file_type.is_symlink() {
            continue;
        }

        let path = entry.path();
        let rel = match path.strip_prefix(root) {
            Ok(r) => r,
            Err(err) => {
                errors.push(format!("{}: {}", path.display(), err));
                continue;
            }
        };
        let rel_path = rel_to_slash(rel);
        if let Err(err) = validate_rel_path(&rel_path) {
            errors.push(err.to_string());
            continue;
        }

        if file_type.is_dir() {
            dirs.push(rel_path);
            continue;
        }
        if !file_type.is_file() {
            continue;
        }

        let metadata = match entry.metadata() {
            Ok(m) => m,
            Err(err) => {
                errors.push(format!("{}: {}", path.display(), err));
                continue;
            }
        };
        let mtime_ns = match metadata.modified() {
            Ok(t) => system_time_to_ns(t),
            Err(err) => {
                errors.push(format!("{}: {}", path.display(), err));
                continue;
            }
        };

        files.push(FileEntry {
            rel_path,
            size: metadata.len(),
            mtime_ns,
        });
        if files.len().is_multiple_of(500) {
            on_count(files.len());
        }
    }

    if !errors.is_empty() {
        return Err(BackupError::ScanFailed(errors.join("; ")));
    }
    on_count(files.len());
    files.sort_by(|a, b| a.rel_path.cmp(&b.rel_path));
    dirs.sort();
    Ok(FsScan { files, dirs })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_accepts_normal_paths() {
        assert!(validate_rel_path("data/game.ini").is_ok());
        assert!(validate_rel_path("a/b/c/d.txt").is_ok());
        assert!(validate_rel_path("file with space.txt").is_ok());
    }

    #[test]
    fn validate_rejects_dangerous_paths() {
        assert!(matches!(
            validate_rel_path("../evil"),
            Err(BackupError::InvalidRelPath(_))
        ));
        assert!(matches!(
            validate_rel_path("a/../b"),
            Err(BackupError::InvalidRelPath(_))
        ));
        assert!(matches!(
            validate_rel_path("./x"),
            Err(BackupError::InvalidRelPath(_))
        ));
        assert!(matches!(
            validate_rel_path("a//b"),
            Err(BackupError::InvalidRelPath(_))
        ));
        assert!(matches!(
            validate_rel_path(""),
            Err(BackupError::InvalidRelPath(_))
        ));
        assert!(matches!(
            validate_rel_path("/abs/path"),
            Err(BackupError::InvalidRelPath(_))
        ));
        assert!(matches!(
            validate_rel_path("C:/win/system.ini"),
            Err(BackupError::InvalidRelPath(_))
        ));
        assert!(matches!(
            validate_rel_path(r"a\b"),
            Err(BackupError::InvalidRelPath(_))
        ));
    }
}
