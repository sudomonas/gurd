# Installer for gurd on Windows
# Usage:
#   irm https://raw.githubusercontent.com/sudomonas/gurd/main/install.ps1 | iex

$ErrorActionPreference = 'Stop'

$Repo = "sudomonas/gurd"
$BinName = "gurd"
$Target = "x86_64-pc-windows-msvc"

Write-Host "Finding latest release of $Repo..." -ForegroundColor Cyan

$ReleaseUrl = "https://api.github.com/repos/$Repo/releases/latest"
try {
    $Release = Invoke-RestMethod -Uri $ReleaseUrl -UseBasicParsing
    $Version = $Release.tag_name
} catch {
    Write-Host "Failed to query GitHub API; falling back to redirect check..." -ForegroundColor Yellow
    $Request = [System.Net.WebRequest]::Create("https://github.com/$Repo/releases/latest")
    $Request.AllowAutoRedirect = $false
    $Response = $Request.GetResponse()
    $Location = $Response.GetResponseHeader("Location")
    $Version = $Location.Substring($Location.LastIndexOf("/") + 1)
}

if (-not $Version) {
    Write-Error "Could not determine latest version."
}

if (-not $Version.StartsWith("v")) {
    $Version = "v$Version"
}

$Archive = "$BinName-$Target.zip"
$DownloadUrl = "https://github.com/$Repo/releases/download/$Version/$Archive"

$InstallDir = Join-Path $env:LOCALAPPDATA "Programs\gurd"
if (-not (Test-Path $InstallDir)) {
    New-Item -ItemType Directory -Path $InstallDir -Force | Out-Null
}

$TempDir = Join-Path ([System.IO.Path]::GetTempPath()) ([System.Guid]::NewGuid().ToString())
New-Item -ItemType Directory -Path $TempDir -Force | Out-Null

try {
    $ZipPath = Join-Path $TempDir $Archive
    Write-Host "Downloading $BinName $Version for Windows ($Target)..." -ForegroundColor Cyan
    Invoke-WebRequest -Uri $DownloadUrl -OutFile $ZipPath -UseBasicParsing

    Write-Host "Extracting archive..." -ForegroundColor Cyan
    Expand-Archive -Path $ZipPath -DestinationPath $TempDir -Force

    $ExtractedExe = Get-ChildItem -Path $TempDir -Filter "$BinName.exe" -Recurse | Select-Object -First 1
    if (-not $ExtractedExe) {
        Write-Error "Could not find $BinName.exe in extracted archive."
    }

    $DestExe = Join-Path $InstallDir "$BinName.exe"
    Copy-Item -Path $ExtractedExe.FullName -Destination $DestExe -Force

    Write-Host "Successfully installed $BinName to $DestExe" -ForegroundColor Green

    # Check and update User PATH
    $UserPath = [System.Environment]::GetEnvironmentVariable("Path", "User")
    if ($UserPath -split ";" -notcontains $InstallDir) {
        Write-Host "Adding $InstallDir to User PATH..." -ForegroundColor Cyan
        [System.Environment]::SetEnvironmentVariable("Path", "$InstallDir;$UserPath", "User")
        $env:Path = "$InstallDir;$env:Path"
        Write-Host "PATH updated. It will be active in all new terminal windows." -ForegroundColor Green
    }

    Write-Host ""
    & "$DestExe" --version
    Write-Host ""
    Write-Host "Ready! To download or sync the latest drug database, run:" -ForegroundColor Green
    Write-Host "    gurd sync" -ForegroundColor Cyan
} finally {
    Remove-Item -Path $TempDir -Recurse -Force -ErrorAction SilentlyContinue
}
