#[cfg(windows)]
use std::ffi::OsString;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

#[cfg(windows)]
fn wide_null(path: &Path) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt;
    path.as_os_str().encode_wide().chain(std::iter::once(0)).collect()
}

#[cfg(windows)]
fn to_long_path_inner(path: &Path) -> PathBuf {
    use std::os::windows::ffi::{OsStrExt, OsStringExt};

    const SLASH: u16 = 0x005C;
    const Q: u16 = 0x003F;
    // \\?\
    const VERBATIM: [u16; 4] = [SLASH, SLASH, Q, SLASH];
    // \\?\UNC\
    const UNC_VERBATIM: [u16; 8] = [SLASH, SLASH, Q, SLASH, 0x0055, 0x004E, 0x0043, SLASH];

    let orig: Vec<u16> = path.as_os_str().encode_wide().collect();
    if orig.len() >= 4 && orig[..4] == VERBATIM {
        return path.to_path_buf();
    }
    if orig.len() >= 2 && orig[0] == SLASH && orig[1] == SLASH {
        let mut w = Vec::with_capacity(orig.len() + 6);
        w.extend_from_slice(&UNC_VERBATIM);
        w.extend_from_slice(&orig[2..]);
        return PathBuf::from(OsString::from_wide(&w));
    }
    if path.is_absolute() {
        let mut w = Vec::with_capacity(orig.len() + 4);
        w.extend_from_slice(&VERBATIM);
        w.extend_from_slice(&orig);
        return PathBuf::from(OsString::from_wide(&w));
    }
    path.to_path_buf()
}

#[cfg(windows)]
fn strip_verbatim_inner(path: &Path) -> PathBuf {
    use std::os::windows::ffi::{OsStrExt, OsStringExt};

    const SLASH: u16 = 0x005C;
    const Q: u16 = 0x003F;
    const VERBATIM: [u16; 4] = [SLASH, SLASH, Q, SLASH];
    const UNC_VERBATIM: [u16; 8] = [SLASH, SLASH, Q, SLASH, 0x0055, 0x004E, 0x0043, SLASH];

    let orig: Vec<u16> = path.as_os_str().encode_wide().collect();
    if orig.len() >= 8 && orig[..8] == UNC_VERBATIM {
        let mut w = Vec::with_capacity(orig.len() - 8 + 2);
        w.push(SLASH);
        w.push(SLASH);
        w.extend_from_slice(&orig[8..]);
        return PathBuf::from(OsString::from_wide(&w));
    }
    if orig.len() >= 4 && orig[..4] == VERBATIM {
        return PathBuf::from(OsString::from_wide(&orig[4..]));
    }
    path.to_path_buf()
}

/// 将绝对路径转换为 `\\?\` 长路径形式（UNC 转 `\\?\UNC\`）。
/// 已带前缀或相对路径原样返回；非 Windows 平台恒等。
pub fn to_long_path(path: &Path) -> PathBuf {
    #[cfg(windows)]
    {
        to_long_path_inner(path)
    }
    #[cfg(not(windows))]
    {
        path.to_path_buf()
    }
}

/// 去掉 `\\?\` / `\\?\UNC\` 前缀，得到可展示、可入库的普通路径。
pub fn strip_verbatim(path: &Path) -> PathBuf {
    #[cfg(windows)]
    {
        strip_verbatim_inner(path)
    }
    #[cfg(not(windows))]
    {
        path.to_path_buf()
    }
}

/// 用平台分隔符把正斜杠相对路径拼到目录上，避免 `\\?\` 下 os error 123。
pub fn join_rel(base: &Path, rel: &str) -> PathBuf {
    let mut p = base.to_path_buf();
    for seg in rel.split('/') {
        p.push(seg);
    }
    p
}

/// 原子覆盖替换：`MoveFileExW(REPLACE_EXISTING | WRITE_THROUGH)`；非 Windows 用 `fs::rename`。
pub fn replace_file(src: &Path, dst: &Path) -> io::Result<()> {
    #[cfg(windows)]
    {
        use windows_sys::Win32::Storage::FileSystem::{
            MoveFileExW, MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH,
        };

        let src_w = wide_null(&to_long_path(src));
        let dst_w = wide_null(&to_long_path(dst));
        let ok = unsafe {
            MoveFileExW(
                src_w.as_ptr(),
                dst_w.as_ptr(),
                MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
            )
        };
        if ok == 0 {
            Err(io::Error::last_os_error())
        } else {
            Ok(())
        }
    }
    #[cfg(not(windows))]
    {
        let _ = (src, dst);
        fs::rename(src, dst)
    }
}

fn temp_sibling(path: &Path) -> PathBuf {
    let name = path
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "file".to_string());
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.subsec_nanos())
        .unwrap_or(0);
    path.with_file_name(format!(
        ".{}.{}.{}.tmp",
        name,
        std::process::id(),
        nanos
    ))
}

/// 写临时文件 + 原子 rename 覆盖，保证目标要么是旧内容要么是完整新内容。
pub fn write_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let tmp = temp_sibling(path);
    fs::write(to_long_path(&tmp), bytes)?;
    match replace_file(&tmp, path) {
        Ok(()) => Ok(()),
        Err(err) => {
            let _ = fs::remove_file(to_long_path(&tmp));
            Err(err)
        }
    }
}

/// 拷贝到目标同目录临时文件后原子替换（跨卷安全，源保留）。
pub fn copy_atomic(src: &Path, dst: &Path) -> io::Result<()> {
    let tmp = temp_sibling(dst);
    fs::copy(to_long_path(src), to_long_path(&tmp))?;
    match replace_file(&tmp, dst) {
        Ok(()) => Ok(()),
        Err(err) => {
            let _ = fs::remove_file(to_long_path(&tmp));
            Err(err)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn join_rel_uses_platform_separators() {
        let p = join_rel(Path::new("base"), "data/config/game.ini");
        let expected = PathBuf::from("base")
            .join("data")
            .join("config")
            .join("game.ini");
        assert_eq!(p, expected);
    }

    #[test]
    fn join_rel_rejects_nothing_but_splits_slashes() {
        let p = join_rel(Path::new(r"D:\game"), "a/b");
        assert_eq!(p, PathBuf::from(r"D:\game").join("a").join("b"));
    }

    #[cfg(windows)]
    #[test]
    fn long_path_roundtrip() {
        let plain = PathBuf::from(r"D:\Games\MyGame\data\game.ini");
        let long = to_long_path(&plain);
        assert!(long.to_string_lossy().starts_with(r"\\?\"));
        assert_eq!(strip_verbatim(&long), plain);

        let unc = PathBuf::from(r"\\server\share\file.txt");
        let long_unc = to_long_path(&unc);
        assert!(long_unc.to_string_lossy().starts_with(r"\\?\UNC\"));
        assert_eq!(strip_verbatim(&long_unc), unc);

        // 幂等
        assert_eq!(to_long_path(&long), long);
    }

    #[cfg(windows)]
    #[test]
    fn relative_path_not_prefixed() {
        let rel = PathBuf::from(r"sub\file.txt");
        assert_eq!(to_long_path(&rel), rel);
    }
}
