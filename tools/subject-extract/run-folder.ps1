# One photo at a time. A failed/timed-out photo does not stop the folder.
param(
    [Parameter(Mandatory = $true)]
    [string]$InputDir,
    [Parameter(Mandatory = $true)]
    [string]$OutputDir,
    [string]$Model = "composer-2.5",
    [int]$TimeoutSeconds = 1500,
    [string]$RepoRoot = ""
)

$ErrorActionPreference = "Stop"
if (-not $RepoRoot) {
    $RepoRoot = (Resolve-Path (Join-Path $PSScriptRoot "..\..")).Path
}
Set-Location -LiteralPath $RepoRoot

$agent = Join-Path $env:LOCALAPPDATA "cursor-agent\agent.ps1"
if (-not (Test-Path -LiteralPath $agent)) {
    Write-Error "Cursor agent CLI not found: $agent"
    exit 2
}

$sx = Join-Path $RepoRoot "tools\subject-extract\sx.ps1"
& $sx doctor
if ($LASTEXITCODE -ne 0) {
    Write-Error "doctor failed; not starting the folder run"
    exit 2
}

function Rel-Path([string]$root, [string]$full) {
    $rootFull = ((Resolve-Path -LiteralPath $root).Path.TrimEnd("\") + "\")
    $fullPath = (Resolve-Path -LiteralPath $full).Path
    if ($fullPath.StartsWith($rootFull, [StringComparison]::OrdinalIgnoreCase)) {
        return $fullPath.Substring($rootFull.Length)
    }
    return $fullPath
}

$inPath = Resolve-Path -LiteralPath $InputDir
New-Item -ItemType Directory -Force $OutputDir | Out-Null
$outRoot = (Resolve-Path -LiteralPath $OutputDir).Path
$log = Join-Path $outRoot "FOLDER.jsonl"

function Write-RunLog([hashtable]$obj) {
    ($obj | ConvertTo-Json -Compress) | Add-Content -LiteralPath $log -Encoding UTF8
}

$images = @(Get-ChildItem -LiteralPath $inPath.Path -File |
    Where-Object { $_.Extension -match '\.(jpe?g|png|webp)$' } |
    Sort-Object Name)

Write-Output ("folder run {0} images -> {1}" -f $images.Count, $outRoot)

foreach ($img in $images) {
    $stem = $img.BaseName -replace '-Enhanced-NR-Edit$', ''
    $dest = Join-Path $outRoot $stem
    New-Item -ItemType Directory -Force $dest | Out-Null
    $done = Join-Path $dest "cutout_full.png"
    if (Test-Path -LiteralPath $done) {
        Write-Output ("skip {0}" -f $img.Name)
        Write-RunLog @{ ts = (Get-Date -Format "o"); image = $img.Name; status = "skip" }
        continue
    }

    $relIn = Rel-Path $RepoRoot $img.FullName
    $relOut = Rel-Path $RepoRoot $dest
    $start = Get-Date
    @"
# Timing

- start: $($start.ToString("yyyy-MM-ddTHH:mm:ssK"))
- end:
- wall_clock_seconds:
"@ | Set-Content -LiteralPath (Join-Path $dest "TIMING.md") -Encoding UTF8

    $prompt = "Work in $RepoRoot. Follow docs\subject-extract\03.1.md exactly. Input: $relIn Out: $relOut TIMING.md start is already stamped. Always write REPORT.md even if BLOCKED. PowerShell: use ; not &&. Quote --xyxy. Do not use the GUI, strip export, or git commit."

    Write-Output ("start {0}" -f $img.Name)
    $stdout = Join-Path $dest "agent.stdout.txt"
    $stderr = Join-Path $dest "agent.stderr.txt"
    $proc = Start-Process -FilePath "powershell.exe" -ArgumentList @(
        "-NoProfile",
        "-File", $agent,
        "-p",
        "--trust",
        "--force",
        "--workspace", $RepoRoot,
        "--model", $Model,
        $prompt
    ) -WorkingDirectory $RepoRoot -PassThru -WindowStyle Hidden `
        -RedirectStandardOutput $stdout -RedirectStandardError $stderr

    $finished = $proc.WaitForExit($TimeoutSeconds * 1000)
    if (-not $finished) {
        Write-Output ("timeout {0}" -f $img.Name)
        & taskkill.exe /PID $proc.Id /T /F | Out-Null
        Set-Content -LiteralPath (Join-Path $dest "REPORT.md") -Encoding UTF8 -Value @"
# Verdict: BLOCKED

Per-image timeout ($TimeoutSeconds s). Folder run continued.
"@
        Write-RunLog @{ ts = (Get-Date -Format "o"); image = $img.Name; status = "timeout"; seconds = $TimeoutSeconds }
        continue
    }

    $status = "ok"
    if ($proc.ExitCode -ne 0) { $status = "agent_exit_$($proc.ExitCode)" }
    if (-not (Test-Path -LiteralPath $done)) { $status = "no_cutout" }
    Write-Output ("done {0} {1}" -f $img.Name, $status)
    Write-RunLog @{
        ts = (Get-Date -Format "o")
        image = $img.Name
        status = $status
        exit = $proc.ExitCode
        seconds = [int]((Get-Date) - $start).TotalSeconds
    }
}

Write-Output "folder run complete"
