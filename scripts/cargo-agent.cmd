@echo off
rem Tauri passes build/run first; forward the complete arguments to the shared Cargo entry.
bash "%~dp0cargo-agent.sh" %*
exit /b %errorlevel%
