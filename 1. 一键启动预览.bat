@echo off
chcp 65001 >nul
title PointHub - Preview
cd /d "%~dp0"
powershell -NoProfile -ExecutionPolicy Bypass -File "scripts\preview.ps1" %*
if errorlevel 1 pause
