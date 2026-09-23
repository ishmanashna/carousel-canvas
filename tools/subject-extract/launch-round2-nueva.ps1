# Second cutout for photos whose latest triage pile is redo.
# Detached. Reads TRIAGE.jsonl (last line per stem) and passes that note into each seat.
param(
    [ValidateRange(1, 12)]
    [int]$Seats = 8,
    [string]$Model = "composer-2.5",
    [int]$TimeoutSeconds = 2400
)
$ErrorActionPreference = "Stop"
$RepoRoot = (Resolve-Path (Join-Path $PSScriptRoot "..\..")).Path
$InputDir = "C:\Users\jordi\Desktop\Nueva carpeta"
$TriageLog = Join-Path $RepoRoot "output\subject_extract\nueva_carpeta_triage\TRIAGE.jsonl"
$OutputDir = Join-Path $RepoRoot "output\subject_extract\nueva_carpeta_round2"
if (-not (Test-Path -LiteralPath $InputDir)) {
    Write-Error "input folder missing: $InputDir"
    exit 2
}
if (-not (Test-Path -LiteralPath $TriageLog)) {
    Write-Error "triage log missing: $TriageLog"
    exit 2
}
Write-Output "Input:  $InputDir"
Write-Output "Triage: $TriageLog"
Write-Output "Output: $OutputDir"
Write-Output "Seats=$Seats pile=redo (detached second cutout)"
& (Join-Path $PSScriptRoot "start-detached.ps1") `
    -InputDir $InputDir `
    -OutputDir $OutputDir `
    -TriageLog $TriageLog `
    -OnlyPile redo `
    -IgnoreDoneCutouts `
    -Seats $Seats `
    -Model $Model `
    -TimeoutSeconds $TimeoutSeconds `
    -RepoRoot $RepoRoot
exit $LASTEXITCODE
