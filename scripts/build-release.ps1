# Folder Backup 一键打包：MSI 安装版 + 便携版 zip
# 用法：
#   powershell -File scripts/build-release.ps1           # 完整门禁 + 打包
#   powershell -File scripts/build-release.ps1 -SkipGates # 跳过门禁直接打包
#
# 便携版文案来自 assets/portable/ 静态模板（字节级拷贝，不经脚本字符串，
# 避免 PowerShell 5.1 对无 BOM 脚本按 ANSI 解析导致中文乱码）。
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

    # 静态模板字节级拷贝（README 带 BOM / settings 无 BOM，编码在仓库里定死）
    Copy-Item 'assets/portable/README.txt' (Join-Path $stage 'README.txt')
    Copy-Item 'assets/portable/settings.json' (Join-Path $stage 'settings.json')

    # ── 自检：坏包不许出 ────────────────────────────────
    Write-Host '== 便携包自检 ==' -ForegroundColor Cyan
    $rb = [System.IO.File]::ReadAllBytes((Join-Path $stage 'README.txt'))
    if ($rb.Length -lt 3 -or $rb[0] -ne 0xEF -or $rb[1] -ne 0xBB -or $rb[2] -ne 0xBF) {
        throw '自检失败: README.txt 缺少 UTF-8 BOM'
    }
    $readmeText = [System.Text.Encoding]::UTF8.GetString($rb)
    if ($readmeText -notlike '*便携版*') {
        throw '自检失败: README.txt 中文损坏（UTF-8 解码不含「便携版」）'
    }
    $sb = [System.IO.File]::ReadAllBytes((Join-Path $stage 'settings.json'))
    if ($sb.Length -eq 0 -or $sb[0] -eq 0xEF) {
        throw '自检失败: settings.json 带 BOM（serde_json 将无法解析）'
    }
    $sj = Get-Content -Raw (Join-Path $stage 'settings.json') -Encoding UTF8 | ConvertFrom-Json
    if ($null -eq $sj.PSObject.Properties['backup_root']) {
        throw '自检失败: settings.json 缺少 backup_root 字段'
    }
    Write-Host 'README BOM + 中文 OK / settings 无 BOM + JSON OK' -ForegroundColor Green

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
