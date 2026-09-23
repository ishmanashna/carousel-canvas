# Write CLEANUP_QUEUE.jsonl. Paths only. Does not copy images.
$ErrorActionPreference = "Stop"
$root = (Resolve-Path (Join-Path $PSScriptRoot "..\..")).Path
$stage1 = Join-Path $root "output\subject_extract\nueva_carpeta"
$stage3 = Join-Path $root "output\subject_extract\nueva_carpeta_round2"
$triage1 = Join-Path $root "output\subject_extract\nueva_carpeta_triage"
$triage2 = Join-Path $root "output\subject_extract\nueva_carpeta_round2_triage"
$pile = Join-Path $triage1 "piles\needs_cleanup"
$out = Join-Path $triage1 "CLEANUP_QUEUE.jsonl"

function Latest-Rows([string]$path) {
    $map = @{}
    if (-not (Test-Path -LiteralPath $path)) { return $map }
    foreach ($line in Get-Content -LiteralPath $path) {
        if (-not $line.Trim()) { continue }
        try {
            $row = $line | ConvertFrom-Json
            if ($row.stem) { $map[[string]$row.stem] = $row }
        }
        catch { }
    }
    return $map
}

$rows1 = Latest-Rows (Join-Path $triage1 "TRIAGE.jsonl")
$rows2 = Latest-Rows (Join-Path $triage2 "TRIAGE.jsonl")
$files = @(Get-ChildItem -LiteralPath $pile -Filter *.png)
if ($files.Count -eq 0) { throw "no previews in $pile" }

$lines = New-Object System.Collections.Generic.List[string]
$from3 = 0
$from1 = 0
foreach ($f in $files) {
    $stem = [IO.Path]::GetFileNameWithoutExtension($f.Name)
    $dir3 = Join-Path $stage3 $stem
    $dir1 = Join-Path $stage1 $stem
    $from = "stage1"
    $photo = $dir1
    $row = $null
    if ($rows1.ContainsKey($stem)) { $row = $rows1[$stem] }
    if (Test-Path -LiteralPath (Join-Path $dir3 "cutout.png")) {
        $from = "stage3"
        $photo = $dir3
        if ($rows2.ContainsKey($stem)) { $row = $rows2[$stem] }
        $from3++
    }
    else {
        if (-not (Test-Path -LiteralPath (Join-Path $dir1 "cutout.png"))) {
            throw "no cutout.png for $stem"
        }
        $from1++
    }
    $orig = ""
    $problem = ""
    $keep = ""
    $leftover = ""
    $why = ""
    if ($null -ne $row) {
        $orig = [string]$row.original
        $problem = [string]$row.problem
        $keep = [string]$row.keep
        $leftover = [string]$row.leftover
        $why = [string]$row.why
    }
    if (-not $orig -or -not (Test-Path -LiteralPath $orig)) {
        $guess = Join-Path "C:\Users\jordi\Desktop\Nueva carpeta" ($stem + ".jpg")
        if (Test-Path -LiteralPath $guess) { $orig = $guess }
    }
    if (-not $leftover.Trim()) {
        if ($problem.Trim()) { $leftover = $problem } elseif ($why.Trim()) { $leftover = $why }
    }
    $lines.Add((@{
        stem = $stem
        photo_dir = $photo
        original = $orig
        from = $from
        problem = $problem
        keep = $keep
        leftover = $leftover
        why = $why
    } | ConvertTo-Json -Compress))
}
Set-Content -LiteralPath $out -Value ($lines -join "`r`n") -Encoding UTF8
"queue $($lines.Count) stage3=$from3 stage1=$from1"
"wrote $out"
