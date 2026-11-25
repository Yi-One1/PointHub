<#
.SYNOPSIS
    Build the Windows artifacts for PointHub.

.DESCRIPTION
    Produces the portable exe by default. Add -Installer for an NSIS setup too.

    Output naming follows "PointHub_v<version>.exe".
    Artifacts in dist\ are treated as shipped history: this script NEVER
    overwrites or deletes them. To ship again, bump the version first
    (Cargo.toml -> [workspace.package] version), then re-run.

    NOTE: keep this file pure ASCII. Windows PowerShell 5.1 reads .ps1 files
    with the system ANSI code page unless a BOM is present, so non-ASCII
    comments can corrupt parsing (quotes get eaten by mangled multibyte pairs).

.EXAMPLE
    powershell -ExecutionPolicy Bypass -File scripts/build-windows.ps1

.EXAMPLE
    powershell -ExecutionPolicy Bypass -File scripts/build-windows.ps1 -Installer

.EXAMPLE
    # The portable exe for this version already exists in dist\; only build the setup.
    powershell -ExecutionPolicy Bypass -File scripts/build-windows.ps1 -InstallerOnly
#>
param(
    [switch]$Installer,
    [switch]$InstallerOnly,
    [switch]$SkipTests
)

$ErrorActionPreference = "Stop"

$root = Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path)
Set-Location $root

# --- version: single source of truth = workspace.package.version -------------
$cargoToml = Get-Content (Join-Path $root "Cargo.toml") -Raw
if ($cargoToml -notmatch '(?m)^version\s*=\s*"([^"]+)"') {
    throw "Cannot read version from Cargo.toml"
}
$version = $Matches[1]
# Product Chinese name built from code points (U+79EF U+5206 U+67A2 U+7EBD)
# so this script can stay pure ASCII: PS 5.1 mangles non-ASCII sources that
# have no BOM, which silently produces a mojibake file name.
$cnName = -join ([char[]](0x79EF, 0x5206, 0x67A2, 0x7EBD))
$baseName = "${cnName}PointHub_v$version"
Write-Host "Version: $version" -ForegroundColor Cyan

# --- keep the other version fields in sync ----------------------------------
# The version appears in four places. Editing by hand leaves one behind, which
# shows up as "file name says 0.2.0 but the app shows 0.1.0".
$syncTargets = @(
    @{ Path = "apps/desktop/src-tauri/tauri.conf.json"; Label = "tauri.conf.json" },
    @{ Path = "apps/desktop/ui/package.json";            Label = "ui/package.json" }
)
foreach ($t in $syncTargets) {
    $full = Join-Path $root $t.Path
    # -Encoding UTF8 is mandatory: PS 5.1 defaults to the system ANSI code page,
    # which shreds the Chinese strings inside these files.
    $raw = Get-Content $full -Raw -Encoding UTF8
    # PS 5.1's -replace has no script-block form (that is PS 6+); it would write
    # the block's source text into the file. .NET regex replacement is enough.
    $new = [regex]::Replace($raw, '("version"\s*:\s*")[^"]+(")', ('${1}' + $version + '${2}'))
    if ($new -ne $raw) {
        # Must be BOM-less UTF-8: a BOM breaks serde_json parsing of JSON files,
        # while Set-Content -Encoding utf8 in PS 5.1 writes a BOM.
        [System.IO.File]::WriteAllText($full, $new, (New-Object System.Text.UTF8Encoding($false)))
        Write-Host "  synced version in $($t.Label)" -ForegroundColor DarkGray
    }
}

# The desktop crate is outside the workspace, so it does not inherit the version.
$desktopToml = Join-Path $root "apps/desktop/src-tauri/Cargo.toml"
$raw = Get-Content $desktopToml -Raw -Encoding UTF8
$new = [regex]::Replace($raw, '(?m)^(version\s*=\s*")[^"]+(")', ('${1}' + $version + '${2}'))
if ($new -ne $raw) {
    [System.IO.File]::WriteAllText($desktopToml, $new, (New-Object System.Text.UTF8Encoding($false)))
    Write-Host "  synced version in desktop Cargo.toml" -ForegroundColor DarkGray
}

# --- 1. tests ---------------------------------------------------------------
if (-not $SkipTests) {
    Write-Host "[1/4] Running tests..." -ForegroundColor Cyan
    cargo test --workspace --quiet
    if ($LASTEXITCODE -ne 0) { throw "Tests failed, build aborted" }
} else {
    Write-Host "[1/4] Tests skipped (-SkipTests)" -ForegroundColor Yellow
}

# --- 2. frontend ------------------------------------------------------------
Write-Host "[2/4] Building frontend..." -ForegroundColor Cyan
Push-Location (Join-Path $root "apps/desktop/ui")
if (-not (Test-Path "node_modules")) { npm install --no-audit --no-fund }
npm run build
if ($LASTEXITCODE -ne 0) { Pop-Location; throw "Frontend build failed" }
Pop-Location

# --- 3. rust release build --------------------------------------------------
Write-Host "[3/4] Compiling release..." -ForegroundColor Cyan
cargo build --release --manifest-path (Join-Path $root "apps/desktop/src-tauri/Cargo.toml")
if ($LASTEXITCODE -ne 0) { throw "Rust build failed" }

$distDir = Join-Path $root "dist"
New-Item -ItemType Directory -Force -Path $distDir | Out-Null

$srcExe = Join-Path $root "apps/desktop/src-tauri/target/release/pointhub.exe"
if (-not (Test-Path $srcExe)) { throw "Build output not found: $srcExe" }

$portable = Join-Path $distDir "$baseName.exe"
if ($InstallerOnly) {
    Write-Host "  portable: skipped (-InstallerOnly)" -ForegroundColor Yellow
} else {
# dist holds shipped history: never overwrite, never delete.
if (Test-Path $portable) {
    throw @"
Refusing to overwrite an existing artifact:
  $portable

Files under dist\ are treated as released history and must not be replaced.
Bump the version first (Cargo.toml -> [workspace.package] version), then rebuild:
  before 1.0.0: new feature = +MINOR, bug fix = +PATCH, breaking change = +MINOR
"@
}
Copy-Item $srcExe $portable
$sizeMb = [math]::Round((Get-Item $portable).Length / 1MB, 1)
Write-Host "  portable: $portable ($sizeMb MB)" -ForegroundColor Green
}

# --- 4. optional installer --------------------------------------------------
if ($Installer -or $InstallerOnly) {
    Write-Host "[4/4] Building installer..." -ForegroundColor Cyan
    Push-Location (Join-Path $root "apps/desktop/src-tauri")
    npx --yes @tauri-apps/cli build --bundles nsis
    if ($LASTEXITCODE -ne 0) { Pop-Location; throw "Installer build failed" }
    Pop-Location

    $setup = Get-ChildItem (Join-Path $root "apps/desktop/src-tauri/target/release/bundle/nsis") -Filter "*.exe" |
        Sort-Object LastWriteTime -Descending | Select-Object -First 1
    if (-not $setup) { throw "NSIS installer not found" }
    $destSetup = Join-Path $distDir "${baseName}_Setup.exe"
    if (Test-Path $destSetup) {
        throw "Refusing to overwrite an existing installer: $destSetup (bump the version first)"
    }
    Copy-Item $setup.FullName $destSetup
    Write-Host "  installer: $destSetup" -ForegroundColor Green
} else {
    Write-Host "[4/4] Installer skipped (add -Installer when needed)" -ForegroundColor Yellow
}

Write-Host ""
Write-Host "Done. Output directory: $distDir" -ForegroundColor Cyan
Get-ChildItem $distDir | Format-Table Name, @{N = "MB"; E = { [math]::Round($_.Length / 1MB, 1) } }
