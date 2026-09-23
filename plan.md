# 任务：从零开发「Folder Backup」Mod 层栈备份 / 恢复工具

## 一句话目标

装 mod 会覆盖、新增游戏本体目录里的文件。开发一个 Windows 优先的本地工具：
在把 mod 拷进底包**之前**，只备份「会被覆盖的那部分原文件」+ 记录底包结构；
多层 mod 用**栈**管理；装坏后按后进先出恢复。

## 环境与交付

- 工作目录：`C:\opencode\gameFolderSnap`（全新，仅有本 plan.md，一切从零搭建）
- 交付：
  1. Cargo workspace = `crates/backup-core`（引擎库）+ `crates/backup-cli`（CLI）
  2. `src-tauri`（Tauri v2）+ `src/`（React）GUI
- 技术栈：jwalk + fs2 + clap + serde/serde_json + chrono + thiserror + windows-sys；edition 2021
- 前端工具链（已确认）：**Vite + React + TypeScript + Vitest**
  - `npm run typecheck` = `tsc --noEmit`；`npm test` = `vitest run`；`npm run build` = `vite build`；`npm run dev` = `vite`
- GUI 窗口壳（已确认 1A）：**从零搭建** TitleBar / Sidebar / StatusBar / Toast / ProgressOverlay / Acrylic / capabilities / settings 骨架；建成后 `styles.css` 视觉视为不动的基准
- 不使用 BLAKE3、zstd、rusqlite
- 备份根目录：CLI 全局参数 `--root` / GUI 设置项，默认 `%LOCALAPPDATA%\FolderBackup\backups`

## 已确认的产品决策（不要重新讨论、不要改）

1. 工作流：用户提供 mod 源文件夹 → 工具比较 → **工具先备份再代为拷入** folder_base
2. 安装前的「快照」= **局部拍照**：仅「交集文件内容」+「应用前结构清单」，不是全目录内容快照
3. 叠加 mod = **栈，LIFO**；只支持从顶层连续恢复，**不支持**跳层 / 摘中间层
4. 备份 = **纯文件夹平铺**，无内容寻址、无去重、无压缩、无哈希、无数据库
5. 恢复后的层目录**保留**（可审计）；不提供自动清理，**提供手动删除已恢复层**
   （CLI `layer rm` / GUI 行内删除钮；仅 `rolled_back` 可删，须确认，不可恢复）
6. Apply 由工具执行拷入；**预览（preview）必须先于 apply / restore 确认**
7. 管理粒度 = 多项目（一个项目 = 一个游戏底包目录）+ 每项目一个层栈
8. 全量拷贝 mod 源（不做排除规则、不做块级 diff）；不加密
9. 应用 / 恢复前提醒用户关闭游戏；被锁定文件：备份阶段严格失败（不拷 mod、列出路径），
   恢复阶段个别失败逐条报告、不强行覆盖
10. **术语（2026-09-23 用户确认）**：产品文案与 CLI 命令统一用「恢复 / restore」（替代原「回滚 / rollback」）；
    CLI 仅保留 `restore`（不留 rollback 别名）；代码标识符 `rollback_top` / `preview_rollback` / `pop_layer` / 事件 `op:"rollback"` 保留不动

## 架构（不可破坏的不变式）

### 磁盘布局

```
<backup_root>/
  projects.json                     ← 项目索引
  <project_id>/
    .lock                           ← 跨进程排他锁（fs2）
    layers/
      0001_20260923T110500/
        meta.json                   ← 该层的唯一事实
        files/                      ← 平铺备份，保留相对路径
          data/config/game.ini
          pak/00_World.pak
      0002_…/
        meta.json
        files/
```

### 不变式

1. 某层的唯一事实 = 该层 `meta.json` + `files/`；没有第二套索引
2. Apply 写序固定：拷完交集 → `status=backed_up`（临时文件 + 原子 rename）→ 拷 mod
   → `status=applied`。崩溃只允许留下「有备份未应用」或「应用中」；
   **绝不允许** `applied` 却没有 `files/`
3. 修改类操作（apply / restore / 删项目）必须先拿项目 `.lock`；
   只读（preview / status / list）不加锁。引擎函数假定调用者已持锁，内部不重复加锁
   （同进程二次加锁会死锁）
4. 恢复仅允许栈顶且 `status=applied` 的层；空栈恢复 → 明确报错
5. `rel_path` 一律正斜杠；读取 / 使用前校验，拒绝 `..`、`.`、空段、绝对路径
6. 覆盖写一律 `platform::replace_file()`（原子替换），不裸 `fs::rename` 覆盖已存在目标

### meta.json

```json
{
  "seq": 1,
  "id": "0001_20260923T110500",
  "created_at": "2026-09-23T11:05:00+08:00",
  "mod_src": "D:/Mods/CoolMod",
  "mod_name": "CoolMod",
  "note": "可选",
  "status": "applied",
  "structure_before": [
    { "rel_path": "data/game.ini", "size": 1024, "mtime_ns": 0 }
  ],
  "dirs_before": ["data", "pak"],
  "overwritten": ["data/game.ini", "pak/00_World.pak"],
  "added": ["scripts/new.lua"],
  "stats": { "overwrite_bytes": 12345678, "add_bytes": 999 }
}
```

- `dirs_before`（`Option`）：应用前底包目录清单，恢复时据此清理本层新增空目录；
  **旧层无此字段 → `None` → 恢复改用 `meta.added` 祖先目录启发式清理**（预览同步显示）。

状态机：

- apply：`creating` → `backed_up` → `applied`
- restore（原 rollback）：`applied` → `restoring` → `rolled_back`
- 崩溃恢复：`backed_up` 且未 `applied` = 底包未被改，该层可丢弃；
  `restoring` 可重入继续（拷回是幂等的）

### projects.json

```json
{
  "projects": [
    {
      "id": "…",
      "name": "MyGame",
      "base_path": "D:/Games/MyGame",
      "created_at": "…"
    }
  ]
}
```

## 产品语义

### 对象

- **Project（项目）**：绑定一个 `folder_base` + 一个层栈
- **Layer（层）**：一次 apply 的事务记录 = 被覆盖原文件的备份 + 应用前结构清单
- **栈**：`0001 → 0002 → …`；恢复只能 pop 顶层

### Apply（应用 mod）

```
输入：project.base_dir, mod_src
1. 遍历 base_dir → structure_before（仅 rel_path / size / mtime，不读文件内容）
   + dirs_before（同趟收集目录清单）
2. 遍历 mod_src  → mod_paths
3. inter    = structure_before ∩ mod_paths   （将被覆盖 → 必须备份原内容）
   only_mod = mod_paths − structure_before    （纯新增）
4. 创建 layer_N/：
   - 把 inter 中 base 的原文件按原相对路径拷入 layer_N/files/
   - 写 meta.json，status = backed_up（临时 + 原子 rename）
5. 把 mod_src 全部拷入 base_dir（覆盖 inter、写入 only_mod）
6. meta.status = applied
```

成本 = 两次结构遍历 + **只拷交集字节**；与游戏总大小解耦。

### Restore（恢复顶层，标识符仍为 rollback_top）

```
输入：project（仅栈顶且 status=applied）
1. layer_N/files/ 全部拷回 base_dir（platform::replace_file）
2. 重新遍历 base_dir → structure_now
   删除 structure_now − structure_before 中的路径（本层新增的文件）
3. 清理本层新增空目录：当前目录 − dirs_before，按深度降序逐个 `remove_dir`（仅空目录；
   非空静默跳过）。**`dirs_before = None`（旧层）→ 按 `meta.added` 中每个文件的全部祖先目录
   作候选**（启发式：底包原有空目录若非本层新增文件的祖先则不入选；
   已知边界：「原有空目录 + 本层恰往里加了文件」在旧层会被误删，新层无此问题）。
   预览与实际用同一函数 `dir_cleanup_candidates`，保证预览 = 实际
4. status = rolled_back；层目录保留；栈深 −1
```

- 不提供「恢复到指定 seq」；要撤多层就从顶往下连续执行

### Preview（只读，必须先确认）

- `preview_apply`：覆盖 N 个 / 新增 M 个 / 未触及 K 个 + 字节数 + 路径列表（截断 200）
- `preview_rollback`：将恢复 N 个 / 将删除 M 个 / 将清理 D 个空目录 + 路径列表（截断 200）
- 恢复预览最多打印 200 条再提示剩余数量（CLI / GUI 同步此限制）

## 模块划分

### backup-core

| 模块 | 职责 |
|---|---|
| `error.rs` | `BackupError`（thiserror）：`ScanFailed` `ProjectNotFound` `LayerNotFound` `StackEmpty` `StatusConflict` `InvalidRelPath` `FileLocked` `Io` `Json` `Other` |
| `platform.rs` | 长路径 `to_long_path` / `strip_verbatim` / `join_rel` / `replace_file` |
| `walk.rs` | jwalk 结构遍历：收集 rel_path、size、mtime + 目录清单（scan_fs）；跳符号链接；错误聚合 → `ScanFailed` |
| `project.rs` | 读写 projects.json；创建 / 列表 / 删除项目；获取释放 `.lock` |
| `layer.rs` | meta.json 原子读写；层目录命名；状态机转换 |
| `preview.rs` | `preview_apply` / `preview_rollback` |
| `apply.rs` | Apply 管线（见上） |
| `pop_layer.rs` | Rollback 管线（见上） |

### backup-cli

全局 `--root`；中文帮助与输出；失败退出码非 0。

```text
backup-cli project add <name> <base_dir>
backup-cli project list
backup-cli project rm <name> [--yes] [--purge]   # 默认只删索引；--purge 才删层数据

backup-cli preview <project> <mod_src>
backup-cli apply <project> <mod_src> [--note] [--yes]
backup-cli status <project>
backup-cli restore <project> [--yes]
backup-cli layer show <project> <seq>
backup-cli layer rm <project> <seq> [--yes]   # 仅删除已恢复层，含备份文件
```

### GUI

保留窗口壳：TitleBar / Sidebar / StatusBar / Toast / ProgressOverlay / Acrylic / capabilities / settings 骨架；`styles.css` 视觉不动。

页面：

1. **项目页**：列表 + 新建（选 base 目录）+ 删除
2. **项目详情页**：栈列表（新 → 旧：seq、mod 名、时间、状态、覆盖/新增统计）；
   「应用 Mod…」「恢复上一层」；栈列表标注「当前顶层」徽章；栈深徽章；空栈时恢复禁用
3. **设置页**：backup_root 路径

IPC：

```
get_settings / set_settings
list_projects / create_project / remove_project
preview_apply / apply_mod
list_layers / preview_rollback / rollback_top / remove_layer
```

进度事件名 `progress`，载荷 `{ op, stage, done, total }`（total=0 表示阶段无分母）：

- Apply：`扫描` → `备份交集` → `写入记录` → `拷入 Mod` → `完成`
- Rollback：`恢复文件` → `删除新增` → `完成`

对话框：`ModApplyPreview` / `RollbackPreview` / `Confirm` / `ProjectDialog`。  
错误统一 `AppError { message, kind, hint }`，按 kind 给中文操作建议。

## Windows 实现要点（直接照做）

- 核心层一律存普通 `PathBuf`；文件 I/O 经 `platform::to_long_path()` 加 `\\?\`
  （UNC 转 `\\?\UNC\`）
- `\\?\` 下 NT 不把 `/` 当分隔符：相对路径拼目录必须用 `platform::join_rel()`
  （换 `MAIN_SEPARATOR`），否则 os error 123
- `std::fs::canonicalize` 返回带 `\\?\` → 展示 / 入库前 `platform::strip_verbatim()`
- 覆盖替换用 `platform::replace_file()`（`MoveFileExW` + REPLACE_EXISTING | WRITE_THROUGH）
- jwalk 自带长路径处理：传普通路径，不要预加 `\\?\`
- 文件锁检测：raw_os_error 32 / 33（ERROR_SHARING_VIOLATION / LOCK_VIOLATION）
- `rel_path` 存正斜杠；拷贝落盘时经 `join_rel` 换成分隔符

## 测试约定

- fixture：临时目录构造 base + mod 夹具；`type Tree = Vec<(&'static str, &'static [u8])>`
- 每个 e2e 独立 tempdir
- Windows-only 测试加 `#[cfg(windows)]`（如 share_mode(0) 锁文件）
- 预览截断 200 条与 CLI / GUI 同步

### 必测场景

1. 只增不改：inter 为空；apply 后 restore 把新增删干净
2. 部分覆盖 + 部分新增：备份仅交集；restore 后路径集 == structure_before，
   被覆盖文件内容还原
3. 两层叠加 LIFO：恢复顶 = 回到仅 A 状态；再恢复 = 干净底包
4. 不存在跳层接口
5. 空栈 restore → `StackEmpty`
6. 崩溃注入：`backed_up` 未 `applied`（底包未脏）；`applied` 中途失败可 restore 收敛
7. 并发第二实例 apply 被 `.lock` 拒绝
8. mod 内含 `../evil` → 拒绝且零写入
9. 长路径 `\\?\` 往返
10. preview 计数与 apply 实际一致（抽样）
11. 恢复清理多级新目录：mod 带 `a/b/c/new.txt` → 恢复后目录集 == 应用前
12. 原有目录保护：原有空目录与「原有目录+本层新文件」恢复后都在；本层纯新目录被清
13. 旧层兼容：meta 无 `dirs_before` → 按 `added` 祖先清理新目录链；原有空目录（非祖先）不删
14. 仅 `rolled_back` 层可删除；`applied`/`restoring` 删除被拒（StatusConflict）；删除后层目录消失、底包不受影响

## 门禁（完成才算做完）

```powershell
cargo build --workspace
cargo test --workspace
cargo clippy --workspace --all-targets   # 必须零警告
npm run typecheck
npm test
npm run build
```

## 实施顺序

> **执行约定（已确认）**：本轮先执行 **阶段 0–3**（脚手架 → core → 管线+测试 → CLI），
> 全部门禁通过并人工确认后，再进入阶段 4–5。

| 阶段 | 内容 | 验收 |
|---|---|---|
| 0 | 脚手架：Cargo workspace + backup-core/backup-cli 空壳 + Vite/React/TS/Vitest + Tauri v2 壳 | `cargo check --workspace` |
| 1 | core：error / platform / walk / project / layer | `cargo check` |
| 2 | core：preview / apply / pop_layer + e2e 必测场景 | test 绿，clippy 零警告 |
| 3 | CLI 全量子命令 + 中文冒烟 | preview → apply → restore 通 |
| 4 | GUI：pages / dialogs / commands + typecheck / build | dev 手跑全流程 |
| 5 | 文档：README / AGENTS / todolist 与实现一致 | 全绿 |

## 范围外（明确不做）

- 跳层恢复、多分支、摘中间层
- 内容去重 / 压缩 / 校验哈希 / 数据库
- 全目录内容快照（仅结构 + 交集备份）
- 定时备份、托盘（按需后续再议）

## 里程碑

- **阶段 1–2**：core 引擎 + 测试
- **阶段 3**：CLI 交付
- **阶段 4**：GUI 交付
- **阶段 5**：文档收口
