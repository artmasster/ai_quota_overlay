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
# The Siam host rate-limits SSH (about 6 connections per 30 s), so this uses four: upload, sign + package,
# download, clean up.
$id = -join ((48..57) + (97..122) | Get-Random -Count 10 | ForEach-Object { [char]$_ })
$remote = "/tmp/aqo-release-$id"
$stage = Join-Path ([IO.Path]::GetTempPath()) "aqo-release-$id"
New-Item -ItemType Directory -Force $stage | Out-Null
Copy-Item target\release\ai-quota-overlay.exe "$stage\aqo-$id-ai-quota-overlay.exe"
Copy-Item installer\installer.nsi "$stage\aqo-$id-installer.nsi"
Copy-Item build\icon.ico "$stage\aqo-$id-icon.ico"
Copy-Item scripts\siam-sign.sh "$stage\aqo-$id-siam-sign.sh"
$steps = @(
        'set -e',
        "mkdir -p $remote/installer $remote/build",
        "mv /tmp/aqo-$id-ai-quota-overlay.exe $remote/ai-quota-overlay.exe",
        "mv /tmp/aqo-$id-installer.nsi $remote/installer/installer.nsi",
        "mv /tmp/aqo-$id-icon.ico $remote/build/icon.ico",
        "mv /tmp/aqo-$id-siam-sign.sh $remote/siam-sign.sh",
        "bash $remote/siam-sign.sh $remote/ai-quota-overlay.exe",
        "cd $remote/installer",
        "makensis -V2 -DVERSION=$version -DEXE_PATH=$remote/ai-quota-overlay.exe -DOUTFILE=$remote/$setup '-DSIGN_UNINSTALLER=bash $remote/siam-sign.sh' installer.nsi",
    "bash $remote/siam-sign.sh $remote/$setup",
    ''
) -join "`n"
# LF line endings: the steps run under bash on the host.
[IO.File]::WriteAllText("$stage\aqo-$id-steps.sh", $steps)
try {
    scp -q (Get-ChildItem $stage).FullName 'siam:/tmp/'
    if ($LASTEXITCODE) { throw 'upload failed' }
    ssh siam "bash /tmp/aqo-$id-steps.sh"
    if ($LASTEXITCODE) { throw 'signing or packaging on the Siam host failed' }
    New-Item -ItemType Directory -Force dist | Out-Null
    scp -q "siam:$remote/$setup" "siam:$remote/ai-quota-overlay.exe" $stage
    if ($LASTEXITCODE) { throw 'download failed' }
    Copy-Item "$stage\$setup" "dist\$setup" -Force
    Copy-Item "$stage\ai-quota-overlay.exe" "dist\ai-quota-overlay-$version-signed.exe" -Force
}
finally {
    ssh siam "rm -rf $remote /tmp/aqo-$id-*"
    Remove-Item -Recurse -Force $stage -ErrorAction SilentlyContinue
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
