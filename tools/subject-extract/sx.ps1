# subject-extract wrapper: run cache venv python on cli.py
$ErrorActionPreference = "Stop"

$ScriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$Cli = Join-Path $ScriptDir "cli.py"

$LocalAppData = [Environment]::GetFolderPath("LocalApplicationData")
$Python = Join-Path $LocalAppData "CarouselCanvas\subject-extract\venv\Scripts\python.exe"

if (-not (Test-Path -LiteralPath $Python)) {
    Write-Error "venv python not found: $Python`nRun: python tools/subject-extract/prefetch.py"
    exit 2
}

# rembg empty-mask is a dirty DirectML session. New process + cooldown; do not retry in-process.
$RembgTries = 8
$RembgCooldownSeconds = 60
if ($args.Count -gt 0 -and $args[0] -eq "rembg") {
    for ($i = 1; $i -le $RembgTries; $i++) {
        & $Python $Cli @args
        if ($LASTEXITCODE -eq 0) { exit 0 }
        if ($i -lt $RembgTries) {
            Write-Output ("rembg try {0}/{1} failed; cooldown {2}s" -f $i, $RembgTries, $RembgCooldownSeconds)
            Start-Sleep -Seconds $RembgCooldownSeconds
        }
    }
    exit $LASTEXITCODE
}

& $Python $Cli @args
exit $LASTEXITCODE
