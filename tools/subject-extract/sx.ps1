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

& $Python $Cli @args
exit $LASTEXITCODE
