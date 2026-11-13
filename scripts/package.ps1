# Builds dist/svtoggle-<version>-windows.zip from target\release (run `cargo build --release` first).
# Refuses to package a build without the admin manifest, so a preview build can't ship.
$ErrorActionPreference = 'Stop'
$root = Split-Path $PSScriptRoot
$rel = Join-Path $root 'target\release'
$ver = (Select-String -Path (Join-Path $root 'app\Cargo.toml') -Pattern '^version\s*=\s*"([^"]+)"').Matches[0].Groups[1].Value
$gui = Join-Path $rel 'svtoggle-gui.exe'
foreach ($f in $gui, (Join-Path $rel 'svtoggle.exe')) { if (-not (Test-Path $f)) { throw "missing $f - run cargo build --release" } }
$bytes = [IO.File]::ReadAllBytes($gui)
$text = [Text.Encoding]::ASCII.GetString($bytes) + [Text.Encoding]::Unicode.GetString($bytes)
if ($text -notmatch 'requireAdministrator') { throw 'svtoggle-gui.exe has no requireAdministrator manifest (preview build?) - refusing to package' }
$stage = Join-Path $root 'dist\stage'
Remove-Item $stage -Recurse -Force -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force $stage | Out-Null
Copy-Item $gui, (Join-Path $rel 'svtoggle.exe') $stage
Copy-Item $gui (Join-Path $stage 'svtoggle-portable.exe')
Copy-Item (Join-Path $PSScriptRoot 'install.ps1'), (Join-Path $root 'LICENSE'), (Join-Path $root 'THIRD-PARTY-NOTICES.md'),
    (Join-Path $root 'app\assets\fonts\OFL.txt'), (Join-Path $root 'README.md') $stage
$zip = Join-Path $root "dist\svtoggle-$ver-windows.zip"
Remove-Item $zip -Force -ErrorAction SilentlyContinue
Compress-Archive -Path "$stage\*" -DestinationPath $zip
Remove-Item $stage -Recurse -Force
$h = (Get-FileHash $zip -Algorithm SHA256).Hash.ToLower()
"$h  $(Split-Path $zip -Leaf)" | Set-Content "$zip.sha256" -Encoding ascii
"packaged $zip"
"sha256 $h"
