# Folder Backup 一键打包：MSI 安装版 + 便携版 zip
# 用法：
#   powershell -File scripts/build-release.ps1           # 完整门禁 + 打包
#   powershell -File scripts/build-release.ps1 -SkipGates # 跳过门禁直接打包
param(
    [switch]$SkipGates
)

$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path)
Push-Location $root
try {
    # ── 读取版本（tauri.conf.json 为准）──────────────────
    $conf = Get-Content -Raw 'src-tauri/tauri.conf.json' | ConvertFrom-Json
    $version = $conf.version
    $product = $conf.productName   # "Folder Backup"
    Write-Host "产品: $product  版本: $version" -ForegroundColor Cyan

    # ── 门禁（可跳过）────────────────────────────────────
    if (-not $SkipGates) {
        Write-Host '== 门禁 ==' -ForegroundColor Cyan
        npm run typecheck
        if ($LASTEXITCODE -ne 0) { throw 'typecheck 失败' }
        npm test
        if ($LASTEXITCODE -ne 0) { throw 'npm test 失败' }
        npm run build
        if ($LASTEXITCODE -ne 0) { throw 'vite build 失败' }
        cargo test --workspace
        if ($LASTEXITCODE -ne 0) { throw 'cargo test 失败' }
    }

    # ── 构建 MSI ────────────────────────────────────────
    Write-Host '== tauri build (msi) ==' -ForegroundColor Cyan
    npm run tauri build -- --bundles msi
    if ($LASTEXITCODE -ne 0) { throw 'tauri build 失败' }

    # Cargo workspace 的 target 在仓库根，不在 src-tauri/ 下
    $msiDir = 'target/release/bundle/msi'
    $msi = Get-ChildItem -Path $msiDir -Filter '*.msi' |
        Sort-Object LastWriteTime -Descending | Select-Object -First 1
    if (-not $msi) { throw "未找到 MSI 产物（$msiDir）" }
    Write-Host "MSI: $($msi.FullName) ($([math]::Round($msi.Length/1MB,1)) MB)"

    # ── 组装便携版 zip ──────────────────────────────────
    Write-Host '== 组装便携版 ==' -ForegroundColor Cyan
    $releaseDir = Join-Path $root 'release'
    if (Test-Path $releaseDir) { Remove-Item -Recurse -Force $releaseDir }
    New-Item -ItemType Directory -Force -Path $releaseDir | Out-Null

    $stage = Join-Path $releaseDir 'portable-stage'
    New-Item -ItemType Directory -Force -Path $stage | Out-Null

    # 释放构建的 exe（workspace 根 target）
    $exeSrc = 'target/release/folder-backup.exe'
    if (-not (Test-Path $exeSrc)) { throw "未找到 exe: $exeSrc" }
    Copy-Item $exeSrc (Join-Path $stage 'FolderBackup.exe')

    # 便携 settings 模板：空 backup_root → 运行时回退默认；文件存在即触发便携模式
    '{"backup_root": ""}' | Set-Content -Path (Join-Path $stage 'settings.json') -Encoding UTF8 -NoNewline

    @'
Folder Backup - 便携版
=====================
双击 FolderBackup.exe 即可运行，无需安装。

- 设置保存在本文件夹 settings.json（随 exe 走）
- 层备份数据默认在 %LOCALAPPDATA%\FolderBackup\backups（可在「设置」中修改）
- 依赖 WebView2 运行时（Win10/11 系统自带；缺失时请安装微软 WebView2 Runtime）
'@ | Set-Content -Path (Join-Path $stage 'README.txt') -Encoding UTF8

    $zipName = "FolderBackup_${version}_x64_portable.zip"
    $zipPath = Join-Path $releaseDir $zipName
    Compress-Archive -Path "$stage\*" -DestinationPath $zipPath -Force
    Remove-Item -Recurse -Force $stage

    # ── 汇总到 release/ ────────────────────────────────
    Copy-Item $msi.FullName $releaseDir

    Write-Host ''
    Write-Host '== 产物 ==' -ForegroundColor Green
    Get-ChildItem $releaseDir | ForEach-Object {
        Write-Host ('  {0}  ({1:N1} MB)' -f $_.Name, ($_.Length / 1MB))
    }
}
finally {
    Pop-Location
}
