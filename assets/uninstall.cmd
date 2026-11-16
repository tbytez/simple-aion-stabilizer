@echo off
rem Stop the app, the engine and the WinDivert driver so this folder
rem can be deleted without a reboot. Right-click -> Run as administrator.
rem Call system tools by absolute System32 path - PATH is user-controlled.
"%SystemRoot%\System32\taskkill.exe" /f /im AionStabilizer.exe >nul 2>&1
"%SystemRoot%\System32\taskkill.exe" /f /im winws.exe >nul 2>&1
"%SystemRoot%\System32\sc.exe" stop windivert >nul 2>&1
"%SystemRoot%\System32\sc.exe" stop windivert14 >nul 2>&1
"%SystemRoot%\System32\sc.exe" delete windivert >nul 2>&1
"%SystemRoot%\System32\sc.exe" delete windivert14 >nul 2>&1
echo App and driver stopped. You can now delete this folder.
pause
