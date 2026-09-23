# 117-photo Nueva carpeta run, detached from Cursor.
# Running this script starts the job. Do not invoke until asked.
param(
    [ValidateRange(1, 12)]
    [int]$Seats = 8,
    [string]$Model = "composer-2.5"
)
$ErrorActionPreference = "Stop"
$RepoRoot = (Resolve-Path (Join-Path $PSScriptRoot "..\..")).Path
$InputDir = "C:\Users\jordi\Desktop\Nueva carpeta"
$OutputDir = Join-Path $RepoRoot "output\subject_extract\nueva_carpeta"
if (-not (Test-Path -LiteralPath $InputDir)) {
    Write-Error "input folder missing: $InputDir"
    exit 2
}
Write-Output "Input:  $InputDir"
Write-Output "Output: $OutputDir"
Write-Output "Seats=$Seats Model=$Model (detached)"
& (Join-Path $PSScriptRoot "start-detached.ps1") `
    -InputDir $InputDir `
    -OutputDir $OutputDir `
    -Seats $Seats `
    -Model $Model `
    -RepoRoot $RepoRoot
exit $LASTEXITCODE
