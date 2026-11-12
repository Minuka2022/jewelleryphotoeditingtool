# Builds the web version and copies it into a folder, ready to double-click.
# Usage:  powershell -ExecutionPolicy Bypass -File scripts\build-web.ps1 [-Destination <folder>]
#
# The program is a single file with the interface inside it. Its data (photos,
# results, styles) lives in a "data" folder next to it and is never touched by
# this script, so a newer copy can simply replace the old one.

param(
    [string]$Destination = 'D:\idearigs\jewellery tol new webbased'
)

$ErrorActionPreference = 'Stop'
$root = Split-Path $PSScriptRoot -Parent
$env:Path = "$env:USERPROFILE\.cargo\bin;$env:Path"

# Workarounds for Rust's GNU toolchain on this machine; harmless elsewhere.
$winlibs = Get-ChildItem "$env:LOCALAPPDATA\Microsoft\WinGet\Packages" -Directory -Filter 'BrechtSanders.WinLibs*' -ErrorAction SilentlyContinue |
    Select-Object -First 1
if ($winlibs) { $env:Path = "$($winlibs.FullName)\mingw64\bin;$env:Path" }
if ($root -match ' ' -and -not $env:CARGO_TARGET_DIR) {
    $env:CARGO_TARGET_DIR = "$env:USERPROFILE\.cargo\targets\jewelry-studio"
}

# 1. The interface, which gets built into the program.
Push-Location "$root\app"
try {
    npm install --no-fund --no-audit
    if ($LASTEXITCODE -ne 0) { throw 'npm install failed' }
    npm run build
    if ($LASTEXITCODE -ne 0) { throw 'The interface did not build.' }
}
finally { Pop-Location }

# 2. The program.
Push-Location $root
try {
    cargo build --release -p jewelry-studio-web
    if ($LASTEXITCODE -ne 0) { throw 'The web program did not build.' }
}
finally { Pop-Location }

# 3. Copy it into place. It cannot be replaced while it is running.
$target = if ($env:CARGO_TARGET_DIR) { $env:CARGO_TARGET_DIR } else { "$root\target" }
$built = "$target\release\jewelry-studio-web.exe"
New-Item -ItemType Directory -Force $Destination | Out-Null
$program = Join-Path $Destination 'Jewelry Studio.exe'
if (Get-Process -ErrorAction SilentlyContinue | Where-Object { $_.Path -eq $program }) {
    throw 'Jewelry Studio is running. Close its window, then run this again.'
}
Copy-Item $built $program -Force
Copy-Item "$root\web-readme.txt" (Join-Path $Destination 'README.txt') -Force
Write-Host "`nReady: $program"
