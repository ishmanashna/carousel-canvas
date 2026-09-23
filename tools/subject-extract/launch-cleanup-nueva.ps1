# Stage 6: first cleanup. Edits the folders named in the queue. Does not copy photo directories.
# Do not invoke until asked.
param(
    [ValidateRange(1, 5)]
    [int]$Seats = 5,
    [string]$Model = "composer-2.5",
    [int]$TimeoutSeconds = 2400
)
$ErrorActionPreference = "Stop"
$RepoRoot = (Resolve-Path (Join-Path $PSScriptRoot "..\..")).Path
$CutoutRoot = Join-Path $RepoRoot "output\subject_extract"
$Queue = Join-Path $CutoutRoot "nueva_carpeta_triage\CLEANUP_QUEUE.jsonl"
$OutputDir = Join-Path $RepoRoot "output\subject_extract\nueva_carpeta_cleanup_run"
if (-not (Test-Path -LiteralPath $Queue)) {
    Write-Error "cleanup queue missing: $Queue"
    exit 2
}
Write-Output "Cutouts: $CutoutRoot"
Write-Output "Queue:   $Queue"
Write-Output "Logs:    $OutputDir"
Write-Output "Seats=$Seats Model=$Model (detached cleanup, no GPU)"
& (Join-Path $PSScriptRoot "start-detached.ps1") `
    -Job cleanup `
    -InputDir $CutoutRoot `
    -Queue $Queue `
    -OutputDir $OutputDir `
    -Seats $Seats `
    -Model $Model `
    -TimeoutSeconds $TimeoutSeconds `
    -RepoRoot $RepoRoot
exit $LASTEXITCODE
