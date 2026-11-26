# Single-instance check: launch twice, the second must exit and only one process remains.
# Usage: powershell -ExecutionPolicy Bypass -File scripts\test-single-instance.ps1
# NOTE: keep this file ASCII-only; PowerShell 5 reads BOM-less UTF-8 as ANSI.
param([string]$Exe = "apps\desktop\src-tauri\target\debug\pointhub.exe")

$ErrorActionPreference = "Stop"
$root = Split-Path $PSScriptRoot -Parent
$exe = if ([System.IO.Path]::IsPathRooted($Exe)) { $Exe } else { Join-Path $root $Exe }
if (-not (Test-Path $exe)) {
    Write-Output "SKIP executable not found: $exe (run cargo build first)"
    exit 2
}

# A running instance of the same app (even another build) owns the single-instance lock.
$other = @(Get-Process -Name pointhub -ErrorAction SilentlyContinue | Where-Object { $_.Path -ne $exe })
if ($other.Count -gt 0) {
    Write-Output "SKIP another pointhub.exe is running, close it first"
    exit 3
}

$d1 = Join-Path $env:TEMP ("ph-si-a-" + [guid]::NewGuid().ToString("n"))
$d2 = Join-Path $env:TEMP ("ph-si-b-" + [guid]::NewGuid().ToString("n"))

$p1 = Start-Process -FilePath $exe -ArgumentList @("--data-dir", $d1) -PassThru
if ($null -eq $p1) {
    Write-Output "FAIL first Start-Process returned no process object"
    exit 1
}
$id1 = [int]$p1.Id
Start-Sleep -Seconds 10
if ($p1.HasExited) {
    Write-Output "FAIL first instance exited on its own (exit=$($p1.ExitCode))"
    exit 1
}

$sw = [Diagnostics.Stopwatch]::StartNew()
$p2 = Start-Process -FilePath $exe -ArgumentList @("--data-dir", $d2) -PassThru
if ($null -eq $p2) {
    Write-Output "FAIL second Start-Process returned no process object"
    Stop-Process -Id $id1 -Force -ErrorAction SilentlyContinue
    exit 1
}
$id2 = [int]$p2.Id
if (-not $p2.WaitForExit(15000)) {
    Write-Output "FAIL second instance still alive after 15s (pid=$id2), single-instance not working"
    Stop-Process -Id $id2 -Force -ErrorAction SilentlyContinue
    Stop-Process -Id $id1 -Force -ErrorAction SilentlyContinue
    exit 1
}
$sw.Stop()

$alive = @(Get-Process -Id $id1 -ErrorAction SilentlyContinue)
if ($alive.Count -ne 1) {
    Write-Output "FAIL first instance disappeared, expected exactly one process"
    exit 1
}

Write-Output ("PASS second instance exited in {0:N1}s (exit={1}), first still running" -f $sw.Elapsed.TotalSeconds, $p2.ExitCode)
Stop-Process -Id $id1 -Force -ErrorAction SilentlyContinue
Remove-Item $d1, $d2 -Recurse -Force -ErrorAction SilentlyContinue
