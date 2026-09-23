# Kill a detached folder run by runner.pid (process tree).
param(
    [string]$OutputDir = ""
)
$ErrorActionPreference = "Stop"
if (-not $OutputDir) {
    $RepoRoot = (Resolve-Path (Join-Path $PSScriptRoot "..\..")).Path
    $OutputDir = Join-Path $RepoRoot "output\subject_extract\nueva_carpeta"
}
$pidFile = Join-Path $OutputDir "runner.pid"
if (-not (Test-Path -LiteralPath $pidFile)) {
    Write-Error "no pid file: $pidFile"
    exit 2
}
$runPid = [int]((Get-Content -LiteralPath $pidFile -Raw).Trim())
Write-Output "killing tree pid=$runPid"
& taskkill.exe /PID $runPid /T /F
$lock = Join-Path $env:LOCALAPPDATA "CarouselCanvas\subject-extract\.gpu.lock"
if (Test-Path -LiteralPath $lock) {
    Remove-Item -LiteralPath $lock -Force
    Write-Output "removed gpu.lock"
}
Remove-Item -LiteralPath $pidFile -Force -ErrorAction SilentlyContinue
exit 0
