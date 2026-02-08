Param(
    [string]$OutDir = "dist"
)

$ErrorActionPreference = "Stop"

Write-Host "Building release binary..."
cargo build --release

New-Item -ItemType Directory -Force -Path $OutDir | Out-Null

$version = (Select-String -Path "Cargo.toml" -Pattern 'version\s*=\s*"([^"]+)"').Matches.Groups[1].Value
$zipPath = Join-Path $OutDir "reorder-vfat-$version-windows.zip"

$files = @(
    "target\release\reorder-vfat.exe",
    "README.md",
    "LICENSE"
)

Write-Host "Creating package: $zipPath"
Compress-Archive -Path $files -DestinationPath $zipPath -Force

Write-Host "Done."
