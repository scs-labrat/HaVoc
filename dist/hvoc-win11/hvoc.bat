@echo off
title HVOC - Veilid P2P Forum
echo Starting HVOC...
echo.
echo   Web UI will open automatically at http://127.0.0.1:7734
echo   Press Ctrl+C to stop the server.
echo.
"%~dp0hvoc-cli.exe" serve
