<#
Signed Windows release: builds the exe here, signs it on the Siam host (`ssh siam`, SSL.com eSigner through
siam-sign.sh), builds the installer there with the uninstaller signed as makensis writes it, signs the installer,
brings it back, writes dist\latest.yml from the signed file and checks the signatures.

    pwsh scripts\release-windows.ps1              # dist\AiQuotaOverlay-<ver>-win-x64-setup.exe + latest.yml
    pwsh scripts\release-windows.ps1 -Upload      # also uploads both to the draft release v<ver> (gh)
    pwsh scripts\release-windows.ps1 -Publish     # uploads, then publishes the draft

Needs: rustup (stable-x86_64-pc-windows-gnu) + WinLibs on PATH for the build, `ssh siam` with the signing
workspace, python, and for -Upload / -Publish the GitHub CLI logged in.
#>
param([switch]$Upload, [switch]$Publish)
$ErrorActionPreference = 'Stop'
$root = Split-Path $PSScriptRoot -Parent
Set-Location $root
$version = (Select-String Cargo.toml -Pattern '^version = "(.+)"').Matches[0].Groups[1].Value
$setup = "AiQuotaOverlay-$version-win-x64-setup.exe"
Write-Host "== Ai Quota Overlay $version"

Write-Host '== build'
cargo build --release --locked
if ($LASTEXITCODE) { throw 'cargo build failed' }

Write-Host '== sign on the Siam host'
$remote = (ssh siam 'mktemp -d /tmp/aqo-release.XXXXXX').Trim()
try {
    ssh siam "mkdir -p $remote/installer $remote/build"
    scp -q target\release\ai-quota-overlay.exe "siam:$remote/ai-quota-overlay.exe"
    scp -q installer\installer.nsi "siam:$remote/installer/installer.nsi"
    scp -q build\icon.ico "siam:$remote/build/icon.ico"
    scp -q scripts\siam-sign.sh "siam:$remote/siam-sign.sh"
    ssh siam "bash $remote/siam-sign.sh $remote/ai-quota-overlay.exe"
    if ($LASTEXITCODE) { throw 'signing the exe failed' }
    ssh siam "cd $remote/installer && makensis -V2 -DVERSION=$version -DEXE_PATH=$remote/ai-quota-overlay.exe -DOUTFILE=$remote/$setup '-DSIGN_UNINSTALLER=bash $remote/siam-sign.sh' installer.nsi"
    if ($LASTEXITCODE) { throw 'makensis failed' }
    ssh siam "bash $remote/siam-sign.sh $remote/$setup"
    if ($LASTEXITCODE) { throw 'signing the installer failed' }
    New-Item -ItemType Directory -Force dist | Out-Null
    scp -q "siam:$remote/$setup" "dist\$setup"
    scp -q "siam:$remote/ai-quota-overlay.exe" "dist\ai-quota-overlay-$version-signed.exe"
}
finally {
    ssh siam "rm -rf $remote"
}

Write-Host '== check signatures'
foreach ($f in "dist\$setup", "dist\ai-quota-overlay-$version-signed.exe") {
    $s = Get-AuthenticodeSignature $f
    Write-Host ("{0}: {1} ({2})" -f (Split-Path $f -Leaf), $s.Status, $s.SignerCertificate.Subject)
    if ($s.Status -ne 'Valid') { throw "$f is not validly signed" }
}

Write-Host '== latest.yml'
python -I scripts\latest-yml.py $version dist\latest.yml "dist\$setup"

if ($Upload -or $Publish) {
    gh release upload "v$version" "dist\$setup" dist\latest.yml --clobber --repo artmasster/ai_quota_overlay
    if ($LASTEXITCODE) { throw 'upload failed' }
}
if ($Publish) {
    gh release edit "v$version" --draft=false --latest --repo artmasster/ai_quota_overlay
    if ($LASTEXITCODE) { throw 'publish failed' }
}
