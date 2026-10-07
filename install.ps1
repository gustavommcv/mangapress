# Installs a verified mangapress release for Windows PowerShell 5.1 or PowerShell 7.
#   irm https://raw.githubusercontent.com/gustavommcv/mangapress/main/install.ps1 | iex
$ErrorActionPreference = "Stop"

$Repo = "gustavommcv/mangapress"
$Target = "x86_64-pc-windows-msvc"
$Asset = "mangapress-$Target.zip"
$Version = if ($env:MANGAPRESS_VERSION) { $env:MANGAPRESS_VERSION } else { "latest" }
$InstallDir = if ($env:MANGAPRESS_INSTALL_DIR) {
    $env:MANGAPRESS_INSTALL_DIR
} else {
    Join-Path $env:LOCALAPPDATA "Programs\mangapress"
}
$InstallDir = [System.IO.Path]::GetFullPath($InstallDir)

if ($Version -ceq "latest") {
    $Release = Invoke-RestMethod -Uri "https://api.github.com/repos/$Repo/releases/latest"
    $Version = $Release.tag_name
}
if (-not $Version -or $Version -isnot [string]) {
    throw "Could not resolve the release tag."
}
if (-not $Version.StartsWith("v", [StringComparison]::Ordinal)) {
    $Version = "v$Version"
}
if ($Version -cnotmatch '^v[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.-]+)?(\+[0-9A-Za-z.-]+)?\z') {
    throw "Invalid release version '$Version'."
}
$Url = "https://github.com/$Repo/releases/download/$Version"

$TempRoot = [System.IO.Path]::GetFullPath([System.IO.Path]::GetTempPath())
$TempName = "mangapress-install-$([Guid]::NewGuid().ToString('N'))"
$TempDir = Join-Path $TempRoot $TempName
New-Item -ItemType Directory -Path $TempDir | Out-Null
try {
    $TmpZip = Join-Path $TempDir $Asset
    $ChecksumFile = Join-Path $TempDir "checksums.txt"
    Write-Host "Downloading $Asset from $Version..."
    Invoke-WebRequest -UseBasicParsing -Uri "$Url/checksums.txt" -OutFile $ChecksumFile
    Invoke-WebRequest -UseBasicParsing -Uri "$Url/$Asset" -OutFile $TmpZip

    $Entries = @(Get-Content -LiteralPath $ChecksumFile -Encoding UTF8 | Where-Object {
        ($_ -split '\s+').Count -ge 2 -and ($_ -split '\s+')[1] -cin @($Asset, "*$Asset")
    })
    $ChecksumPattern = '^([0-9a-fA-F]{64}) [ *]' + [regex]::Escape($Asset) + '\z'
    if ($Entries.Count -ne 1 -or $Entries[0] -cnotmatch $ChecksumPattern) {
        throw "Missing, duplicate, or malformed checksum for $Asset."
    }
    $ExpectedHash = $Matches[1]
    if ((Get-FileHash -LiteralPath $TmpZip -Algorithm SHA256).Hash -ine $ExpectedHash) {
        throw "Checksum verification failed for $Asset."
    }

    $StageDir = Join-Path $TempDir "extracted"
    Expand-Archive -LiteralPath $TmpZip -DestinationPath $StageDir
    $StagedBinary = Join-Path $StageDir "mangapress.exe"
    $Binary = Get-Item -LiteralPath $StagedBinary
    if ($Binary.PSIsContainer -or ($Binary.Attributes -band [IO.FileAttributes]::ReparsePoint)) {
        throw "The archive does not contain a regular mangapress.exe executable."
    }
    $ActualVersion = & $StagedBinary --version
    if ($LASTEXITCODE -ne 0 -or [string]$ActualVersion -cne "mangapress $($Version.Substring(1))") {
        throw "The downloaded executable does not match $Version."
    }

    [System.IO.Directory]::CreateDirectory($InstallDir) | Out-Null
    Copy-Item -LiteralPath $StagedBinary -Destination (Join-Path $InstallDir "mangapress.exe") -Force
    foreach ($Notice in @("LICENSE-MIT", "LICENSE-APACHE", "THIRD-PARTY-NOTICES.md", "DEPENDENCY-LICENSES.txt")) {
        $Source = Join-Path $StageDir $Notice
        $Destination = Join-Path $InstallDir $Notice
        if (Test-Path -LiteralPath $Source -PathType Leaf) {
            Copy-Item -LiteralPath $Source -Destination $Destination -Force
        } elseif (Test-Path -LiteralPath $Destination -PathType Leaf) {
            Remove-Item -LiteralPath $Destination
        }
    }

    if ($env:MANGAPRESS_NO_PATH_UPDATE -ne "1") {
        $UserPath = [Environment]::GetEnvironmentVariable("Path", "User")
        $PathEntries = @()
        if ($UserPath) { $PathEntries = $UserPath -split ";" }
        if (-not ($PathEntries -contains $InstallDir)) {
            $NewPath = if ($UserPath) { "$UserPath;$InstallDir" } else { $InstallDir }
            [Environment]::SetEnvironmentVariable("Path", $NewPath, "User")
            Write-Host "Added $InstallDir to your user PATH. Restart your terminal for it to take effect."
        }
    }
    Write-Host "Installed mangapress to $InstallDir\mangapress.exe"
    Write-Host $ActualVersion
} finally {
    # Only remove the unique directory created above, never an installation folder.
    $ResolvedTemp = [System.IO.Path]::GetFullPath($TempDir)
    if ($ResolvedTemp -ne [System.IO.Path]::GetFullPath((Join-Path $TempRoot $TempName)) -or
        (Split-Path -Leaf $ResolvedTemp) -ne $TempName) {
        throw "Refusing to remove an unexpected temporary path."
    }
    Remove-Item -LiteralPath $ResolvedTemp -Recurse -Force
}
