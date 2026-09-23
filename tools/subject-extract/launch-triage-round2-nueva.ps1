# Stage 4: look-only triage of the second-cutout folder. Detached. Does not touch stage 2 piles.
param(
    [ValidateRange(1, 5)]
    [int]$Seats = 5,
    [string]$Model = "composer-2.5",
    [int]$TimeoutSeconds = 2400
)
$ErrorActionPreference = "Stop"
$RepoRoot = (Resolve-Path (Join-Path $PSScriptRoot "..\..")).Path
$CutoutRoot = Join-Path $RepoRoot "output\subject_extract\nueva_carpeta_round2"
$OriginalsDir = "C:\Users\jordi\Desktop\Nueva carpeta"
$OutputDir = Join-Path $RepoRoot "output\subject_extract\nueva_carpeta_round2_triage"
if (-not (Test-Path -LiteralPath $CutoutRoot)) {
    Write-Error "second cutout root missing: $CutoutRoot"
    exit 2
}
if (-not (Test-Path -LiteralPath $OriginalsDir)) {
    Write-Error "originals missing: $OriginalsDir"
    exit 2
}
$runLog = Join-Path $CutoutRoot "runner.log"
$done = $false
if (Test-Path -LiteralPath $runLog) {
    $done = Select-String -LiteralPath $runLog -Pattern "folder run complete" -Quiet
}
if (-not $done) {
    Write-Error "second cutout has not logged 'folder run complete'. Let it finish, then launch triage."
    exit 2
}
New-Item -ItemType Directory -Force -Path (Join-Path $OutputDir "piles\good") | Out-Null
Write-Output "Cutouts:   $CutoutRoot"
Write-Output "Originals: $OriginalsDir"
Write-Output "Piles:     $OutputDir"
Write-Output "Good:      $(Join-Path $OutputDir 'piles\good')"
Write-Output "Seats=$Seats Model=$Model (detached triage of second cutout, no GPU)"
& (Join-Path $PSScriptRoot "start-detached.ps1") `
    -Job triage `
    -InputDir $CutoutRoot `
    -OriginalsDir $OriginalsDir `
    -OutputDir $OutputDir `
    -Seats $Seats `
    -Model $Model `
    -TimeoutSeconds $TimeoutSeconds `
    -RepoRoot $RepoRoot `
    -TriagePass second
exit $LASTEXITCODE
