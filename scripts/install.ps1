# GitNodes installer for Windows (PowerShell).
#
#   Invoke-WebRequest https://raw.githubusercontent.com/AndreaBozzo/gitnodes/main/scripts/install.ps1 -OutFile install-gitnodes.ps1
#   Get-Content .\install-gitnodes.ps1
#   & .\install-gitnodes.ps1
#
# Downloads the prebuilt gitnodes.exe for your platform and puts it on your PATH
# — no Rust toolchain, no compiling, no manual PATH editing.
#
# Overridable via env:
#   GITNODES_REPO     owner/repo to download from (default: AndreaBozzo/gitnodes)
#   GITNODES_VERSION  release tag (default: latest)
#   GITNODES_BIN_DIR  install directory (default: %LOCALAPPDATA%\gitnodes\bin)

$ErrorActionPreference = 'Stop'

$repo    = if ($env:GITNODES_REPO)    { $env:GITNODES_REPO }    else { 'AndreaBozzo/gitnodes' }
$version = if ($env:GITNODES_VERSION) { $env:GITNODES_VERSION } else { 'latest' }
$binDir  = if ($env:GITNODES_BIN_DIR) { $env:GITNODES_BIN_DIR } else { Join-Path $env:LOCALAPPDATA 'gitnodes\bin' }

# --- detect architecture -> Rust target triple -------------------------------
$arch = $env:PROCESSOR_ARCHITECTURE
switch ($arch) {
  'AMD64' { $cpu = 'x86_64' }
  # Windows 11 on ARM can run the published x86_64 binary through x64 emulation.
  'ARM64' { $cpu = 'x86_64' }
  default { throw "unsupported architecture '$arch'" }
}
$target = "$cpu-pc-windows-msvc"
$asset  = "gitnodes-$target.zip"

if ($version -eq 'latest') {
  $url = "https://github.com/$repo/releases/latest/download/$asset"
} else {
  $url = "https://github.com/$repo/releases/download/$version/$asset"
}

$tmp = Join-Path $env:TEMP ("gitnodes-" + [System.Guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $tmp -Force | Out-Null
try {
  $zip = Join-Path $tmp $asset
  Write-Host "Downloading $asset ..."
  Invoke-WebRequest -Uri $url -OutFile $zip -UseBasicParsing

  if ($version -eq 'latest') {
    $sumsUrl = "https://github.com/$repo/releases/latest/download/SHA256SUMS"
  } else {
    $sumsUrl = "https://github.com/$repo/releases/download/$version/SHA256SUMS"
  }
  $sumsFile = Join-Path $tmp 'SHA256SUMS'
  Invoke-WebRequest -Uri $sumsUrl -OutFile $sumsFile -UseBasicParsing
  $escapedAsset = [Regex]::Escape($asset)
  $entry = Get-Content $sumsFile |
    Where-Object { $_ -match "^([0-9a-fA-F]{64})\s+\*?$escapedAsset$" } |
    Select-Object -First 1
  if (-not $entry) { throw "$asset is not listed in SHA256SUMS" }
  $expected = ([Regex]::Match($entry, '^([0-9a-fA-F]{64})')).Groups[1].Value.ToLowerInvariant()
  $actual = (Get-FileHash -Path $zip -Algorithm SHA256).Hash.ToLowerInvariant()
  if ($actual -ne $expected) {
    throw "checksum mismatch for $asset (expected $expected, got $actual)"
  }
  Write-Host "Verified checksum."

  Expand-Archive -Path $zip -DestinationPath $tmp -Force
  $exe = Join-Path $tmp 'gitnodes.exe'
  if (-not (Test-Path $exe)) { throw "archive did not contain gitnodes.exe" }

  New-Item -ItemType Directory -Path $binDir -Force | Out-Null
  Copy-Item -Path $exe -Destination (Join-Path $binDir 'gitnodes.exe') -Force
  Write-Host "`nInstalled gitnodes to $binDir\gitnodes.exe"
}
finally {
  Remove-Item -Recurse -Force $tmp -ErrorAction SilentlyContinue
}

# --- add to user PATH (persisted, no admin needed) ---------------------------
$userPath = [Environment]::GetEnvironmentVariable('Path', 'User')
if (($userPath -split ';') -notcontains $binDir) {
  [Environment]::SetEnvironmentVariable('Path', "$userPath;$binDir", 'User')
  Write-Host "Added $binDir to your PATH (restart your terminal to pick it up)."
}
$env:Path = "$env:Path;$binDir"

Write-Host "`nRun:  gitnodes init my-brain"
