# Builds the signed Windows installer into the "installer" folder.
# Usage:  powershell -ExecutionPolicy Bypass -File scripts\build-installer.ps1

$ErrorActionPreference = 'Stop'
$root = Split-Path $PSScriptRoot -Parent
$env:Path = "$env:USERPROFILE\.cargo\bin;$env:Path"

# Updates are signed so the app only installs files that really come from us.
# The private key lives outside the project and must never be committed.
if (-not $env:TAURI_SIGNING_PRIVATE_KEY) {
    $keyFile = "$env:USERPROFILE\.tauri\jewelry-studio.key"
    if (-not (Test-Path $keyFile)) { throw "The update-signing key is missing: $keyFile" }
    $env:TAURI_SIGNING_PRIVATE_KEY = (Get-Content $keyFile -Raw).Trim()
    $env:TAURI_SIGNING_PRIVATE_KEY_PASSWORD = ''
}

# Two workarounds for Rust's GNU toolchain. Neither is needed with the MSVC
# toolchain (Visual Studio C++ Build Tools), and both are harmless there.
# 1. Its resource compiler comes from the WinLibs MinGW package.
$winlibs = Get-ChildItem "$env:LOCALAPPDATA\Microsoft\WinGet\Packages" -Directory -Filter 'BrechtSanders.WinLibs*' -ErrorAction SilentlyContinue |
    Select-Object -First 1
if ($winlibs) { $env:Path = "$($winlibs.FullName)\mingw64\bin;$env:Path" }
# 2. That resource compiler fails when the build folder's path contains spaces.
if ($root -match ' ' -and -not $env:CARGO_TARGET_DIR) {
    $env:CARGO_TARGET_DIR = "$env:USERPROFILE\.cargo\targets\jewelry-studio"
}

Push-Location "$root\app"
try {
    npm install --no-fund --no-audit
    if ($LASTEXITCODE -ne 0) { throw 'npm install failed' }
    npx tauri build
    if ($LASTEXITCODE -ne 0) { throw 'The build failed. See the messages above.' }
}
finally { Pop-Location }

# The version is set in one place: [workspace.package] in the root Cargo.toml.
$version = [regex]::Match((Get-Content "$root\Cargo.toml" -Raw), '(?m)^version = "(.+)"').Groups[1].Value
$target = if ($env:CARGO_TARGET_DIR) { $env:CARGO_TARGET_DIR } else { "$root\target" }
$setup = Get-ChildItem "$target\release\bundle\nsis\*_${version}_*-setup.exe" | Select-Object -First 1
if (-not $setup) { throw "No installer for version $version was produced." }

New-Item -ItemType Directory -Force "$root\installer" | Out-Null
$name = "JewelryStudio-$version-setup.exe"
Copy-Item $setup.FullName "$root\installer\$name" -Force
Copy-Item "$($setup.FullName).sig" "$root\installer\$name.sig" -Force
Write-Host "`nInstaller ready: $root\installer\$name"
