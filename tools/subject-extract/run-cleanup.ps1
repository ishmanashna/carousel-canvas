# First cleanup session. No GPU. Edits the merged cutout folders named in the queue.
# Does not delete stage-1 or stage-3 folders. pre_cleanup.png in each folder is the undo.
param(
    [Parameter(Mandatory = $true)]
    [string]$CutoutRoot,
    [Parameter(Mandatory = $true)]
    [string]$Queue,
    [Parameter(Mandatory = $true)]
    [string]$OutputDir,
    [string]$Model = "composer-2.5",
    [int]$TimeoutSeconds = 2400,
    [string]$RepoRoot = "",
    [ValidateRange(1, 5)]
    [int]$Seats = 5
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
if (-not (Test-Path -LiteralPath $Queue)) {
    Write-Error "queue missing: $Queue"
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

$cutRoot = (Resolve-Path -LiteralPath $CutoutRoot).Path
New-Item -ItemType Directory -Force -Path $OutputDir | Out-Null
$outRoot = (Resolve-Path -LiteralPath $OutputDir).Path
$log = Join-Path $outRoot "RUN.jsonl"

$items = New-Object System.Collections.Generic.List[object]
foreach ($line in Get-Content -LiteralPath $Queue) {
    if (-not $line.Trim()) { continue }
    $row = $line | ConvertFrom-Json
    $stem = [string]$row.stem
    $photo = [string]$row.photo_dir
    if (-not $stem -or -not $photo) { continue }
    $done = Join-Path $photo "cleanup.done"
    if (Test-Path -LiteralPath $done) { continue }
    [void]$items.Add($row)
}

Write-Output ("cleanup {0} photos -> {1} (seats={2})" -f $items.Count, $cutRoot, $Seats)
if ($items.Count -eq 0) {
    Write-Output "cleanup complete"
    exit 0
}

$seatsUsed = [Math]::Min($Seats, $items.Count)

function New-CleanupLauncher($row, [string]$batchDir) {
    $relPhoto = Rel-Path $RepoRoot ([string]$row.photo_dir)
    $orig = [string]$row.original
    if (-not $orig) { $orig = "(original path missing)" }
    $prompt = @"
Work in $RepoRoot. Follow docs\subject-extract\cleanup.md exactly. One photo. Out: $relPhoto Input: $orig Brief: problem=$($row.problem) keep=$($row.keep) leftover=$($row.leftover) Delete only the leftover. Do not invent missing subject. pre_cleanup.png and pre_cleanup_full.png stay untouched. First command is keep-best promote. Preview every lasso before applying it. At most 3 lassos. Fused junk is polygon-only. A tight wipe is only for a floater that does not touch the keeper. No rembg, no SAM, no wipe-islands. Then lift-alpha if cutout.png changed, and write REPORT.md. PowerShell: semicolon, not double-ampersand. Quote xyxy, poly, and origin. No GUI, strip, or git commit.
"@
    $promptFile = Join-Path $batchDir "AGENT_PROMPT.txt"
    $launcher = Join-Path $batchDir "run-agent.ps1"
    Set-Content -LiteralPath $promptFile -Value $prompt -Encoding UTF8
    $agentLit = $agent.Replace("'", "''")
    $rootLit = $RepoRoot.Replace("'", "''")
    $promptLit = $promptFile.Replace("'", "''")
    $cutLit = $cutRoot.Replace("'", "''")
    $origDir = Split-Path -Parent $orig
    if (-not (Test-Path -LiteralPath $origDir)) { $origDir = $cutRoot }
    $origLit = $origDir.Replace("'", "''")
    @"
Set-Location -LiteralPath '$rootLit'
`$prompt = Get-Content -LiteralPath '$promptLit' -Raw
& '$agentLit' -p --trust --force --workspace '$rootLit' --add-dir '$cutLit' --add-dir '$origLit' --model $Model -- `$prompt
exit `$LASTEXITCODE
"@ | Set-Content -LiteralPath $launcher -Encoding UTF8
    return $launcher
}

function Ensure-Snapshot([string]$photo) {
    $cut = Join-Path $photo "cutout.png"
    $snap = Join-Path $photo "pre_cleanup.png"
    if (Test-Path -LiteralPath $snap) { return }
    if (-not (Test-Path -LiteralPath $cut)) { throw "no cutout.png in $photo" }
    Copy-Item -LiteralPath $cut -Destination $snap
    $srcLen = (Get-Item -LiteralPath $cut).Length
    $dstLen = (Get-Item -LiteralPath $snap).Length
    if ($srcLen -ne $dstLen -or $dstLen -lt 1000) { throw "snapshot failed in $photo" }
    if (-not (Test-Path -LiteralPath $cut)) { throw "cutout missing after snapshot $photo" }
}

function Start-CleanupJob($row, [int]$index) {
    Ensure-Snapshot ([string]$row.photo_dir)
    $batchDir = Join-Path $outRoot ("_clean_{0:d3}" -f $index)
    New-Item -ItemType Directory -Force -Path $batchDir | Out-Null
    $launcher = New-CleanupLauncher $row $batchDir
    $stdout = Join-Path $batchDir "agent.stdout.txt"
    $stderr = Join-Path $batchDir "agent.stderr.txt"
    Write-Output ("start {0} {1}" -f $index, $row.stem)
    $arg = "-NoProfile -File `"$launcher`""
    $proc = Start-Process -FilePath "powershell.exe" -ArgumentList $arg -WorkingDirectory $RepoRoot -PassThru -WindowStyle Hidden `
        -RedirectStandardOutput $stdout -RedirectStandardError $stderr
    return [pscustomobject]@{
        Index = $index
        Row = $row
        BatchDir = $batchDir
        Proc = $proc
        Start = Get-Date
    }
}

function Complete-CleanupJob($job) {
    $photo = [string]$job.Row.photo_dir
    $cutout = Join-Path $photo "cutout.png"
    $snap = Join-Path $photo "pre_cleanup.png"
    $report = Join-Path $photo "REPORT.md"
    $ok = (Test-Path -LiteralPath $cutout) -and (Test-Path -LiteralPath $snap) -and (Test-Path -LiteralPath $report)
    $status = "ok"
    if (-not $ok) { $status = "incomplete" }
    if ($ok) {
        Set-Content -LiteralPath (Join-Path $photo "cleanup.done") -Value $status -Encoding UTF8
    }
    (@{
        ts = (Get-Date -Format "o")
        image = [string]$job.Row.stem
        status = $status
        exit = $job.Proc.ExitCode
    } | ConvertTo-Json -Compress) | Add-Content -LiteralPath $log -Encoding UTF8
    Write-Output ("done {0} {1}" -f $job.Row.stem, $status)
}

$queue = New-Object System.Collections.Queue
$n = 1
foreach ($row in $items) {
    $queue.Enqueue(@{ Index = $n; Row = $row })
    $n++
}
$active = New-Object System.Collections.Generic.List[object]
while ($queue.Count -gt 0 -or $active.Count -gt 0) {
    while ($active.Count -lt $seatsUsed -and $queue.Count -gt 0) {
        $next = $queue.Dequeue()
        [void]$active.Add((Start-CleanupJob $next.Row $next.Index))
    }
    Start-Sleep -Milliseconds 600
    $still = New-Object System.Collections.Generic.List[object]
    foreach ($job in $active) {
        $proc = $job.Proc
        $proc.Refresh()
        $elapsed = ((Get-Date) - $job.Start).TotalSeconds
        if (-not $proc.HasExited -and $elapsed -gt $TimeoutSeconds) {
            Write-Output ("timeout {0}" -f $job.Row.stem)
            & taskkill.exe /PID $proc.Id /T /F 2>$null | Out-Null
            Complete-CleanupJob $job
            continue
        }
        if ($proc.HasExited) {
            Complete-CleanupJob $job
            continue
        }
        [void]$still.Add($job)
    }
    $active = $still
}
Write-Output "cleanup complete"
