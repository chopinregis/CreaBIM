# Removes CreaBIM installed by install.ps1: the shortcuts, the Settings > Apps
# entry and the program folder. Projects you saved elsewhere are not touched.
$dest = Join-Path $env:LOCALAPPDATA "Programs\CreaBIM"
Get-Process -Name "CreaBIM" -ErrorAction SilentlyContinue | Stop-Process -Force
foreach ($dir in [Environment]::GetFolderPath("Programs"), [Environment]::GetFolderPath("Desktop")) {
    Remove-Item (Join-Path $dir "CreaBIM.lnk") -ErrorAction SilentlyContinue
}
Remove-Item "HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\CreaBIM" -Recurse -ErrorAction SilentlyContinue
# this script runs from the folder it deletes: remove it a moment later
Start-Process cmd.exe -ArgumentList "/c timeout /t 2 >nul & rd /s /q `"$dest`"" -WindowStyle Hidden
