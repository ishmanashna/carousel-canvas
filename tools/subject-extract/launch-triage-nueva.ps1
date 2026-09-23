# Look-only triage of Nueva carpeta cutouts. Detached. Do not invoke until asked.
param(
    [ValidateRange(1, 5)]
    [int]$Seats = 5,
    [string]$Model = "composer-2.5",
    [int]$TimeoutSeconds = 2400
)
$ErrorActionPreference = "Stop"
$RepoRoot = (Resolve-Path (Join-Path $PSScriptRoot "..\..")).Path
$CutoutRoot = Join-Path $RepoRoot "output\subject_extract\nueva_carpeta"
$OriginalsDir = "C:\Users\jordi\Desktop\Nueva carpeta"
$OutputDir = Join-Path $RepoRoot "output\subject_extract\nueva_carpeta_triage"
if (-not (Test-Path -LiteralPath $CutoutRoot)) {
    Write-Error "cutout root missing: $CutoutRoot"
    exit 2
}
if (-not (Test-Path -LiteralPath $OriginalsDir)) {
    Write-Error "originals missing: $OriginalsDir"
    exit 2
}
$cutPidFile = Join-Path $CutoutRoot "runner.pid"
if (Test-Path -LiteralPath $cutPidFile) {
    $cutPid = 0
    [void][int]::TryParse((Get-Content -LiteralPath $cutPidFile -Raw).Trim(), [ref]$cutPid)
    if ($cutPid -gt 0 -and (Get-Process -Id $cutPid -ErrorAction SilentlyContinue)) {
        Write-Error "cutout run still going (pid $cutPid). Let it finish, then launch triage."
        exit 2
    }
}
Write-Output "Cutouts:   $CutoutRoot"
Write-Output "Originals: $OriginalsDir"
Write-Output "Piles:     $OutputDir"
Write-Output "Seats=$Seats Model=$Model (detached triage, no GPU)"
& (Join-Path $PSScriptRoot "start-detached.ps1") `
    -Job triage `
    -InputDir $CutoutRoot `
    -OriginalsDir $OriginalsDir `
    -OutputDir $OutputDir `
    -Seats $Seats `
    -Model $Model `
    -TimeoutSeconds $TimeoutSeconds `
    -RepoRoot $RepoRoot
exit $LASTEXITCODE
