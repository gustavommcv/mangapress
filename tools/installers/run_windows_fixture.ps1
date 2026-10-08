# Mock downloads only: hashing, ZIP extraction, execution, and installation stay native.
$ErrorActionPreference = "Stop"
$Catalog = Get-Content -LiteralPath $env:INSTALLER_CATALOG -Encoding UTF8 -Raw | ConvertFrom-Json
function Record-Download([string]$Uri) {
    Add-Content -LiteralPath $Catalog.log -Value $Uri -Encoding UTF8
    if ($Catalog.fail_urls -contains $Uri) { throw "Fixture download failed" }
}
function Invoke-RestMethod([string]$Uri) {
    Record-Download $Uri
    if ($Uri -ne $Catalog.latest_api) { throw "Unexpected fixture API URL: $Uri" }
    return [PSCustomObject]@{ tag_name = $Catalog.latest_tag }
}
function Invoke-WebRequest([string]$Uri, [string]$OutFile, [switch]$UseBasicParsing) {
    Record-Download $Uri
    $Entry = $Catalog.files.PSObject.Properties[$Uri]
    if ($null -eq $Entry) { throw "Unexpected fixture download URL: $Uri" }
    Copy-Item -LiteralPath $Entry.Value -Destination $OutFile
}

$PathBefore = [Environment]::GetEnvironmentVariable("Path", "User")
try {
    # Match the documented download-and-iex entry point without changing execution policy.
    Invoke-Expression ([IO.File]::ReadAllText($env:INSTALLER_SCRIPT))
} finally {
    if ([Environment]::GetEnvironmentVariable("Path", "User") -cne $PathBefore) {
        throw "The isolated installer changed the user PATH."
    }
}
