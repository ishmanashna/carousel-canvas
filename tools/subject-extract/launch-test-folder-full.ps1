# Full TEST IMAGES subject-extract, skipping DONE_CUTOUTS keepers.
# Usage: powershell -NoProfile -File tools\subject-extract\launch-test-folder-full.ps1 [-Seats 8]
param(
    [ValidateRange(1,12)]
    [int]$Seats = 8,
    [string]$Model = "composer-2.5"
)
$ErrorActionPreference = "Stop"
$RepoRoot = (Resolve-Path (Join-Path $PSScriptRoot "..\..")).Path
$InputDir = Join-Path $RepoRoot "TEST IMAGES"
$OutputDir = Join-Path $RepoRoot "output\subject_extract\test_folder_full"
$DoneDir = Join-Path $RepoRoot "output\subject_extract\DONE_CUTOUTS"
New-Item -ItemType Directory -Force -Path $OutputDir | Out-Null
Write-Output "Input:  $InputDir"
Write-Output "Output: $OutputDir"
Write-Output "DONE:   $DoneDir"
Write-Output "Seats=$Seats Model=$Model"
& (Join-Path $PSScriptRoot "run-folder.ps1") `
    -InputDir $InputDir `
    -OutputDir $OutputDir `
    -DoneCutoutsDir $DoneDir `
    -Seats $Seats `
    -Model $Model
