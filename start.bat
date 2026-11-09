@echo off
cd /d D:\Code\Tools\ai-token-hud
if exist "target\release\ai-token-hud.exe" (
    start "" "target\release\ai-token-hud.exe"
) else (
    cargo run --release
)
