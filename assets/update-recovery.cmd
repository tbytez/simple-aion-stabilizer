@echo off
rem Restore AionStabilizer.exe after an interrupted update. If the app
rem was killed or the PC lost power mid-swap, the exe can be left renamed
rem aside - this puts it back. Run normally (no admin needed).
cd /d "%~dp0"
if exist AionStabilizer.exe (
    echo AionStabilizer.exe is present - nothing to repair.
    pause
    exit /b 0
)
rem Prefer the known previous installation when both files exist. The .new
rem file may have been moved aside after failed post-install verification.
if exist AionStabilizer.exe.old (
    ren AionStabilizer.exe.old AionStabilizer.exe
    if errorlevel 1 goto repair_failed
    echo Restored the previous version. Starting it now.
    start "" AionStabilizer.exe
    exit /b 0
)
if exist AionStabilizer.exe.new (
    ren AionStabilizer.exe.new AionStabilizer.exe
    if errorlevel 1 goto repair_failed
    echo Restored the staged version. Starting it now.
    start "" AionStabilizer.exe
    exit /b 0
)
echo No backup found - please reinstall the app.
pause

exit /b 1
:repair_failed
echo Could not restore the executable. Close the app and retry, or reinstall.
pause
exit /b 1
