@echo off
REM ---------------------------------------------------------------------------
REM node.bat shim for Tauri Android builds.
REM
REM Why: Tauri's Gradle rust plugin hardcodes `node.bat`, but Node for Windows
REM ships only `node.exe`, and Java's ProcessBuilder does not perform PATHEXT
REM lookup, so it fails with "A problem occurred starting process".
REM
REM This file only forwards to the real node.exe. Keep this directory free of
REM any node.exe, otherwise this shim would shadow the real one.
REM
REM Keep this file ASCII-only: cmd.exe reads .bat in the OEM codepage and will
REM mis-parse non-ASCII bytes.
REM ---------------------------------------------------------------------------
call node.exe %*
exit /b %ERRORLEVEL%
