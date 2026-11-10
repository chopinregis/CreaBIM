# Installs CreaBIM for the current user (no administrator rights needed):
# the app in %LOCALAPPDATA%\Programs\CreaBIM, Start menu and desktop
# shortcuts, and an entry in Settings > Apps to uninstall it.
$ErrorActionPreference = "Stop"
$here = Split-Path -Parent $MyInvocation.MyCommand.Path
$dest = Join-Path $env:LOCALAPPDATA "Programs\CreaBIM"
$version = (Get-Content (Join-Path $here "VERSION.txt") -Raw).Trim()

Get-Process -Name "CreaBIM" -ErrorAction SilentlyContinue | Stop-Process -Force
New-Item -ItemType Directory -Force $dest | Out-Null
foreach ($f in "CreaBIM.exe", "creabim-cli.exe", "LICENSE", "README.md", "READ ME FIRST.txt", "uninstall.ps1", "VERSION.txt") {
    Copy-Item (Join-Path $here $f) (Join-Path $dest $f) -Force
}

$exe = Join-Path $dest "CreaBIM.exe"
$shell = New-Object -ComObject WScript.Shell
foreach ($dir in [Environment]::GetFolderPath("Programs"), [Environment]::GetFolderPath("Desktop")) {
    $lnk = $shell.CreateShortcut((Join-Path $dir "CreaBIM.lnk"))
    $lnk.TargetPath = $exe
    $lnk.WorkingDirectory = $dest
    $lnk.IconLocation = "$exe,0"
    $lnk.Description = "CreaBIM, open-source BIM modeller"
    $lnk.Save()
}

$key = "HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\CreaBIM"
New-Item -Path $key -Force | Out-Null
$size = [int]((Get-ChildItem $dest | Measure-Object Length -Sum).Sum / 1KB)
$props = @{
    DisplayName = "CreaBIM"; DisplayVersion = $version; Publisher = "Regis Benoit Brice Nde Tene";
    DisplayIcon = "$exe,0"; InstallLocation = $dest; EstimatedSize = $size; NoModify = 1; NoRepair = 1;
    URLInfoAbout = "https://github.com/chopinregis/CreaBIM";
    UninstallString = "powershell.exe -NoProfile -ExecutionPolicy Bypass -File `"$dest\uninstall.ps1`""
}
foreach ($k in $props.Keys) { New-ItemProperty -Path $key -Name $k -Value $props[$k] -Force | Out-Null }

Start-Process $exe
