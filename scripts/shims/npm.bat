@echo off
REM ---------------------------------------------------------------------------
REM npm.bat shim for Tauri Android builds.
REM
REM Why: Tauri's Gradle rust plugin hardcodes `npm.bat`, but modern Node for
REM Windows ships only `npm.cmd`, and Java's ProcessBuilder does not perform
REM PATHEXT lookup, so it fails with "A problem occurred starting process".
REM
REM This file only forwards to the real npm.cmd. Keep the directory free of any
REM npm.cmd, otherwise this shim will call itself in a loop.
REM
REM Keep this file ASCII-only: cmd.exe reads .bat in the OEM codepage and will
REM mis-parse non-ASCII bytes.
REM ---------------------------------------------------------------------------
call npm.cmd %*
