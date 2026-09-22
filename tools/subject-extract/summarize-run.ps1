# Roll ACTS.jsonl + REPORT.md + FOLDER.jsonl into METRICS.md
param(
    [Parameter(Mandatory = $true)]
    [string]$OutputDir
)

$ErrorActionPreference = "Stop"
$root = (Resolve-Path -LiteralPath $OutputDir).Path
$lines = @("# Metrics", "", "| photo | verdict | wall_s | rembg_s | sam_s | wipe_s | gpu_wait_s | look_s | sam_n | wipe_n | rembg_tries |", "| --- | --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |")

function Sum-Cmd($rows, $name) {
    ($rows | Where-Object { $_.command -eq $name } | Measure-Object -Property elapsed_seconds -Sum).Sum
}

Get-ChildItem -LiteralPath $root -Directory | Sort-Object Name | ForEach-Object {
    $actsPath = Join-Path $_.FullName "ACTS.jsonl"
    $reportPath = Join-Path $_.FullName "REPORT.md"
    $timingPath = Join-Path $_.FullName "TIMING.md"
    if (-not (Test-Path -LiteralPath $actsPath)) { return }

    $rows = @()
    Get-Content -LiteralPath $actsPath | ForEach-Object {
        if ($_.Trim()) { $rows += ($_ | ConvertFrom-Json) }
    }
    $rembg = [math]::Round((Sum-Cmd $rows "rembg"), 1)
    $sam = [math]::Round((Sum-Cmd $rows "sam-box"), 1)
    $wipeRows = $rows | Where-Object { $_.command -eq "wipe" -or $_.command -eq "erase-alpha" -or $_.command -eq "wipe-islands" }
    $wipe = [math]::Round((($wipeRows | Measure-Object -Property elapsed_seconds -Sum).Sum), 1)
    $cli = [math]::Round((($rows | Measure-Object -Property elapsed_seconds -Sum).Sum), 1)
    $gpu = [math]::Round((($rows | Where-Object { $_.gpu_lock_wait_seconds } | Measure-Object -Property gpu_lock_wait_seconds -Sum).Sum), 1)
    $samN = @($rows | Where-Object { $_.command -eq "sam-box" }).Count
    $wipeN = @($wipeRows).Count
    $rembgN = @($rows | Where-Object { $_.command -eq "rembg" }).Count
    $wall = $null
    if (Test-Path -LiteralPath $timingPath) {
        $t = Get-Content -LiteralPath $timingPath -Raw
        if ($t -match "wall_clock_seconds:\s*([0-9.]+)") { $wall = [double]$Matches[1] }
    }
    $look = if ($null -ne $wall) { [math]::Round($wall - $cli, 1) } else { "" }
    $verdict = "unknown"
    if (Test-Path -LiteralPath $reportPath) {
        $r = Get-Content -LiteralPath $reportPath -Raw
        if ($r -match "BLOCKED") { $verdict = "BLOCKED" }
        elseif ($r -match "PARTIAL") { $verdict = "PARTIAL" }
        elseif ($r -match "WORKS") { $verdict = "WORKS" }
        elseif ($r -match "FAIL") { $verdict = "FAIL" }
    }
    $wallCell = if ($null -ne $wall) { $wall } else { "" }
    $lines += "| $($_.Name) | $verdict | $wallCell | $rembg | $sam | $wipe | $gpu | $look | $samN | $wipeN | $rembgN |"
}

$lines += ""
$lines += "look_s = wall minus sum of ACTS elapsed_seconds (grid/look/agent)."
$lines += "Finals: compare_finals/"
$lines += "Per-command log: each photo ACTS.jsonl (sx-time, ort_providers, gpu_lock_wait_seconds)."
$dest = Join-Path $root "METRICS.md"
$lines -join "`n" | Set-Content -LiteralPath $dest -Encoding UTF8
Write-Output "wrote $dest"
