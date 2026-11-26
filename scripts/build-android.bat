@echo off
REM ===========================================================================
REM  PointHub - Android build script
REM
REM  Three Windows-specific traps this script works around:
REM   1) Non-ASCII project path breaks NDK ld.lld (cannot open object files)
REM      -> point CARGO_TARGET_DIR to an ASCII-only directory
REM   2) Gradle refuses non-ASCII project paths
REM      -> android.overridePathCheck=true in gen/android/gradle.properties
REM   3) Tauri's Gradle rust plugin hardcodes `npm.bat` and `node.bat`, but
REM      modern Node ships only `npm.cmd` / `node.exe`, and Java does not do
REM      PATHEXT lookup
REM      -> scripts\shims\npm.bat + node.bat shims, put first on PATH
REM
REM  NOTE: keep this file ASCII-only. cmd.exe reads .bat in the OEM codepage;
REM  UTF-8 Chinese comments here will corrupt parsing.
REM ===========================================================================
setlocal enabledelayedexpansion

if "%ANDROID_HOME%"=="" set "ANDROID_HOME=D:\APP\Android\AndroidSDK"
if "%JAVA_HOME%"==""    set "JAVA_HOME=D:\APP\Android\Android Studio\jbr"
if "%NDK_HOME%"==""     set "NDK_HOME=%ANDROID_HOME%\ndk\30.0.16248370"
if "%CARGO_TARGET_DIR%"=="" set "CARGO_TARGET_DIR=D:\ph-build\target"

set "SHIM_DIR=%~dp0shims"
set "PATH=%SHIM_DIR%;%PATH%"
set "APP_DIR=%~dp0..\apps\desktop"

echo [1/3] environment check
if not exist "%ANDROID_HOME%" ( echo   ERROR: ANDROID_HOME not found: %ANDROID_HOME% & exit /b 1 )
if not exist "%JAVA_HOME%\bin\java.exe" ( echo   ERROR: JAVA_HOME not found: %JAVA_HOME% & exit /b 1 )
if not exist "%SHIM_DIR%\npm.bat" ( echo   ERROR: npm shim missing: %SHIM_DIR%\npm.bat & exit /b 1 )
if not exist "%SHIM_DIR%\node.bat" ( echo   ERROR: node shim missing: %SHIM_DIR%\node.bat & exit /b 1 )
if not exist "%CARGO_TARGET_DIR%" mkdir "%CARGO_TARGET_DIR%"
echo   ANDROID_HOME=%ANDROID_HOME%
echo   JAVA_HOME=%JAVA_HOME%
echo   NDK_HOME=%NDK_HOME%
echo   TARGET_DIR=%CARGO_TARGET_DIR%

echo [1/3] sync version
powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp0sync-version.ps1"
if errorlevel 1 ( echo   ERROR: version sync failed & exit /b 1 )

echo [2/3] build frontend
pushd "%APP_DIR%\ui"
call npm run build
if errorlevel 1 ( popd & echo   ERROR: frontend build failed & exit /b 1 )
popd

echo [2/3] gradle patch (tracked; gen/ is gitignored)
powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp0patch-android-gradle.ps1"
if errorlevel 1 ( echo   ERROR: gradle patch failed & exit /b 1 )

echo [2/3] android sources patch (WorkManager worker + notification)
powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp0patch-android-sources.ps1"
if errorlevel 1 ( echo   ERROR: android sources patch failed & exit /b 1 )

echo [2/3] app icons (Windows bundle + Android mipmaps)
where python >nul 2>nul
if errorlevel 1 ( echo   ERROR: python not found on PATH & exit /b 1 )
python "%~dp0gen-app-icons.py" --android "%APP_DIR%\src-tauri\gen\android\app\src\main\res"
if errorlevel 1 ( echo   ERROR: icon generation failed & exit /b 1 )

echo [3/3] build apk
REM   first arg "release" switches to a release build (smaller, stripped)
REM   anything else keeps the debug build
set "MODE=--debug"
set "EXTRA=%*"
if /I "%~1"=="release" (
  REM  release is the CLI default; only --debug has to be passed explicitly
  set "MODE="
  set "EXTRA=%2 %3 %4"
  set "IS_RELEASE=1"
)
pushd "%APP_DIR%"
call npx @tauri-apps/cli android build %MODE% --apk %EXTRA%
set "RC=%ERRORLEVEL%"
popd

if not "%RC%"=="0" (
  echo.
  echo BUILD FAILED exit=%RC%
  exit /b %RC%
)

echo.
echo BUILD OK
if "%IS_RELEASE%"=="1" (
  powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp0copy-android-release.ps1"
)
for /r "%APP_DIR%\src-tauri\gen\android\app\build\outputs\apk" %%f in (*.apk) do echo   %%f
endlocal
