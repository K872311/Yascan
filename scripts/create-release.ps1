#!/usr/bin/env pwsh
# Yascan Release Builder Script
# Usage: .\scripts\create-release.ps1 [version]

param(
    [string]$Version = "2.10.0"
)

$ErrorActionPreference = "Stop"

Write-Host "=== Yascan Release Builder ===" -ForegroundColor Cyan

# Clean previous builds
Write-Host "Cleaning previous builds..." -ForegroundColor Yellow
if (Test-Path "target\release") {
    Remove-Item -Recurse -Force "target\release"
}

# Set optimization flags
$env:RUSTFLAGS = "-C target-feature=+crt-static"

# Build release
Write-Host "Building release binary..." -ForegroundColor Yellow
cargo build --release

if ($LASTEXITCODE -ne 0) {
    Write-Error "Build failed!"
    exit 1
}

# Create release directory
$ReleaseDir = "yascan-v$Version-windows-x64"
if (Test-Path $ReleaseDir) {
    Remove-Item -Recurse -Force $ReleaseDir
}
New-Item -ItemType Directory -Force -Path $ReleaseDir | Out-Null

# Copy binaries
Write-Host "Copying binaries..." -ForegroundColor Yellow
Copy-Item "target\release\yascan.exe" "$ReleaseDir\"
Copy-Item "target\release\yascan-util.exe" "$ReleaseDir\"

# Copy signatures
Write-Host "Copying signatures..." -ForegroundColor Yellow
Copy-Item -Recurse "signatures" "$ReleaseDir\"

# Copy docs
if (Test-Path "README.md") {
    Copy-Item "README.md" "$ReleaseDir\"
}

# Create config directory
New-Item -ItemType Directory -Force -Path "$ReleaseDir\config" | Out-Null

# Create ZIP
Write-Host "Creating ZIP archive..." -ForegroundColor Yellow
$ZipName = "$ReleaseDir.zip"
if (Test-Path $ZipName) {
    Remove-Item -Force $ZipName
}
Compress-Archive -Path $ReleaseDir -DestinationPath $ZipName -CompressionLevel Optimal

# Show results
$ExeSize = (Get-Item "$ReleaseDir\yascan.exe").Length / 1MB
$ZipSize = (Get-Item $ZipName).Length / 1MB

Write-Host "`n=== Build Complete ===" -ForegroundColor Green
Write-Host "Binary size:  {0:N2} MB" -f $ExeSize
Write-Host "Archive size: {0:N2} MB" -f $ZipSize
Write-Host "Output: $ZipName"

# Calculate hashes (if certutil available)
if (Get-Command certutil -ErrorAction SilentlyContinue) {
    Write-Host "`n=== SHA256 Hashes ===" -ForegroundColor Cyan
    certutil -hashfile "$ReleaseDir\yascan.exe" SHA256 | Select-Object -First 1
}

Write-Host "`nDone! Distribution ready in: $ReleaseDir"
