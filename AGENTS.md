# AGENTS.md — Folder Backup 代理开发约定

修改本仓库代码前必读。产品与架构的**唯一事实来源是 [plan.md](plan.md)**；本文只列执行层约定。

## 项目是什么

Windows mod 层栈备份/恢复工具：apply 前只备份**交集文件**+ 结构清单；多层 LIFO；恢复只 pop 栈顶。Rust workspace（`backup-core` 引擎 + `backup-cli`）+ Tauri v2 GUI（React/Vite/TS/Vitest）。

## 术语（必须遵守）

| ✅ 用 | ❌ 不用 |
|---|---|
| **恢复 / `restore`**（CLI 命令、用户文案） | 回滚 / `rollback`（CLI 命令已删除；仅代码标识符保留 `rollback_top`、`op:"rollback"`、`pop_layer`） |
| 栈顶、已恢复（`rolled_back`）、已应用（`applied`） | 跳层、摘中间层（**禁止实现**，plan 决策 3） |

## 门禁（提交前全绿，缺一不可）

```powershell
cargo build --workspace
cargo test --workspace
cargo clippy --workspace --all-targets   # 必须零警告
npm run typecheck
npm test
npm run build
```

打包验收（改过 tauri.conf / 便携逻辑时）：

```powershell
powershell -File scripts/build-release.ps1
```

## 关键不变式（破坏即 bug）

1. 层唯一事实 = `meta.json` + `files/`，无第二套索引
2. apply 写序固定：拷交集 → `backed_up`（临时+原子 rename）→ 拷 mod → `applied`；**绝不允许** `applied` 却没有 `files/`
3. 修改类操作（apply / restore / 删项目 / 删层）**先拿 `.lock`（fs2）**；引擎函数假定调用者已持锁，**内部不重复加锁**（同进程二次加锁死锁）
4. restore 仅栈顶且 `applied`（或 `restoring` 幂等重入）；空栈 → `StackEmpty`
5. `rel_path` 正斜杠入库；读写前校验拒 `..` `.` 空段 绝对路径
6. 覆盖写一律 `platform::replace_file()`（MoveFileExW + REPLACE_EXISTING）；meta.json 一律临时文件 + 原子 rename
7. 仅 `rolled_back` 层可 `layer rm`；`applied`/`restoring` 删除必须 `StatusConflict`

## 代码约定

- **edition 2021**；依赖集中在根 `Cargo.toml` workspace.dependencies
- **禁止引入**：BLAKE3、zstd、rusqlite（plan 明确排除）
- 用户可见文案（CLI 输出、GUI、错误 Display）一律**简体中文**；`BackupError` 的 message/hint 中文
- GUI 错误统一 `AppError { message, kind, hint }`，按 kind 给操作建议；新增错误变体要补 `hint`
- 预览路径截断 `PREVIEW_PATH_LIMIT = 200`，CLI / GUI 同步
- 进度事件 `progress`：`{ op, stage, done, total }`；`total=0` 表示无分母
- 长路径：核心存普通 `PathBuf`，I/O 经 `platform::to_long_path()`；jwalk 传普通路径不预加 `\\?\`；相对路径拼接用 `join_rel`
- 文件锁检测：`raw_os_error` 32 / 33 → `FileLocked`

## 测试约定

- fixture：`type Tree = Vec<(&'static str, &'static [u8])>` + `tempfile` 独立 tempdir
- e2e 场景清单见 plan.md「必测场景」（14 项），改管线必须保持全绿
- Windows-only 测试加 `#[cfg(windows)]`
- vitest 放 `src/**/*.test.{ts,tsx}`；RTL 渲染测试需 `afterEach(cleanup)`

## 前端约定

- 设计令牌与壳在 `src/styles.css`（`.page` 统一三页 shell；`.main-header` 有 `min-height:36px` 锁标题几何）——**改样式时保持三页 header 一致，禁止只改某一页的结构**
- 页面切换用 `useState<Page>`，不引路由库
- IPC 封装在 `src/lib/ipc.ts`；类型在 `src/lib/types.ts`

## 分发

- 一键打包：`scripts/build-release.ps1`（MSI + portable zip → `release/`）
- 便携判定：exe 旁存在 `settings.json` → `resolve_settings_path` 返回 exe 旁路径
- CI：`.github/workflows/release.yml`，**推 `v*` tag** 触发；门禁在脚本内，不过不发版
- 产物路径注意：**Cargo workspace 的 target 在仓库根 `target/`**，不是 `src-tauri/target/`

## 常用命令

```powershell
npm run tauri dev          # GUI 开发
cargo test -p backup-core  # 引擎 e2e（17 项）
cargo run -p backup-cli -- --help
powershell -File scripts/build-release.ps1 -SkipGates   # 快速出包
```
