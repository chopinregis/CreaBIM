# Builds a one-file Windows setup with IExpress (part of Windows; nothing to
# download). Run after `cargo build --release -p creabim-app -p creabim-cli`.
#   powershell -File packaging\windows\make-setup.ps1 -Out C:\path\to\folder
param(
    [string]$Out = ".",
    [string]$Target = "target\release"
)
$ErrorActionPreference = "Stop"
$root = Resolve-Path (Join-Path $PSScriptRoot "..\..")
$version = (Select-String -Path (Join-Path $root "Cargo.toml") -Pattern '^version = "(.+)"').Matches[0].Groups[1].Value
$date = if ($Date) { $Date } else { Get-Date -Format "yyyy-MM-dd" }
$name = "CreaBIM-$version-$date-windows-x64-setup.exe"
$stage = Join-Path $env:TEMP "creabim-setup-stage"
Remove-Item $stage -Recurse -Force -ErrorAction SilentlyContinue
New-Item -ItemType Directory $stage | Out-Null

$bin = if ([IO.Path]::IsPathRooted($Target)) { $Target } else { Join-Path $root $Target }
Copy-Item (Join-Path $bin "creabim-app.exe") (Join-Path $stage "CreaBIM.exe")
Copy-Item (Join-Path $bin "creabim.exe") (Join-Path $stage "creabim-cli.exe")
Copy-Item (Join-Path $root "LICENSE"), (Join-Path $root "README.md") $stage
Copy-Item (Join-Path $root "docs\RELEASE_README.txt") (Join-Path $stage "READ ME FIRST.txt")
Copy-Item (Join-Path $PSScriptRoot "install.ps1"), (Join-Path $PSScriptRoot "uninstall.ps1") $stage
Set-Content (Join-Path $stage "VERSION.txt") $version -Encoding ascii

New-Item -ItemType Directory -Force $Out | Out-Null
$targetExe = Join-Path (Resolve-Path $Out) $name
$files = Get-ChildItem $stage | ForEach-Object { $_.Name }
$i = 0
$strings = ($files | ForEach-Object { "FILE$i=`"$_`""; $i++ }) -join "`r`n"
$i = 0
$list = ($files | ForEach-Object { "%FILE$i%="; $i++ }) -join "`r`n"
$sed = @"
[Version]
Class=IEXPRESS
SEDVersion=3
[Options]
PackagePurpose=InstallApp
ShowInstallProgramWindow=1
HideExtractAnimation=1
UseLongFileName=1
InsideCompressed=0
CAB_FixedSize=0
CAB_ResvCodeSigning=0
RebootMode=N
InstallPrompt=%InstallPrompt%
DisplayLicense=%DisplayLicense%
FinishMessage=%FinishMessage%
TargetName=%TargetName%
FriendlyName=%FriendlyName%
AppLaunched=%AppLaunched%
PostInstallCmd=%PostInstallCmd%
AdminQuietInstCmd=%AdminQuietInstCmd%
UserQuietInstCmd=%UserQuietInstCmd%
SourceFiles=SourceFiles
[Strings]
InstallPrompt=Install CreaBIM $version?
DisplayLicense=$stage\LICENSE
FinishMessage=CreaBIM is installed. It is in the Start menu and on your desktop.
TargetName=$targetExe
FriendlyName=CreaBIM $version
AppLaunched=powershell.exe -NoProfile -ExecutionPolicy Bypass -WindowStyle Hidden -File install.ps1
PostInstallCmd=<None>
AdminQuietInstCmd=
UserQuietInstCmd=
$strings
[SourceFiles]
SourceFiles0=$stage\
[SourceFiles0]
$list
"@
$sedPath = Join-Path $env:TEMP "creabim-setup.sed"
Set-Content $sedPath $sed -Encoding ascii
& "$env:SystemRoot\System32\iexpress.exe" /N /Q $sedPath | Out-Null
$deadline = (Get-Date).AddMinutes(5)
while (-not (Test-Path $targetExe) -and (Get-Date) -lt $deadline) { Start-Sleep -Milliseconds 500 }
Start-Sleep -Seconds 2
Get-Item $targetExe | Select-Object Name, Length
