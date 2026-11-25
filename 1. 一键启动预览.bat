@echo off
setlocal
chcp 65001 >nul
title PointHub - Preview (run from source, no packaging)

set "ROOT=%~dp0"
cd /d "%ROOT%"

echo ================================================================
echo   PointHub  -  Preview build from current source
echo ----------------------------------------------------------------
echo   This is the UNPACKAGED version: it compiles and runs the code
echo   in this folder right now, so you can check changes without
echo   producing a new exe under dist\.
echo.
echo   This black window is the log window. Keep it open.
echo   Close it (or press Ctrl+C) to stop the app.
echo ================================================================
echo.

if not exist "%ROOT%apps\desktop\ui\package.json" goto wrongdir
if not exist "%ROOT%apps\desktop\src-tauri\Cargo.toml" goto wrongdir

echo [1/3] Building frontend...
cd /d "%ROOT%apps\desktop\ui"
if not exist "node_modules" (
    echo        first run: installing npm packages, this can take a few minutes...
    call npm install --no-audit --no-fund
    if errorlevel 1 goto fail
)
call npm run build
if errorlevel 1 goto fail

echo.
echo [2/3] Compiling Rust (dev profile)...
cd /d "%ROOT%apps\desktop\src-tauri"
cargo build
if errorlevel 1 goto fail
echo.

echo.
echo [3/3] Starting PointHub...
echo       A main window plus a floating ball should appear.
echo.
cargo run
if errorlevel 1 goto fail
goto done


:wrongdir
echo [ERROR] Keep this file in the PointHub project root folder.
pause
exit /b 1

:fail
echo.
echo [FAILED] See the messages above.
echo          Send me the last 20 lines and I will fix it.
pause
exit /b 1

:done
echo.
echo App closed.
pause
