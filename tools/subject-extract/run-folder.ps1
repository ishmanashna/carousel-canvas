# Parallel subject-extract folder run.
# Up to -Seats Composer agents work at once (look/decide on CPU).
# rembg + SAM still share one GPU via tools/subject-extract/gpu_lock.py.
param(
    [Parameter(Mandatory = $true)]
    [string]$InputDir,
    [Parameter(Mandatory = $true)]
    [string]$OutputDir,
    [string]$Model = "composer-2.5",
    [int]$TimeoutSeconds = 2400,
    [string]$RepoRoot = "",
    [string[]]$Include = @(),
    [ValidateRange(1, 12)]
    [int]$Seats = 2,
    [string]$DoneCutoutsDir = ""
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
$compare = Join-Path $outRoot "compare_finals"
New-Item -ItemType Directory -Force $compare | Out-Null

if (-not $DoneCutoutsDir) {
    $DoneCutoutsDir = Join-Path $RepoRoot "output\subject_extract\DONE_CUTOUTS"
}
$script:CutoutName = "cutout_full.png"
$script:TotalImages = 0
$script:FinishedCount = 0
$script:SkippedCount = 0

function Test-CutoutDone([string]$stem) {
    foreach ($p in @(
        (Join-Path (Join-Path $outRoot $stem) $script:CutoutName),
        (Join-Path (Join-Path $DoneCutoutsDir $stem) $script:CutoutName)
    )) {
        if (Test-Path -LiteralPath $p) { return $true }
    }
    return $false
}

function Write-SeatProgress([string]$note) {
    $left = [Math]::Max(0, $script:TotalImages - $script:FinishedCount - $script:SkippedCount)
    Write-Output ("progress finished={0} skipped={1} remaining~={2} ({3})" -f $script:FinishedCount, $script:SkippedCount, $left, $note)
}


function Write-RunLog([hashtable]$obj) {
    ($obj | ConvertTo-Json -Compress) | Add-Content -LiteralPath $log -Encoding UTF8
}

$images = @(Get-ChildItem -LiteralPath $inPath.Path -File |
    Where-Object { $_.Extension -match '\.(jpe?g|png|webp)$' } |
    Sort-Object Name)
if ($Include.Count -gt 0) {
    $ids = foreach ($raw in $Include) {
        foreach ($part in ($raw -split ',')) {
            $t = $part.Trim()
            if ($t) { $t }
        }
    }
    $ordered = @()
    foreach ($id in $ids) {
        $hit = @($images | Where-Object { $_.Name -like "*$id*" -or $_.BaseName -like "*$id*" })
        $ordered += $hit
    }
    $images = $ordered
}

Write-Output ("folder run {0} images -> {1} (seats={2})" -f $images.Count, $outRoot, $Seats)
Write-Output ("skip-if-present also checks: {0}" -f $DoneCutoutsDir)
$script:TotalImages = $images.Count
if ($images.Count -eq 0) {
    Write-Error "no images matched (InputDir=$($inPath.Path) Include=$($Include -join ','))"
    exit 2
}

function New-AgentLauncher([System.IO.FileInfo]$img, [string]$dest) {
    $relIn = Rel-Path $RepoRoot $img.FullName
    $relOut = Rel-Path $RepoRoot $dest
    $start = Get-Date
    @"
# Timing

- start: $($start.ToString("yyyy-MM-ddTHH:mm:ssK"))
- end:
- wall_clock_seconds:
"@ | Set-Content -LiteralPath (Join-Path $dest "TIMING.md") -Encoding UTF8

    $prompt = @"
Work in $RepoRoot. Follow docs\subject-extract\03.1.md exactly (including Final checker pass). Input: $relIn Out: $relOut TIMING.md start is already stamped. Set `$env:SX_ACTS_LOG to $relOut\ACTS.jsonl before any sx call. Always write REPORT.md. Always wipe-islands before lift-alpha. Do not mark BLOCKED because rembg was empty. sx rembg already retries. If it still fails, wait 3 minutes and run sx rembg again until you have a seed. Isolated / detached hand fragments are junk - wipe them. Cleanup is light (~10-20%): only remove obvious spare floaters that do not touch the subject. Never wipe into face, hands, attached hair, or held instruments. If unsure, leave it. After the seed: at most 3 wipes and 2 tight sam-box fixes. Prefer WORKS when usable for a carousel; PARTIAL only if the subject is clearly damaged or major held kit is missing. Do not chase perfection. SAM only on missing subject bits with a tight box. Copy numbers into TIMING.md from ACTS.jsonl. PowerShell: use semicolon, not double-ampersand. Quote xyxy box values. Do not use the GUI, strip export, or git commit.
"@
    $promptFile = Join-Path $dest "AGENT_PROMPT.txt"
    $launcher = Join-Path $dest "run-agent.ps1"
    Set-Content -LiteralPath $promptFile -Value $prompt -Encoding UTF8
    $agentLit = $agent.Replace("'", "''")
    $rootLit = $RepoRoot.Replace("'", "''")
    $promptLit = $promptFile.Replace("'", "''")
    @"
Set-Location -LiteralPath '$rootLit'
`$prompt = Get-Content -LiteralPath '$promptLit' -Raw
& '$agentLit' -p --trust --force --workspace '$rootLit' --model $Model -- `$prompt
exit `$LASTEXITCODE
"@ | Set-Content -LiteralPath $launcher -Encoding UTF8
    return $launcher
}

function Start-PhotoJob([System.IO.FileInfo]$img) {
    $stem = $img.BaseName -replace '-Enhanced-NR-Edit$', ''
    $dest = Join-Path $outRoot $stem
    New-Item -ItemType Directory -Force $dest | Out-Null
    $done = Join-Path $dest $script:CutoutName
    if (Test-CutoutDone $stem) {
        Write-Output ("skip {0} (output or DONE_CUTOUTS)" -f $img.Name)
        Write-RunLog @{ ts = (Get-Date -Format "o"); image = $img.Name; status = "skip" }
        $script:SkippedCount++
        Write-SeatProgress "skip"
        return $null
    }
    $launcher = New-AgentLauncher $img $dest
    $stdout = Join-Path $dest "agent.stdout.txt"
    $stderr = Join-Path $dest "agent.stderr.txt"
    $arg = "-NoProfile -File `"$launcher`""
    Write-Output ("start {0}" -f $img.Name)
    $proc = Start-Process -FilePath "powershell.exe" -ArgumentList $arg -WorkingDirectory $RepoRoot -PassThru -WindowStyle Hidden `
        -RedirectStandardOutput $stdout -RedirectStandardError $stderr
    return [pscustomobject]@{
        Img = $img
        Stem = $stem
        Dest = $dest
        Done = $done
        Proc = $proc
        Start = Get-Date
    }
}

function Complete-PhotoJob($job, [string]$forcedStatus) {
    $img = $job.Img
    $proc = $job.Proc
    $proc.Refresh()
    $status = $forcedStatus
    if (-not $status) {
        $status = "ok"
        if ($proc.ExitCode -ne 0) { $status = "agent_exit_$($proc.ExitCode)" }
        if (-not (Test-Path -LiteralPath $job.Done)) { $status = "no_cutout" }
    }
    if (Test-Path -LiteralPath $job.Done) {
        Copy-Item -LiteralPath $job.Done -Destination (Join-Path $compare "$($job.Stem).png") -Force
    }
    $seconds = [int]((Get-Date) - $job.Start).TotalSeconds
    $script:FinishedCount++
    Write-Output ("done {0} {1}" -f $img.Name, $status)
    Write-SeatProgress "seat freed"
    Write-RunLog @{
        ts = (Get-Date -Format "o")
        image = $img.Name
        status = $status
        exit = $proc.ExitCode
        seconds = $seconds
        cutout = (Test-Path -LiteralPath $job.Done)
        seats = $Seats
    }
}

function Invoke-PhotoBatch([System.IO.FileInfo[]]$batch) {
    $active = New-Object System.Collections.Generic.List[object]
    $queue = New-Object System.Collections.Queue
    foreach ($img in $batch) { $queue.Enqueue($img) }

    while ($queue.Count -gt 0 -or $active.Count -gt 0) {
        while ($active.Count -lt $Seats -and $queue.Count -gt 0) {
            $next = $queue.Dequeue()
            $job = Start-PhotoJob $next
            if ($null -ne $job) { [void]$active.Add($job) }
        }

        Start-Sleep -Milliseconds 800
        $still = New-Object System.Collections.Generic.List[object]
        foreach ($job in $active) {
            $proc = $job.Proc
            $proc.Refresh()
            $elapsed = ((Get-Date) - $job.Start).TotalSeconds
            if (-not $proc.HasExited -and $elapsed -gt $TimeoutSeconds) {
                Write-Output ("timeout {0}" -f $job.Img.Name)
                & taskkill.exe /PID $proc.Id /T /F 2>$null | Out-Null
                Set-Content -LiteralPath (Join-Path $job.Dest "REPORT.md") -Encoding UTF8 -Value @"
# Verdict: BLOCKED

Per-image timeout ($TimeoutSeconds s). Folder run continued.
"@
                Complete-PhotoJob $job "timeout"
                continue
            }
            if ($proc.HasExited) {
                Complete-PhotoJob $job $null
                continue
            }
            [void]$still.Add($job)
        }
        $active = $still
    }
}

for ($round = 1; $round -le 2; $round++) {
    Write-Output ("folder round {0}" -f $round)
    $batch = $images
    if ($round -gt 1) {
        $batch = @($images | Where-Object {
            $id = $_.BaseName -replace '-Enhanced-NR-Edit$', ''
            $cut = Join-Path $outRoot $id
            $cut = Join-Path $cut "cutout_full.png"
            -not (Test-Path -LiteralPath $cut)
        })
        if ($batch.Count -eq 0) { break }
        Write-Output ("retry {0} photos with no cutout after 120s GPU rest" -f $batch.Count)
        Start-Sleep -Seconds 120
    }
    Invoke-PhotoBatch $batch
}

$summarize = Join-Path $PSScriptRoot "summarize-run.ps1"
if (Test-Path -LiteralPath $summarize) {
    & $summarize -OutputDir $outRoot
}

Write-Output "folder run complete"



