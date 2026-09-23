# Folder Backup

Windows 优先的 mod 层栈备份 / 恢复工具：在把 mod 拷进游戏底包**之前**，只备份「会被覆盖的那部分原文件」+ 记录底包结构；多层 mod 用栈管理，装坏后按后进先出**恢复**。

- 成本 = 两次结构遍历 + **只拷交集字节**，与游戏总大小解耦
- 无哈希、无去重、无压缩、无数据库 —— 纯文件夹平铺备份
- 提供 **GUI（Tauri v2 + Acrylic）** 与 **CLI**

## 下载

从 [Releases](../../releases) 获取：

| 产物 | 说明 |
|---|---|
| `Folder Backup_x.x.x_x64_en-US.msi` | 安装版，默认装到 `C:\Program Files\Folder Backup\`（需管理员）；设置存 `%LOCALAPPDATA%\FolderBackup\settings.json` |
| `FolderBackup_x.x.x_x64_portable.zip` | 便携版：解压即用；设置随 exe 旁 `settings.json` 走 |

两版的层备份数据默认都在 `%LOCALAPPDATA%\FolderBackup\backups`（可在设置中修改）。运行时需要 WebView2（Win10/11 系统自带）。

## 使用（GUI）

```powershell
npm install
npm run tauri dev
```

1. **新建项目**：绑定一个游戏底包目录（`folder_base`）
2. **应用 Mod…**：选 mod 源目录 → 预览（覆盖 / 新增 / 字节数，路径截断 200）→ 确认后工具先备份再拷入
3. **恢复上一层**：预览（恢复 / 删除 / 清理空目录）→ 确认 → 只 pop 栈顶一层；栈列表标注「当前顶层」
4. **设置**：修改备份根目录

> 应用 / 恢复前请先**关闭游戏**。被锁定的文件：备份阶段严格失败（不拷 mod、列出路径）；恢复阶段逐条报告、不强行覆盖。

## 使用（CLI）

```text
backup-cli --root <备份根> project add <名称> <底包目录>
backup-cli project list
backup-cli project rm <名称> [--yes] [--purge]

backup-cli preview <项目> <mod目录>
backup-cli apply   <项目> <mod目录> [--note] [--yes]
backup-cli status  <项目>
backup-cli restore <项目> [--yes]          # 只恢复栈顶一层
backup-cli layer show <项目> <seq>
backup-cli layer rm   <项目> <seq> [--yes] # 删除已恢复（rolled_back）的层
```

构建与冒烟：

```powershell
cargo build -p backup-cli
.\target\debug\backup-cli.exe --help
```

## 开发

### 前置

- Rust stable（MSVC）、Node.js 18+、VS Build Tools 2022（C++ 桌面开发负载）

### 布局

```
crates/backup-core   引擎（扫描 / 差分 / 层栈 / 恢复管线）
crates/backup-cli    CLI（clap，中文输出）
src-tauri            Tauri v2 后端（IPC / Acrylic / 设置）
src                  React + Vite 前端
scripts              打包脚本
plan.md              产品与架构决策（唯一事实来源）
```

### 门禁（提交前必须全绿）

```powershell
cargo build --workspace
cargo test --workspace
cargo clippy --workspace --all-targets   # 零警告
npm run typecheck
npm test
npm run build
```

## 打包与分发

```powershell
# 本地一键：门禁 + MSI + 便携 zip → release/
powershell -File scripts/build-release.ps1
# 跳过门禁：
powershell -File scripts/build-release.ps1 -SkipGates
```

**自动分发**：推 `v*` tag 触发 GitHub Actions（门禁不过不发版），自动构建 MSI + 便携 zip 并上传 Release：

```powershell
git tag v0.1.0
git push origin v0.1.0
```

## 便携版机制

启动时若 **exe 旁存在 `settings.json`** → 便携模式（设置读写都在 exe 旁）；否则用 `%LOCALAPPDATA%\FolderBackup\settings.json`（MSI / 开发环境）。便携 zip 已自带该文件；备份数据（可达数 GB）两版都默认留在 LOCALAPPDATA，不放 exe 旁边。

## 设计要点

- 层的唯一事实 = `meta.json` + `files/`；恢复只能 pop 栈顶（LIFO），不支持跳层
- 写序：拷交集 → `backed_up` → 拷 mod → `applied`；崩溃不会出现「applied 却无备份」
- 修改类操作持项目 `.lock`（fs2 跨进程排他）；只读预览不加锁
- `rel_path` 一律正斜杠入库；文件 I/O 经 `\\?\` 长路径；覆盖写走 `MoveFileExW` 原子替换

详细决策见 [plan.md](plan.md)。
