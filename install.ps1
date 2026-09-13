# Installs the latest mangapress release for Windows.
#   irm https://raw.githubusercontent.com/gustavommcv/mangapress/main/install.ps1 | iex
$ErrorActionPreference = "Stop"

$Repo = "gustavommcv/mangapress"
$Target = "x86_64-pc-windows-msvc"
$Asset = "mangapress-$Target.zip"
$Url = "https://github.com/$Repo/releases/latest/download/$Asset"
$InstallDir = "$env:LOCALAPPDATA\Programs\mangapress"

New-Item -ItemType Directory -Force -Path $InstallDir | Out-Null

$TmpZip = Join-Path ([System.IO.Path]::GetTempPath()) $Asset
Write-Host "Downloading $Asset..."
Invoke-WebRequest -Uri $Url -OutFile $TmpZip

Expand-Archive -Path $TmpZip -DestinationPath $InstallDir -Force
Remove-Item $TmpZip

$UserPath = [Environment]::GetEnvironmentVariable("Path", "User")
$PathEntries = @()
if ($UserPath) { $PathEntries = $UserPath -split ";" }
if (-not ($PathEntries -contains $InstallDir)) {
    $NewPath = if ($UserPath) { "$UserPath;$InstallDir" } else { $InstallDir }
    [Environment]::SetEnvironmentVariable("Path", $NewPath, "User")
    Write-Host "Added $InstallDir to your user PATH. Restart your terminal for it to take effect."
}

Write-Host "Installed mangapress to $InstallDir\mangapress.exe"
& "$InstallDir\mangapress.exe" --version
