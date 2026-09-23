# Parallel look-only triage. No GPU.
# Each Composer seat gets up to -BatchSize photos (default 5).
# Concurrent seats = min(-Seats, number of batches), capped at 5.
param(
    [Parameter(Mandatory = $true)]
    [string]$CutoutRoot,
    [Parameter(Mandatory = $true)]
    [string]$OutputDir,
    [Parameter(Mandatory = $true)]
    [string]$OriginalsDir,
    [string]$Model = "composer-2.5",
    [int]$TimeoutSeconds = 2400,
    [string]$RepoRoot = "",
    [ValidateRange(1, 5)]
    [int]$Seats = 5,
    [ValidateRange(1, 5)]
    [int]$BatchSize = 5,
    [ValidateSet("first", "second")]
    [string]$Pass = "first"
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

function Rel-Path([string]$root, [string]$full) {
    $rootFull = ((Resolve-Path -LiteralPath $root).Path.TrimEnd("\") + "\")
    $fullPath = (Resolve-Path -LiteralPath $full).Path
    if ($fullPath.StartsWith($rootFull, [StringComparison]::OrdinalIgnoreCase)) {
        return $fullPath.Substring($rootFull.Length)
    }
    return $fullPath
}


$cutRoot = (Resolve-Path -LiteralPath $CutoutRoot).Path
$origRoot = (Resolve-Path -LiteralPath $OriginalsDir).Path
New-Item -ItemType Directory -Force $OutputDir | Out-Null
$outRoot = (Resolve-Path -LiteralPath $OutputDir).Path
$triageLog = Join-Path $outRoot "TRIAGE.jsonl"
$log = Join-Path $outRoot "RUN.jsonl"
$pilesRoot = Join-Path $outRoot "piles"
$queuesRoot = Join-Path $outRoot "queues"
New-Item -ItemType Directory -Force $queuesRoot | Out-Null
foreach ($p in @("good", "needs_cleanup", "redo", "wont_work", "unlabeled")) {
    New-Item -ItemType Directory -Force (Join-Path $pilesRoot $p) | Out-Null
}

$script:Total = 0
$script:Finished = 0
$script:Skipped = 0

function Write-SeatProgress([string]$note) {
    $left = [Math]::Max(0, $script:Total - $script:Finished - $script:Skipped)
    Write-Output ("progress finished={0} skipped={1} remaining~={2} ({3})" -f $script:Finished, $script:Skipped, $left, $note)
}

function Write-RunLog([hashtable]$obj) {
    ($obj | ConvertTo-Json -Compress) | Add-Content -LiteralPath $log -Encoding UTF8
}

function Test-HasCutout([string]$photoDir) {
    foreach ($name in @("cutout_full.png", "cutout.png", "best.png")) {
        if (Test-Path -LiteralPath (Join-Path $photoDir $name)) { return $true }
    }
    return $false
}

function Copy-PilePreview([string]$photoDir, [string]$stem, [string]$pile) {
    foreach ($other in @("good", "needs_cleanup", "redo", "wont_work", "unlabeled")) {
        if ($other -eq $pile) { continue }
        $old = Join-Path (Join-Path $pilesRoot $other) "$stem.png"
        if (Test-Path -LiteralPath $old) { Remove-Item -LiteralPath $old -Force }
    }
    $destPile = Join-Path $pilesRoot $pile
    New-Item -ItemType Directory -Force $destPile | Out-Null
    foreach ($name in @("cutout_full_checker.png", "cutout_checker.png", "cutout_full.png", "cutout.png")) {
        $src = Join-Path $photoDir $name
        if (Test-Path -LiteralPath $src) {
            Copy-Item -LiteralPath $src -Destination (Join-Path $destPile "$stem.png") -Force
            return
        }
    }
}

function Find-Original([string]$stem) {
    foreach ($ext in @(".jpg", ".jpeg", ".png", ".webp", ".JPG", ".JPEG", ".PNG", ".WEBP")) {
        $hit = Join-Path $origRoot ($stem + $ext)
        if (Test-Path -LiteralPath $hit) { return $hit }
    }
    $loose = @(Get-ChildItem -LiteralPath $origRoot -File -ErrorAction SilentlyContinue |
        Where-Object { $_.BaseName -eq $stem })
    if ($loose.Count -gt 0) { return $loose[0].FullName }
    return ""
}

function Write-Handoff([string]$stem, [string]$photoDir, [string]$original, [string]$pile, [string]$status, [string]$why, [string]$keep, [string]$leftover, [string]$missing, [string]$problem, [string]$next) {
    $json = (@{
        stem = $stem
        pile = $pile
        status = $status
        photo_dir = $photoDir
        original = $original
        why = $why
        problem = $problem
        keep = $keep
        leftover = $leftover
        missing = $missing
        next = $next
    } | ConvertTo-Json -Compress)
    Add-Content -LiteralPath $triageLog -Value $json -Encoding UTF8
    Add-Content -LiteralPath (Join-Path $queuesRoot "$pile.jsonl") -Value $json -Encoding UTF8
}

$photos = @(Get-ChildItem -LiteralPath $cutRoot -Directory |
    Where-Object {
        $_.Name -ne "compare_finals" -and
        $_.Name -ne "piles" -and
        $_.Name -notlike "triage*" -and
        $_.Name -notlike "_batch*"
    } |
    Sort-Object Name)

Write-Output ("triage {0} photo folders -> {1}" -f $photos.Count, $outRoot)
$script:Total = $photos.Count
if ($photos.Count -eq 0) {
    Write-Error "no photo folders in $cutRoot"
    exit 2
}

$needAgent = New-Object System.Collections.Generic.List[object]
$latestPile = @{}
if (Test-Path -LiteralPath $triageLog) {
    foreach ($line in Get-Content -LiteralPath $triageLog) {
        if (-not $line.Trim()) { continue }
        try {
            $row = $line | ConvertFrom-Json
            if ($row.stem) { $latestPile[[string]$row.stem] = ([string]$row.pile).ToLowerInvariant() }
        }
        catch { }
    }
}
foreach ($dir in $photos) {
    $stem = $dir.Name
    $original = Find-Original $stem
    $prev = ""
    if ($latestPile.ContainsKey($stem)) { $prev = $latestPile[$stem] }
    if ($prev -match '^(good|needs_cleanup|redo|wont_work)$') {
        Write-Output ("skip {0} (already {1})" -f $stem, $prev)
        Write-RunLog @{ ts = (Get-Date -Format "o"); image = $stem; status = "skip" }
        $script:Skipped++
        Write-SeatProgress "skip"
        continue
    }
    if (-not (Test-HasCutout $dir.FullName)) {
        Write-Handoff $stem $dir.FullName $original "redo" "no_cutout" "no cutout file" "" "" "entire seed" "no cutout file" "cut from the original; there is no seed"
        Copy-PilePreview $dir.FullName $stem "redo"
        Write-Output ("no_cutout {0} -> redo" -f $stem)
        Write-RunLog @{ ts = (Get-Date -Format "o"); image = $stem; status = "no_cutout"; pile = "redo" }
        $script:Finished++
        Write-SeatProgress "no cutout"
        continue
    }
    [void]$needAgent.Add([pscustomobject]@{
        Stem = $stem
        PhotoDir = $dir.FullName
        Original = $original
    })
}

$batches = New-Object System.Collections.Generic.List[object]
for ($i = 0; $i -lt $needAgent.Count; $i += $BatchSize) {
    $take = [Math]::Min($BatchSize, $needAgent.Count - $i)
    $chunk = New-Object System.Collections.Generic.List[object]
    for ($j = 0; $j -lt $take; $j++) {
        [void]$chunk.Add($needAgent[$i + $j])
    }
    [void]$batches.Add($chunk)
}

$seatCap = [Math]::Min(5, [Math]::Max(1, $Seats))
$seatsUsed = [Math]::Min($seatCap, [Math]::Max(1, $batches.Count))
if ($batches.Count -eq 0) {
    $seatsUsed = 0
}
Write-Output ("batches={0} photos_per_batch<={1} concurrent_seats={2}" -f $batches.Count, $BatchSize, $seatsUsed)

function New-TriageLauncher($items, [string]$batchDir) {
    $items = @($items)
    $lines = New-Object System.Collections.Generic.List[string]
    $n = 1
    foreach ($it in $items) {
        $relPhoto = Rel-Path $RepoRoot $it.PhotoDir
        $origPath = $it.Original
        if (-not $origPath) { $origPath = "(original not found next to the source folder under this stem)" }
        [void]$lines.Add(("$n. stem: {0}`r`n   cutout folder: {1}`r`n   original: {2}" -f $it.Stem, $relPhoto, $origPath))
        $n++
    }
    $list = [string]::Join("`r`n", $lines)
    $count = @($items).Count
    $relBatch = Rel-Path $RepoRoot $batchDir
    $rules = "Follow docs\subject-extract\triage.md exactly. Every line needs why. needs_cleanup needs leftover. redo needs missing and next (what to do differently on a from-scratch second cutout). problem says what is wrong. Decide in order: no usable person at all is wont_work; a damaged keeper (hole in the face, missing head or hair, head off the body, held instrument broken apart) is redo; an intact keeper plus extra pixels to delete is needs_cleanup; otherwise good. A rough edge or a tiny speck is still good. Several complete people are good. Fragments of other people, and incomplete gear you are not keeping, are cleanup, not redo. A clean detail crop is good."
    if ($Pass -eq "second") {
        $rules = "Follow docs\subject-extract\triage-second.md exactly. Pile is only good or needs_cleanup. Every line needs why and problem. needs_cleanup needs leftover. There is no redo pile and no wont_work pile."
    }
    $prompt = @"
Work in $RepoRoot. $rules You have $count photo(s) (max 5). For each photo below, look at the cutout in the cutout folder (cutout_full_checker.png, else cutout_checker.png, else cutout_full.png). Open the original path when you are unsure. Write one JSON object per photo, one per line, to $relBatch\BATCH.jsonl. Do all of them. Do not write a markdown file per photo. Do not run sx rembg, sam, wipe, wipe-islands, erase-alpha, lasso, or lift-alpha. Do not edit cutouts. Ignore REPORT.md. PowerShell: semicolon, not double-ampersand. No GUI, strip, or git commit.

$list
"@
    $promptFile = Join-Path $batchDir "AGENT_PROMPT.txt"
    $launcher = Join-Path $batchDir "run-agent.ps1"
    Set-Content -LiteralPath $promptFile -Value $prompt -Encoding UTF8
    $agentLit = $agent.Replace("'", "''")
    $rootLit = $RepoRoot.Replace("'", "''")
    $promptLit = $promptFile.Replace("'", "''")
    $cutLit = $cutRoot.Replace("'", "''")
    $origLit = $origRoot.Replace("'", "''")
    @"
Set-Location -LiteralPath '$rootLit'
`$prompt = Get-Content -LiteralPath '$promptLit' -Raw
& '$agentLit' -p --trust --force --workspace '$rootLit' --add-dir '$cutLit' --add-dir '$origLit' --model $Model -- `$prompt
exit `$LASTEXITCODE
"@ | Set-Content -LiteralPath $launcher -Encoding UTF8
    return $launcher
}

function Start-TriageBatch($items, [int]$batchIndex) {
    $items = @($items)
    $batchDir = Join-Path $outRoot ("_batch_{0:d3}" -f $batchIndex)
    New-Item -ItemType Directory -Force $batchDir | Out-Null
    $names = ($items | ForEach-Object { $_.Stem }) -join ","
    $launcher = New-TriageLauncher $items $batchDir
    $stdout = Join-Path $batchDir "agent.stdout.txt"
    $stderr = Join-Path $batchDir "agent.stderr.txt"
    $arg = "-NoProfile -File `"$launcher`""
    Write-Output ("start batch {0} n={1} {2}" -f $batchIndex, @($items).Count, $names)
    $proc = Start-Process -FilePath "powershell.exe" -ArgumentList $arg -WorkingDirectory $RepoRoot -PassThru -WindowStyle Hidden `
        -RedirectStandardOutput $stdout -RedirectStandardError $stderr
    return [pscustomobject]@{
        Index = $batchIndex
        Items = $items
        BatchDir = $batchDir
        Proc = $proc
        Start = Get-Date
        Attempt = 1
    }
}

function Test-BatchFileComplete([string]$batchFile, $items) {
    $items = @($items)
    if (-not (Test-Path -LiteralPath $batchFile)) { return $false }
    $have = @{}
    foreach ($line in Get-Content -LiteralPath $batchFile) {
        if (-not $line.Trim()) { continue }
        try {
            $row = $line | ConvertFrom-Json
            $stem = [string]$row.stem
            $pile = ([string]$row.pile).ToLowerInvariant()
            $whyOk = -not [string]::IsNullOrWhiteSpace([string]$row.why)
            $cleanupOk = ($pile -ne "needs_cleanup") -or -not [string]::IsNullOrWhiteSpace([string]$row.leftover)
            $redoOk = ($Pass -eq "second") -or ($pile -ne "redo") -or (
                -not [string]::IsNullOrWhiteSpace([string]$row.missing) -and
                -not [string]::IsNullOrWhiteSpace([string]$row.next)
            )
            $pileOk = if ($Pass -eq "second") { $pile -match '^(good|needs_cleanup)$' } else { $pile -match '^(good|needs_cleanup|redo|wont_work)$' }
            if ($stem -and $whyOk -and $cleanupOk -and $redoOk -and $pileOk) {
                $have[$stem] = $true
            }
        }
        catch { }
    }
    foreach ($it in $items) {
        if (-not $have.ContainsKey([string]$it.Stem)) { return $false }
    }
    return $true
}

function Save-AttemptLogs([string]$batchDir, [int]$attempt) {
    foreach ($name in @("agent.stdout.txt", "agent.stderr.txt", "BATCH.jsonl")) {
        $src = Join-Path $batchDir $name
        if (-not (Test-Path -LiteralPath $src)) { continue }
        $base = [IO.Path]::GetFileNameWithoutExtension($name)
        $ext = [IO.Path]::GetExtension($name)
        Copy-Item -LiteralPath $src -Destination (Join-Path $batchDir "$base.attempt$attempt$ext") -Force
        if ($name -eq "BATCH.jsonl") { Remove-Item -LiteralPath $src -Force }
    }
}

function Complete-TriageBatch($job, [string]$forcedStatus) {
    $proc = $job.Proc
    $proc.Refresh()
    $batchStatus = $forcedStatus
    if (-not $batchStatus) {
        $batchStatus = "ok"
        if ($null -ne $proc.ExitCode -and $proc.ExitCode -ne 0) {
            $batchStatus = "agent_exit_$($proc.ExitCode)"
        }
    }
    foreach ($it in @($job.Items)) {
        $status = $batchStatus
        $pile = "unlabeled"
        $why = ""
        $keep = ""
        $leftover = ""
        $missing = ""
        $problem = ""
        $nextNote = ""
        $batchFile = Join-Path $job.BatchDir "BATCH.jsonl"
        $hit = $null
        if (Test-Path -LiteralPath $batchFile) {
            foreach ($line in Get-Content -LiteralPath $batchFile) {
                if (-not $line.Trim()) { continue }
                try {
                    $row = $line | ConvertFrom-Json
                    if ([string]$row.stem -eq $it.Stem) { $hit = $row }
                }
                catch { }
            }
        }
        if ($forcedStatus -eq "timeout") {
            $why = "triage batch timeout ($TimeoutSeconds s)"
            $status = "timeout"
        }
        elseif ($null -eq $hit) {
            $why = "triage agent did not write a BATCH.jsonl line ($batchStatus)"
            $errPath = Join-Path $job.BatchDir "agent.stderr.txt"
            if (Test-Path -LiteralPath $errPath) {
                $tail = ((Get-Content -LiteralPath $errPath -Tail 4) -join " ").Trim()
                if ($tail.Length -gt 240) { $tail = $tail.Substring($tail.Length - 240) }
                if ($tail) { $why = "$why $tail" }
            }
            $status = "no_triage"
        }
        else {
            $candidate = ([string]$hit.pile).ToLowerInvariant()
            if ($candidate -match '^(good|needs_cleanup|redo|wont_work)$') { $pile = $candidate }
            $why = [string]$hit.why
            $keep = [string]$hit.keep
            $leftover = [string]$hit.leftover
            $missing = [string]$hit.missing
            $problem = [string]$hit.problem
            $nextNote = [string]$hit.next
            if ($Pass -eq "second" -and $pile -match '^(redo|wont_work)$') {
                if ([string]::IsNullOrWhiteSpace($leftover)) {
                    if (-not [string]::IsNullOrWhiteSpace($problem)) { $leftover = $problem }
                    else { $leftover = $why }
                }
                $pile = "needs_cleanup"
                if ($status -eq "ok") { $status = "remapped_to_cleanup" }
            }
        }
        Write-Handoff $it.Stem $it.PhotoDir $it.Original $pile $status $why $keep $leftover $missing $problem $nextNote
        Copy-PilePreview $it.PhotoDir $it.Stem $pile
        $script:Finished++
        Write-Output ("done {0} pile={1} {2}" -f $it.Stem, $pile, $status)
        Write-RunLog @{
            ts = (Get-Date -Format "o")
            image = $it.Stem
            batch = $job.Index
            status = $status
            pile = $pile
            exit = $proc.ExitCode
            seats = $seatsUsed
        }
    }
    Write-SeatProgress "seat freed"
}

if ($batches.Count -gt 0) {
    $active = New-Object System.Collections.Generic.List[object]
    $queue = New-Object System.Collections.Queue
    $bi = 1
    foreach ($chunk in $batches) {
        $queue.Enqueue(@{ Index = $bi; Items = @($chunk) })
        $bi++
    }

    while ($queue.Count -gt 0 -or $active.Count -gt 0) {
        while ($active.Count -lt $seatsUsed -and $queue.Count -gt 0) {
            $next = $queue.Dequeue()
            $job = Start-TriageBatch @($next.Items) $next.Index
            if ($null -ne $job) { [void]$active.Add($job) }
        }
        Start-Sleep -Milliseconds 600
        $still = New-Object System.Collections.Generic.List[object]
        foreach ($job in $active) {
            $proc = $job.Proc
            $proc.Refresh()
            $elapsed = ((Get-Date) - $job.Start).TotalSeconds
            if (-not $proc.HasExited -and $elapsed -gt $TimeoutSeconds) {
                Write-Output ("timeout batch {0}" -f $job.Index)
                & taskkill.exe /PID $proc.Id /T /F 2>$null | Out-Null
                Complete-TriageBatch $job "timeout"
                continue
            }
            if ($proc.HasExited) {
                $batchFile = Join-Path $job.BatchDir "BATCH.jsonl"
                $attempt = 1
                if ($null -ne $job.Attempt) { $attempt = [int]$job.Attempt }
                if (-not (Test-BatchFileComplete $batchFile $job.Items) -and $attempt -lt 3) {
                    Write-Output ("retry batch {0} attempt {1}/3 (agent left no complete BATCH.jsonl)" -f $job.Index, ($attempt + 1))
                    Save-AttemptLogs $job.BatchDir $attempt
                    $again = Start-TriageBatch @($job.Items) $job.Index
                    $again.Attempt = $attempt + 1
                    [void]$still.Add($again)
                    continue
                }
                Complete-TriageBatch $job $null
                continue
            }
            [void]$still.Add($job)
        }
        $active = $still
    }
}

$summary = @("# Triage piles", "")
foreach ($p in @("good", "needs_cleanup", "redo", "wont_work", "unlabeled")) {
    $n = @(Get-ChildItem -LiteralPath (Join-Path $pilesRoot $p) -File -ErrorAction SilentlyContinue).Count
    $summary += "- ${p}: $n"
}
$summary += ""
$summary += "Handoff: TRIAGE.jsonl and queues/<pile>.jsonl"
Set-Content -LiteralPath (Join-Path $outRoot "PILES.md") -Value $summary -Encoding UTF8
Write-Output "triage complete"
Get-Content (Join-Path $outRoot "PILES.md")
