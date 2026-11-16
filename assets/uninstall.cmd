@echo off
rem Stop the app, the engine and the WinDivert driver so this folder
rem can be deleted without a reboot. Right-click -^> Run as administrator.
taskkill /f /im AionStabilizer.exe >nul 2>&1
taskkill /f /im winws.exe >nul 2>&1
sc stop windivert >nul 2>&1
sc stop windivert14 >nul 2>&1
sc delete windivert >nul 2>&1
sc delete windivert14 >nul 2>&1
echo App and driver stopped. You can now delete this folder.
pause
