# Fast launcher for the unpackaged build.
#
# Rebuilds only what actually changed, then runs target\debug\pointhub.exe
# directly -- `cargo run` adds a couple of seconds of its own bookkeeping on
# every launch, and `npm run build` is pointless when the UI did not change.
#
# Keep this file pure ASCII: Windows PowerShell 5.1 reads .ps1 files with the
# system ANSI code page unless a BOM is present, so non-ASCII comments can
# corrupt parsing.

param(
    [Parameter(ValueFromRemainingArguments = $true)]
    [string[]]$AppArgs
)

$ErrorActionPreference = "Stop"

# scripts\ -> project root
$root = Split-Path -Parent $PSScriptRoot
$ui = Join-Path $root "apps\desktop\ui"
$crate = Join-Path $root "apps\desktop\src-tauri"
$exe = Join-Path $crate "target\debug\pointhub.exe"

if (-not (Test-Path (Join-Path $ui "package.json"))) {
    Write-Host "[ERROR] preview.ps1 must live in <project>\scripts" -ForegroundColor Red
    exit 1
}

function Newest($items) {
    $items | Where-Object { $_ } | Sort-Object LastWriteTime -Descending | Select-Object -First 1
}

# --- 1. frontend: rebuild only if a source file is newer than dist ----------
$distDir = Join-Path $ui "dist"
$uiInputs = @()
$uiInputs += Get-Item (Join-Path $ui "index.html") -ErrorAction SilentlyContinue
$uiInputs += Get-Item (Join-Path $ui "package.json") -ErrorAction SilentlyContinue
$uiInputs += Get-Item (Join-Path $ui "vite.config.ts") -ErrorAction SilentlyContinue
$uiInputs += Get-ChildItem (Join-Path $ui "src") -Recurse -File -ErrorAction SilentlyContinue
$uiNewest = Newest $uiInputs
$distNewest = Newest (Get-ChildItem $distDir -Recurse -File -ErrorAction SilentlyContinue)

$needUi = (-not $distNewest) -or ($uiNewest -and $uiNewest.LastWriteTime -gt $distNewest.LastWriteTime)
if ($needUi) {
    Write-Host "[1/2] frontend changed, building..." -ForegroundColor Cyan
    Push-Location $ui
    if (-not (Test-Path "node_modules")) {
        Write-Host "      installing npm packages (first run only)..." -ForegroundColor DarkGray
        npm install --no-audit --no-fund
    }
    npm run build
    if ($LASTEXITCODE -ne 0) { Pop-Location; throw "frontend build failed" }
    Pop-Location
    $distNewest = Newest (Get-ChildItem $distDir -Recurse -File)
} else {
    Write-Host "[1/2] frontend unchanged" -ForegroundColor DarkGray
}

# --- 2. rust: rebuild only if a source file is newer than the exe -----------
$rsInputs = @()
$rsInputs += Get-Item (Join-Path $root "Cargo.toml") -ErrorAction SilentlyContinue
$rsInputs += Get-Item (Join-Path $crate "Cargo.toml") -ErrorAction SilentlyContinue
$rsInputs += Get-ChildItem (Join-Path $root "crates") -Recurse -File -Include *.rs, *.toml -ErrorAction SilentlyContinue
$rsInputs += Get-ChildItem (Join-Path $crate "src") -Recurse -File -ErrorAction SilentlyContinue
# Frontend assets are embedded at compile time, so a UI change also needs a rebuild.
$rsInputs += $distNewest
$rsNewest = Newest $rsInputs
$exeFile = Get-Item $exe -ErrorAction SilentlyContinue

$needRs = (-not $exeFile) -or ($rsNewest -and $rsNewest.LastWriteTime -gt $exeFile.LastWriteTime)
if ($needRs) {
    Write-Host "[2/2] rust changed, compiling (about 25s)..." -ForegroundColor Cyan
    Push-Location $crate
    cargo build
    if ($LASTEXITCODE -ne 0) { Pop-Location; throw "rust build failed" }
    Pop-Location
} else {
    Write-Host "[2/2] binary up to date" -ForegroundColor DarkGray
}

Write-Host ""
Write-Host "Starting PointHub (close this window to quit)..." -ForegroundColor Green
& $exe @AppArgs
