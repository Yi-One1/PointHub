<#
.SYNOPSIS
    构建积分枢纽 PointHub 的 Windows 产物。

.DESCRIPTION
    默认只产出「便携 exe」（需求第 12 条：日常只要这一种）。
    加 -Installer 才额外产出 NSIS 安装包。

    产物命名遵循「中文名 + 英文名 + _v版本」(需求第 16 条)：
      积分枢纽PointHub_v0.1.0.exe
      积分枢纽PointHub_v0.1.0_Setup.exe   (仅 -Installer)

.EXAMPLE
    powershell -ExecutionPolicy Bypass -File scripts/build-windows.ps1

.EXAMPLE
    powershell -ExecutionPolicy Bypass -File scripts/build-windows.ps1 -Installer
#>
param(
    [switch]$Installer,
    [switch]$SkipTests
)

$ErrorActionPreference = "Stop"

$root = Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path)
Set-Location $root

# --- 版本号单点来源：workspace.package.version ------------------------------
$cargoToml = Get-Content (Join-Path $root "Cargo.toml") -Raw
if ($cargoToml -notmatch '(?m)^version\s*=\s*"([^"]+)"') {
    throw "无法从 Cargo.toml 读到版本号"
}
$version = $Matches[1]
$baseName = "积分枢纽PointHub_v$version"
Write-Host "版本：$version" -ForegroundColor Cyan

# --- 1. 校验 ----------------------------------------------------------------
if (-not $SkipTests) {
    Write-Host "[1/4] 运行测试..." -ForegroundColor Cyan
    cargo test --workspace --quiet
    if ($LASTEXITCODE -ne 0) { throw "测试未通过，终止构建" }
} else {
    Write-Host "[1/4] 跳过测试（-SkipTests）" -ForegroundColor Yellow
}

# --- 2. 前端 ----------------------------------------------------------------
Write-Host "[2/4] 构建前端..." -ForegroundColor Cyan
Push-Location (Join-Path $root "apps/desktop/ui")
if (-not (Test-Path "node_modules")) { npm install --no-audit --no-fund }
npm run build
if ($LASTEXITCODE -ne 0) { Pop-Location; throw "前端构建失败" }
Pop-Location

# --- 3. Rust 发布构建 -------------------------------------------------------
Write-Host "[3/4] 编译 release..." -ForegroundColor Cyan
cargo build --release --manifest-path (Join-Path $root "apps/desktop/src-tauri/Cargo.toml")
if ($LASTEXITCODE -ne 0) { throw "Rust 构建失败" }

$distDir = Join-Path $root "dist"
New-Item -ItemType Directory -Force -Path $distDir | Out-Null

$srcExe = Join-Path $root "apps/desktop/src-tauri/target/release/pointhub.exe"
if (-not (Test-Path $srcExe)) { throw "找不到编译产物：$srcExe" }

$portable = Join-Path $distDir "$baseName.exe"
Copy-Item $srcExe $portable -Force
$sizeMb = [math]::Round((Get-Item $portable).Length / 1MB, 1)
Write-Host "  便携版：$portable ($sizeMb MB)" -ForegroundColor Green

# --- 4. 可选：安装包 --------------------------------------------------------
if ($Installer) {
    Write-Host "[4/4] 构建安装包..." -ForegroundColor Cyan
    Push-Location (Join-Path $root "apps/desktop/src-tauri")
    npx --yes @tauri-apps/cli build --bundles nsis
    if ($LASTEXITCODE -ne 0) { Pop-Location; throw "安装包构建失败" }
    Pop-Location

    $setup = Get-ChildItem (Join-Path $root "apps/desktop/src-tauri/target/release/bundle/nsis") -Filter "*.exe" |
        Sort-Object LastWriteTime -Descending | Select-Object -First 1
    if (-not $setup) { throw "没找到 NSIS 安装包" }
    $destSetup = Join-Path $distDir "${baseName}_Setup.exe"
    Copy-Item $setup.FullName $destSetup -Force
    Write-Host "  安装包：$destSetup" -ForegroundColor Green
} else {
    Write-Host "[4/4] 跳过安装包（需要时加 -Installer）" -ForegroundColor Yellow
}

Write-Host ""
Write-Host "完成。产物目录：$distDir" -ForegroundColor Cyan
Get-ChildItem $distDir | Format-Table Name, @{N = "MB"; E = { [math]::Round($_.Length / 1MB, 1) } }
