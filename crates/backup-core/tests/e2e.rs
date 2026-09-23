use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use backup_core::apply::{apply, apply_with_plan};
use backup_core::apply_plan::build_plan;
use backup_core::error::BackupError;
use backup_core::layer::{
    delete_layer, files_dir, layer_dir, layer_dir_name, list_layers, next_seq, stack_top,
    write_meta, LayerMeta, LayerStatus,
};
use backup_core::platform;
use backup_core::pop_layer::rollback_top;
use backup_core::preview::{preview_apply, preview_rollback, PREVIEW_PATH_LIMIT};
use backup_core::project::{create_project, ProjectLock, ProjectMeta};
use backup_core::walk::{scan_dirs, scan_tree};
use backup_core::Progress;
use chrono::Local;
use tempfile::TempDir;

type Tree = Vec<(&'static str, &'static [u8])>;

struct Fixture {
    tmp: TempDir,
    root: PathBuf,
    base: PathBuf,
}

fn write_tree(root: &Path, tree: &Tree) {
    for (rel, data) in tree {
        let p = platform::join_rel(root, rel);
        if let Some(parent) = p.parent() {
            fs::create_dir_all(platform::to_long_path(parent)).unwrap();
        }
        fs::write(platform::to_long_path(&p), data).unwrap();
    }
}

fn read_tree(root: &Path) -> BTreeMap<String, Vec<u8>> {
    let mut out = BTreeMap::new();
    for e in scan_tree(root).unwrap() {
        let p = platform::join_rel(root, &e.rel_path);
        out.insert(e.rel_path, fs::read(platform::to_long_path(&p)).unwrap());
    }
    out
}

fn setup(base_tree: &Tree) -> Fixture {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("backup_root");
    let base = tmp.path().join("base");
    fs::create_dir_all(&root).unwrap();
    write_tree(&base, base_tree);
    Fixture { tmp, root, base }
}

fn empty_tree() -> Tree {
    Vec::new()
}

fn add_project(fx: &Fixture, name: &str) -> ProjectMeta {
    create_project(&fx.root, name, &fx.base).unwrap()
}

fn no_progress(_p: Progress) {}

/// 场景 1：只增不改 → inter 为空；rollback 把新增删干净。
#[test]
fn only_add_no_overwrite_apply_rollback() {
    let fx = setup(&vec![
        ("data/game.ini", b"orig-ini".as_slice()),
        ("readme.md", b"readme".as_slice()),
    ]);
    let proj = add_project(&fx, "p1");
    let mod_src = fx.tmp.path().join("mod");
    write_tree(
        &mod_src,
        &vec![
            ("scripts/new.lua", b"lua".as_slice()),
            ("scripts/extra.lua", b"extra".as_slice()),
        ],
    );

    let before = read_tree(&fx.base);

    let preview = preview_apply(&fx.base, &mod_src).unwrap();
    assert_eq!(preview.overwrite_count, 0);
    assert_eq!(preview.add_count, 2);
    assert_eq!(preview.untouched_count, 2);

    let lock = ProjectLock::try_acquire(&fx.root, &proj.id).unwrap();
    let meta = apply(&fx.root, &proj, &mod_src, Some("仅新增"), no_progress).unwrap();
    drop(lock);

    assert_eq!(meta.overwritten.len(), 0);
    assert_eq!(meta.added.len(), 2);
    assert_eq!(meta.status, LayerStatus::Applied);
    // inter 为空 → files/ 下没有任何备份文件
    let ldir = layer_dir(&fx.root, &proj, &meta.id);
    assert_eq!(scan_tree(&files_dir(&ldir)).unwrap().len(), 0);
    // 底包新增了 2 个文件
    assert_eq!(read_tree(&fx.base).len(), 4);

    let lock = ProjectLock::try_acquire(&fx.root, &proj.id).unwrap();
    let rolled = rollback_top(&fx.root, &proj, no_progress).unwrap();
    drop(lock);

    assert_eq!(rolled.status, LayerStatus::RolledBack);
    assert_eq!(read_tree(&fx.base), before);
    // 层目录保留可审计
    assert!(ldir.join("meta.json").exists());
}

/// 场景 2：部分覆盖 + 部分新增 → 备份仅交集；rollback 后路径集与内容全还原。
#[test]
fn partial_overwrite_and_add() {
    let fx = setup(&vec![
        ("a.txt", b"orig-A".as_slice()),
        ("b.txt", b"orig-B".as_slice()),
    ]);
    let proj = add_project(&fx, "p1");
    let mod_src = fx.tmp.path().join("mod");
    write_tree(
        &mod_src,
        &vec![("a.txt", b"mod-A".as_slice()), ("c.txt", b"new-C".as_slice())],
    );
    let before = read_tree(&fx.base);

    let preview = preview_apply(&fx.base, &mod_src).unwrap();
    assert_eq!(preview.overwrite_count, 1);
    assert_eq!(preview.add_count, 1);
    assert_eq!(preview.untouched_count, 1);
    assert_eq!(preview.overwrite_bytes, 6);
    assert_eq!(preview.add_bytes, 5);

    let lock = ProjectLock::try_acquire(&fx.root, &proj.id).unwrap();
    let meta = apply(&fx.root, &proj, &mod_src, None, no_progress).unwrap();
    drop(lock);

    // 备份仅交集：只有 a.txt，内容为原文件
    let ldir = layer_dir(&fx.root, &proj, &meta.id);
    let backups = scan_tree(&files_dir(&ldir)).unwrap();
    assert_eq!(backups.len(), 1);
    assert_eq!(backups[0].rel_path, "a.txt");
    let backed_a = fs::read(platform::to_long_path(&platform::join_rel(
        &files_dir(&ldir),
        "a.txt",
    )))
    .unwrap();
    assert_eq!(backed_a, b"orig-A");
    assert_eq!(meta.stats.overwrite_bytes, 6);
    assert_eq!(meta.stats.add_bytes, 5);

    // 底包已被 mod 覆盖
    let mid = read_tree(&fx.base);
    assert_eq!(mid.get("a.txt").unwrap(), b"mod-A");
    assert!(mid.contains_key("c.txt"));

    let lock = ProjectLock::try_acquire(&fx.root, &proj.id).unwrap();
    rollback_top(&fx.root, &proj, no_progress).unwrap();
    drop(lock);

    assert_eq!(read_tree(&fx.base), before);
}

/// 场景 3：两层叠加 LIFO → 回滚顶回到仅 A，再回滚 = 干净底包。
#[test]
fn two_layers_lifo() {
    let fx = setup(&vec![("a.txt", b"base".as_slice())]);
    let proj = add_project(&fx, "p1");
    let mod_a = fx.tmp.path().join("modA");
    let mod_b = fx.tmp.path().join("modB");
    write_tree(&mod_a, &vec![("a.txt", b"layer-A".as_slice())]);
    write_tree(
        &mod_b,
        &vec![
            ("a.txt", b"layer-B".as_slice()),
            ("e.txt", b"extra".as_slice()),
        ],
    );
    let clean = read_tree(&fx.base);

    let lock = ProjectLock::try_acquire(&fx.root, &proj.id).unwrap();
    apply(&fx.root, &proj, &mod_a, None, no_progress).unwrap();
    apply(&fx.root, &proj, &mod_b, None, no_progress).unwrap();
    drop(lock);

    let layers = list_layers(&fx.root, &proj).unwrap();
    assert_eq!(layers.len(), 2);
    assert_eq!(layers[0].seq, 1);
    assert_eq!(layers[1].seq, 2);

    // 回滚顶层 → 回到仅 A 状态
    let lock = ProjectLock::try_acquire(&fx.root, &proj.id).unwrap();
    let top = rollback_top(&fx.root, &proj, no_progress).unwrap();
    drop(lock);
    assert_eq!(top.seq, 2);
    assert_eq!(top.status, LayerStatus::RolledBack);
    let mid = read_tree(&fx.base);
    assert_eq!(mid.get("a.txt").unwrap(), b"layer-A");
    assert!(!mid.contains_key("e.txt"));

    // 再回滚 → 干净底包
    let lock = ProjectLock::try_acquire(&fx.root, &proj.id).unwrap();
    let top = rollback_top(&fx.root, &proj, no_progress).unwrap();
    drop(lock);
    assert_eq!(top.seq, 1);
    assert_eq!(read_tree(&fx.base), clean);

    // 两层都已出栈（目录保留）→ 空栈
    let lock = ProjectLock::try_acquire(&fx.root, &proj.id).unwrap();
    let err = rollback_top(&fx.root, &proj, no_progress).unwrap_err();
    drop(lock);
    assert!(matches!(err, BackupError::StackEmpty));
}

/// 场景 4：不存在跳层接口（源码守卫）。
#[test]
fn no_jump_layer_api() {
    let src_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let forbidden = [
        "fn rollback_to(",
        "fn rollback_seq(",
        "fn pop_to(",
        "fn pop_seq(",
        "fn remove_layer(",
        "fn discard_layer_at(",
    ];
    for entry in fs::read_dir(&src_dir).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().and_then(|e| e.to_str()) != Some("rs") {
            continue;
        }
        let text = fs::read_to_string(&path).unwrap();
        for f in forbidden {
            assert!(
                !text.contains(f),
                "发现禁止的跳层接口 {f} in {}",
                path.display()
            );
        }
    }
}

/// 场景 5：空栈 rollback → StackEmpty。
#[test]
fn empty_stack_rollback_errors() {
    let fx = setup(&vec![("a.txt", b"a".as_slice())]);
    let proj = add_project(&fx, "p1");

    let err = rollback_top(&fx.root, &proj, no_progress).unwrap_err();
    assert!(matches!(err, BackupError::StackEmpty));

    let err = preview_rollback(&fx.root, &proj).unwrap_err();
    assert!(matches!(err, BackupError::StackEmpty));
}

/// 场景 6a：崩溃注入 backed_up 未 applied → 底包未脏、不可回滚、层可丢弃。
#[test]
fn crash_backed_up_not_applied_is_clean() {
    let fx = setup(&vec![("a.txt", b"orig".as_slice())]);
    let proj = add_project(&fx, "p1");
    let clean = read_tree(&fx.base);

    // 手工构造一个 backed_up 层（模拟：备份完成、拷 mod 前崩溃）
    let seq = next_seq(&[]);
    let dir_name = layer_dir_name(seq, Local::now());
    let ldir = layer_dir(&fx.root, &proj, &dir_name);
    fs::create_dir_all(files_dir(&ldir)).unwrap();
    fs::copy(
        platform::to_long_path(&fx.base.join("a.txt")),
        platform::to_long_path(&platform::join_rel(&files_dir(&ldir), "a.txt")),
    )
    .unwrap();
    let meta = LayerMeta {
        seq,
        id: dir_name.clone(),
        created_at: Local::now(),
        mod_src: "D:/Mods/X".to_string(),
        mod_name: "X".to_string(),
        note: None,
        status: LayerStatus::BackedUp,
        structure_before: scan_tree(&fx.base)
            .unwrap()
            .into_iter()
            .map(|e| backup_core::layer::StructEntry {
                rel_path: e.rel_path,
                size: e.size,
                mtime_ns: e.mtime_ns,
            })
            .collect(),
        dirs_before: None,
        overwritten: vec!["a.txt".to_string()],
        added: vec![],
        stats: backup_core::layer::LayerStats::default(),
    };
    write_meta(&ldir, &meta).unwrap();

    // 底包未脏
    assert_eq!(read_tree(&fx.base), clean);
    // 不可回滚（未 applied）
    let err = rollback_top(&fx.root, &proj, no_progress).unwrap_err();
    assert!(matches!(err, BackupError::StatusConflict(_)));
    // 层可丢弃：手工删除后栈空
    fs::remove_dir_all(&ldir).unwrap();
    assert!(stack_top(&fx.root, &proj).unwrap().is_none());
    // 底包依旧干净
    assert_eq!(read_tree(&fx.base), clean);
}

/// 场景 6b：拷 mod 中途失败 → 状态推到 applied，rollback 可收敛。
#[test]
fn crash_mid_mod_copy_then_rollback_converges() {
    let fx = setup(&vec![("a.txt", b"orig-A".as_slice())]);
    // base 里放一个与 mod 文件同名的目录，使拷贝失败
    fs::create_dir_all(fx.base.join("blocked")).unwrap();
    let proj = add_project(&fx, "p1");

    let mod_src = fx.tmp.path().join("mod");
    write_tree(
        &mod_src,
        &vec![
            ("a.txt", b"mod-A".as_slice()),
            ("aaa_new.txt", b"new".as_slice()),
            ("blocked", b"x".as_slice()),
        ],
    );

    // 记录文件级底包（目录不计）
    let clean: BTreeMap<String, Vec<u8>> = read_tree(&fx.base);

    let lock = ProjectLock::try_acquire(&fx.root, &proj.id).unwrap();
    let err = apply(&fx.root, &proj, &mod_src, None, no_progress).unwrap_err();
    drop(lock);
    assert!(matches!(err, BackupError::Io(_) | BackupError::FileLocked(_)));

    // 中途失败但状态必须是 applied（可回滚收敛）
    let layers = list_layers(&fx.root, &proj).unwrap();
    assert_eq!(layers.len(), 1);
    assert_eq!(layers[0].status, LayerStatus::Applied);
    // 部分已拷入：a.txt 被覆盖、aaa_new 已存在
    let mid = read_tree(&fx.base);
    assert_eq!(mid.get("a.txt").unwrap(), b"mod-A");
    assert!(mid.contains_key("aaa_new.txt"));

    // rollback 收敛到干净底包
    let lock = ProjectLock::try_acquire(&fx.root, &proj.id).unwrap();
    rollback_top(&fx.root, &proj, no_progress).unwrap();
    drop(lock);
    assert_eq!(read_tree(&fx.base), clean);
    assert!(fx.base.join("blocked").is_dir());
}

/// 场景 7：并发第二实例拿 `.lock` 被拒绝。
#[test]
fn concurrent_lock_rejected() {
    let fx = setup(&vec![("a.txt", b"a".as_slice())]);
    let proj = add_project(&fx, "p1");

    let _lock1 = ProjectLock::try_acquire(&fx.root, &proj.id).unwrap();
    let err = ProjectLock::try_acquire(&fx.root, &proj.id).unwrap_err();
    assert!(matches!(err, BackupError::FileLocked(_)));
}

/// 场景 8：mod 内含 `../evil` → 拒绝且零写入。
#[test]
fn evil_rel_path_rejected_zero_write() {
    let fx = setup(&vec![("a.txt", b"orig".as_slice())]);
    let proj = add_project(&fx, "p1");
    let mod_src = fx.tmp.path().join("mod");
    write_tree(&mod_src, &vec![("ok.txt", b"ok".as_slice())]);
    let clean = read_tree(&fx.base);

    // 构造带 evil 路径的计划
    let mut plan = build_plan(&fx.base, &mod_src).unwrap();
    plan.added.push("../evil".to_string());
    plan.mod_entries.push(backup_core::walk::FileEntry {
        rel_path: "../evil".to_string(),
        size: 1,
        mtime_ns: 0,
    });

    let lock = ProjectLock::try_acquire(&fx.root, &proj.id).unwrap();
    let err = apply_with_plan(&fx.root, &proj, &plan, None, no_progress).unwrap_err();
    drop(lock);

    assert!(matches!(err, BackupError::InvalidRelPath(_)));
    // 零写入：没有层、底包未变、无 evil 文件
    assert!(list_layers(&fx.root, &proj).unwrap().is_empty());
    assert_eq!(read_tree(&fx.base), clean);
    assert!(!fx.tmp.path().join("evil").exists());
    assert!(!fx.base.parent().unwrap().join("evil").exists());
}

/// 场景 9：长路径 `\\?\` 往返（>260 字符全管线）。
#[cfg(windows)]
#[test]
fn long_path_apply_rollback() {
    let fx = setup(&empty_tree());
    let seg = "d".repeat(80);
    let deep_rel = format!("{seg}/{seg}/{seg}/{seg}/deep.ini");
    assert!(platform::join_rel(&fx.base, &deep_rel).to_string_lossy().len() > 260);

    write_tree(&fx.base, &empty_tree());
    // 手工写深文件
    let deep_path = platform::join_rel(&fx.base, &deep_rel);
    fs::create_dir_all(platform::to_long_path(deep_path.parent().unwrap())).unwrap();
    fs::write(platform::to_long_path(&deep_path), b"deep-orig").unwrap();

    // scan 能看到（jwalk 长路径）
    let entries = scan_tree(&fx.base).unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].rel_path, deep_rel.replace('\\', "/"));

    let proj = add_project(&fx, "long");
    let mod_src = fx.tmp.path().join("mod");
    write_tree(&mod_src, &vec![]);
    let mod_deep = platform::join_rel(&mod_src, &deep_rel);
    fs::create_dir_all(platform::to_long_path(mod_deep.parent().unwrap())).unwrap();
    fs::write(platform::to_long_path(&mod_deep), b"deep-mod").unwrap();

    let lock = ProjectLock::try_acquire(&fx.root, &proj.id).unwrap();
    let meta = apply(&fx.root, &proj, &mod_src, None, no_progress).unwrap();
    drop(lock);
    assert_eq!(meta.overwritten, vec![deep_rel.clone()]);
    assert_eq!(
        fs::read(platform::to_long_path(&deep_path)).unwrap(),
        b"deep-mod"
    );

    let lock = ProjectLock::try_acquire(&fx.root, &proj.id).unwrap();
    rollback_top(&fx.root, &proj, no_progress).unwrap();
    drop(lock);
    assert_eq!(
        fs::read(platform::to_long_path(&deep_path)).unwrap(),
        b"deep-orig"
    );
}

/// 场景 10：preview 计数与 apply 实际一致（抽样）。
#[test]
fn preview_matches_apply() {
    let fx = setup(&vec![
        ("keep.txt", b"keep".as_slice()),
        ("over.txt", b"over-old".as_slice()),
        ("both.txt", b"both-old".as_slice()),
    ]);
    let proj = add_project(&fx, "p1");
    let mod_src = fx.tmp.path().join("mod");
    write_tree(
        &mod_src,
        &vec![
            ("over.txt", b"over-new".as_slice()),
            ("add1.txt", b"a1".as_slice()),
            ("add2.txt", b"a22".as_slice()),
        ],
    );

    let preview = preview_apply(&fx.base, &mod_src).unwrap();
    let lock = ProjectLock::try_acquire(&fx.root, &proj.id).unwrap();
    let meta = apply(&fx.root, &proj, &mod_src, None, no_progress).unwrap();
    drop(lock);

    assert_eq!(preview.overwrite_count, meta.overwritten.len());
    assert_eq!(preview.add_count, meta.added.len());
    assert_eq!(preview.overwrite_bytes, meta.stats.overwrite_bytes);
    assert_eq!(preview.add_bytes, meta.stats.add_bytes);
    assert_eq!(preview.untouched_count, 2); // keep.txt + both.txt
}

/// 预览截断：250 个新增 → 列表 200，计数 250；回滚预览同理。
#[test]
fn preview_truncates_at_200() {
    let fx = setup(&vec![("seed.txt", b"s".as_slice())]);
    let proj = add_project(&fx, "p1");
    let mod_src = fx.tmp.path().join("mod");
    // 250 个文件名按字典序可预测
    let tree: Tree = (0..250)
        .map(|i| {
            let name = format!("f{i:03}.txt");
            // 泄漏到 &'static str 不可行 → 用字符串池
            Box::leak(name.into_boxed_str()) as &'static str
        })
        .map(|n| -> (&'static str, &'static [u8]) { (n, b"x".as_slice()) })
        .collect();
    write_tree(&mod_src, &tree);

    let preview = preview_apply(&fx.base, &mod_src).unwrap();
    assert_eq!(preview.add_count, 250);
    assert_eq!(preview.add_paths.len(), PREVIEW_PATH_LIMIT);

    let lock = ProjectLock::try_acquire(&fx.root, &proj.id).unwrap();
    apply(&fx.root, &proj, &mod_src, None, no_progress).unwrap();
    drop(lock);

    let rb = preview_rollback(&fx.root, &proj).unwrap();
    // 250 新增 + seed 在 before → 恢复 0 个（inter 为空），删除 250
    assert_eq!(rb.restore_count, 0);
    assert_eq!(rb.delete_count, 250);
    assert_eq!(rb.delete_paths.len(), PREVIEW_PATH_LIMIT);
}

/// Windows：文件被独占锁定时备份阶段严格失败（不拷 mod、列出路径）。
#[cfg(windows)]
#[test]
fn backup_strict_fail_on_locked_file() {
    use std::os::windows::fs::OpenOptionsExt;

    let fx = setup(&vec![("game.ini", b"locked-content".as_slice())]);
    let proj = add_project(&fx, "p1");
    let mod_src = fx.tmp.path().join("mod");
    write_tree(
        &mod_src,
        &vec![
            ("game.ini", b"mod".as_slice()),
            ("extra.txt", b"e".as_slice()),
        ],
    );
    let clean = read_tree(&fx.base);

    // 独占打开 game.ini（share_mode 0：别人不能读）
    let _hold = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .share_mode(0)
        .open(platform::to_long_path(&fx.base.join("game.ini")))
        .unwrap();

    let lock = ProjectLock::try_acquire(&fx.root, &proj.id).unwrap();
    let err = apply(&fx.root, &proj, &mod_src, None, no_progress).unwrap_err();
    drop(lock);

    match err {
        BackupError::FileLocked(paths) => {
            assert_eq!(paths.len(), 1);
            assert!(paths[0].contains("game.ini"));
        }
        other => panic!("期望 FileLocked，得到 {other:?}"),
    }
    // 严格失败：没有层残留、mod 未拷入（先释放占用再读底包）
    assert!(list_layers(&fx.root, &proj).unwrap().is_empty());
    drop(_hold);
    assert_eq!(read_tree(&fx.base), clean);
}

/// 目录清理 1：mod 带多级新目录 → 恢复后目录集回到应用前。
#[test]
fn restore_removes_multilevel_new_dirs() {
    let fx = setup(&vec![("game.ini", b"orig".as_slice())]);
    let proj = add_project(&fx, "p1");
    let dirs_before = scan_dirs(&fx.base).unwrap();

    let mod_src = fx.tmp.path().join("mod");
    write_tree(
        &mod_src,
        &vec![
            ("game.ini", b"mod".as_slice()),
            ("a/b/c/new.txt", b"deep".as_slice()),
        ],
    );

    let lock = ProjectLock::try_acquire(&fx.root, &proj.id).unwrap();
    apply(&fx.root, &proj, &mod_src, None, no_progress).unwrap();
    drop(lock);
    // 应用后新目录存在
    assert!(fx.base.join("a/b/c").is_dir());

    // preview 应报出将清理的目录
    let rb = preview_rollback(&fx.root, &proj).unwrap();
    assert!(rb.empty_dirs_count >= 3, "至少 a/b/c、a/b、a，得到 {}", rb.empty_dirs_count);

    let lock = ProjectLock::try_acquire(&fx.root, &proj.id).unwrap();
    rollback_top(&fx.root, &proj, no_progress).unwrap();
    drop(lock);

    let dirs_after = scan_dirs(&fx.base).unwrap();
    assert_eq!(dirs_after, dirs_before, "恢复后目录集必须回到应用前");
    assert!(!fx.base.join("a").exists(), "多级新目录应被清干净");
}

/// 目录清理 2：原有空目录与原有目录（本层新加文件）都不得误删。
#[test]
fn restore_keeps_preexisting_dirs() {
    let fx = setup(&vec![
        ("game.ini", b"orig".as_slice()),
        ("data/keep.txt", b"keep".as_slice()),
    ]);
    let proj = add_project(&fx, "p1");
    // 底包原有空目录
    let empty_dir = fx.base.join("keep_empty");
    fs::create_dir_all(&empty_dir).unwrap();

    let mod_src = fx.tmp.path().join("mod");
    write_tree(
        &mod_src,
        &vec![
            ("game.ini", b"mod".as_slice()),
            ("data/new.txt", b"new".as_slice()),
            ("brand_new/x.txt", b"x".as_slice()),
        ],
    );

    let lock = ProjectLock::try_acquire(&fx.root, &proj.id).unwrap();
    apply(&fx.root, &proj, &mod_src, None, no_progress).unwrap();
    drop(lock);

    let lock = ProjectLock::try_acquire(&fx.root, &proj.id).unwrap();
    rollback_top(&fx.root, &proj, no_progress).unwrap();
    drop(lock);

    // 原有空目录保留
    assert!(empty_dir.is_dir(), "原有空目录 keep_empty 不得被删");
    // 原有 data/ 保留，其下本层新文件已删
    assert!(fx.base.join("data").is_dir());
    assert!(!fx.base.join("data/new.txt").exists());
    assert!(fx.base.join("data/keep.txt").exists());
    // 本层新建 brand_new/ 被清掉
    assert!(!fx.base.join("brand_new").exists());
}

/// 目录清理 3：旧层 meta 无 dirs_before（向后兼容）→ 用 added 祖先启发式清目录；
/// 底包原有、未被本层写入的空目录不得误删。
#[test]
fn restore_legacy_meta_without_dirs_before_uses_added_ancestors() {
    let fx = setup(&vec![("game.ini", b"orig".as_slice())]);
    let proj = add_project(&fx, "p1");
    // 底包原有空目录（mod 未触及）
    let keep_empty = fx.base.join("keep_empty");
    fs::create_dir_all(&keep_empty).unwrap();

    // 手工构造旧格式层：dirs_before=None
    let seq = next_seq(&[]);
    let dir_name = layer_dir_name(seq, Local::now());
    let ldir = layer_dir(&fx.root, &proj, &dir_name);
    fs::create_dir_all(files_dir(&ldir)).unwrap();
    let meta = LayerMeta {
        seq,
        id: dir_name.clone(),
        created_at: Local::now(),
        mod_src: "D:/Mods/Legacy".into(),
        mod_name: "Legacy".into(),
        note: None,
        status: LayerStatus::Applied,
        structure_before: scan_tree(&fx.base)
            .unwrap()
            .into_iter()
            .map(|e| backup_core::layer::StructEntry {
                rel_path: e.rel_path,
                size: e.size,
                mtime_ns: e.mtime_ns,
            })
            .collect(),
        dirs_before: None,
        overwritten: vec![],
        added: vec!["newdir/file.txt".to_string(), "a/b/c/deep.txt".to_string()],
        stats: backup_core::layer::LayerStats::default(),
    };
    write_meta(&ldir, &meta).unwrap();

    // 底包模拟 mod 已拷入
    write_tree(
        &fx.base,
        &vec![
            ("newdir/file.txt", b"added".as_slice()),
            ("a/b/c/deep.txt", b"deep".as_slice()),
        ],
    );

    // 预览：旧层也应报出将清理的目录（added 祖先）
    let rb = preview_rollback(&fx.root, &proj).unwrap();
    assert!(
        rb.empty_dirs_count >= 4,
        "至少 newdir、a、a/b、a/b/c，得到 {}",
        rb.empty_dirs_count
    );

    let lock = ProjectLock::try_acquire(&fx.root, &proj.id).unwrap();
    let rolled = rollback_top(&fx.root, &proj, no_progress).unwrap();
    drop(lock);
    assert_eq!(rolled.status, LayerStatus::RolledBack);

    // 文件被删
    assert!(!fx.base.join("newdir/file.txt").exists());
    assert!(!fx.base.join("a/b/c/deep.txt").exists());
    // 新目录链被启发式清掉
    assert!(!fx.base.join("newdir").exists(), "newdir 应被清理");
    assert!(!fx.base.join("a").exists(), "a/b/c 链应被清理");
    // 底包原有空目录保留（不是 added 祖先）
    assert!(keep_empty.is_dir(), "原有空目录 keep_empty 不得被删");
}

/// 必测 14：仅 rolled_back 可删除；applied 拒绝；删除后层从列表消失、底包不受影响。
#[test]
fn delete_layer_only_rolled_back() {
    let fx = setup(&vec![
        ("a.txt", b"base".as_slice()),
        ("b.txt", b"keep".as_slice()),
    ]);
    let proj = add_project(&fx, "p1");
    let mod_src = fx.tmp.path().join("mod");
    write_tree(&mod_src, &vec![("a.txt", b"mod".as_slice())]);
    let clean = read_tree(&fx.base);

    let lock = ProjectLock::try_acquire(&fx.root, &proj.id).unwrap();
    let meta = apply(&fx.root, &proj, &mod_src, None, no_progress).unwrap();
    drop(lock);
    let ldir1 = layer_dir(&fx.root, &proj, &meta.id);

    // applied 层不可删
    let lock = ProjectLock::try_acquire(&fx.root, &proj.id).unwrap();
    let err = delete_layer(&fx.root, &proj, meta.seq).unwrap_err();
    assert!(matches!(err, BackupError::StatusConflict(_)));
    drop(lock);
    assert!(ldir1.join("meta.json").exists(), "applied 层目录必须完好");

    // 不存在的 seq
    let lock = ProjectLock::try_acquire(&fx.root, &proj.id).unwrap();
    let err = delete_layer(&fx.root, &proj, 999).unwrap_err();
    assert!(matches!(err, BackupError::LayerNotFound(_)));
    drop(lock);

    // 恢复后可删
    let lock = ProjectLock::try_acquire(&fx.root, &proj.id).unwrap();
    rollback_top(&fx.root, &proj, no_progress).unwrap();
    drop(lock);
    assert_eq!(read_tree(&fx.base), clean);

    let lock = ProjectLock::try_acquire(&fx.root, &proj.id).unwrap();
    let removed = delete_layer(&fx.root, &proj, meta.seq).unwrap();
    drop(lock);
    assert_eq!(removed.seq, meta.seq);
    assert!(!ldir1.exists(), "已恢复层目录应被整棵删除");
    assert!(list_layers(&fx.root, &proj).unwrap().is_empty());

    // 底包不受影响；删除 max seq 后 next_seq 回落，可正常创建新层
    assert_eq!(read_tree(&fx.base), clean);
    assert_eq!(next_seq(&[]), 1);
    let lock = ProjectLock::try_acquire(&fx.root, &proj.id).unwrap();
    let m2 = apply(&fx.root, &proj, &mod_src, None, no_progress).unwrap();
    drop(lock);
    assert_eq!(m2.seq, 1, "序号复用不 panic，新层正常创建");
}
