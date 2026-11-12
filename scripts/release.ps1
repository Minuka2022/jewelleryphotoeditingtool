# Publishes a new version: sets the version number, builds the signed
# installer, pushes the code and creates the GitHub release that installed
# copies of the app update themselves from.
#
# Usage:  powershell -ExecutionPolicy Bypass -File scripts\release.ps1 -Version 0.3.0 -Notes "What changed"

param(
    [Parameter(Mandatory)][string]$Version,
    [string]$Notes = 'Improvements and fixes.'
)

$ErrorActionPreference = 'Stop'
$root = Split-Path $PSScriptRoot -Parent
Set-Location $root

if ($Version -notmatch '^\d+\.\d+\.\d+$') { throw 'The version must look like 0.3.0' }
if (git status --porcelain) { throw 'There are uncommitted changes. Commit them first.' }
if (git tag --list "v$Version") { throw "Version $Version has already been released." }

# The app asks this repository for updates, so releases must be published there.
$config = Get-Content "$root\app\src-tauri\tauri.conf.json" -Raw | ConvertFrom-Json
$repo = [regex]::Match($config.plugins.updater.endpoints[0], 'github\.com/([^/]+/[^/]+)/releases').Groups[1].Value
if (-not $repo) { throw 'Could not read the releases repository from tauri.conf.json.' }

# 1. Set the version (one place: [workspace.package] in the root Cargo.toml).
$cargo = Get-Content "$root\Cargo.toml" -Raw
$updated = ([regex]'(?m)^version = ".+"').Replace($cargo, "version = `"$Version`"", 1)
if ($updated -ne $cargo) {
    try {
        [System.IO.File]::WriteAllText("$root\Cargo.toml", $updated)
        $env:Path = "$env:USERPROFILE\.cargo\bin;$env:Path"
        # Rewrites only this project's own entries in Cargo.lock.
        cargo update --workspace --quiet
        if ($LASTEXITCODE -ne 0) { throw 'Could not update Cargo.lock.' }
        npm --prefix "$root\app" version $Version --no-git-tag-version --allow-same-version | Out-Null
        if ($LASTEXITCODE -ne 0) { throw 'Could not update package.json.' }
    }
    catch {
        # Leave the project as it was, so the release can simply be run again.
        git checkout --quiet -- Cargo.toml Cargo.lock app/package.json app/package-lock.json
        throw
    }
    git commit --all --quiet --message "Release v$Version"
}

# 2. Build the signed installer.
& "$PSScriptRoot\build-installer.ps1"

# 3. Push the code and the version tag.
git tag "v$Version"
git push --quiet origin HEAD "v$Version"
if ($LASTEXITCODE -ne 0) { throw 'Could not push to GitHub.' }

# 4. Publish. "latest.json" is the file the app reads to learn about the update.
$asset = "JewelryStudio-$Version-setup.exe"
$latest = [ordered]@{
    version   = $Version
    notes     = $Notes
    pub_date  = (Get-Date).ToUniversalTime().ToString('yyyy-MM-ddTHH:mm:ssZ')
    platforms = @{
        'windows-x86_64' = [ordered]@{
            signature = (Get-Content "$root\installer\$asset.sig" -Raw).Trim()
            url       = "https://github.com/$repo/releases/download/v$Version/$asset"
        }
    }
}
[System.IO.File]::WriteAllText("$root\installer\latest.json", ($latest | ConvertTo-Json -Depth 5))

gh release create "v$Version" "$root\installer\$asset" "$root\installer\latest.json" `
    --repo $repo --title "Jewelry Studio $Version" --notes $Notes
if ($LASTEXITCODE -ne 0) { throw 'Could not create the GitHub release.' }
Write-Host "`nVersion $Version is published. Installed copies will offer the update the next time they start."
