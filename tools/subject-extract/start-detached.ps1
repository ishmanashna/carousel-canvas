# Spawn run-folder.ps1 in a new console that is not in Cursor's job.
# Closing Cursor must not kill the run. Does not wait.
param(
    [Parameter(Mandatory = $true)]
    [string]$InputDir,
    [Parameter(Mandatory = $true)]
    [string]$OutputDir,
    [string]$Model = "composer-2.5",
    [int]$TimeoutSeconds = 2400,
    [string]$RepoRoot = "",
    [string]$Include = "",
    [ValidateRange(1, 12)]
    [int]$Seats = 2,
    [string]$DoneCutoutsDir = "",
    [string]$TriageLog = "",
    [string]$OnlyPile = "",
    [switch]$IgnoreDoneCutouts,
    [ValidateSet("folder", "triage", "cleanup")]
    [string]$Job = "folder",
    [string]$OriginalsDir = "",
    [ValidateSet("first", "second")]
    [string]$TriagePass = "first",
    [string]$Queue = ""
)

$ErrorActionPreference = "Stop"
if (-not $RepoRoot) {
    $RepoRoot = (Resolve-Path (Join-Path $PSScriptRoot "..\..")).Path
}
$runFolder = Join-Path $PSScriptRoot "run-folder.ps1"
$runTriage = Join-Path $PSScriptRoot "run-triage.ps1"
$runCleanup = Join-Path $PSScriptRoot "run-cleanup.ps1"
if ($Job -eq "folder" -and -not (Test-Path -LiteralPath $runFolder)) {
    Write-Error "missing $runFolder"
    exit 2
}
if ($Job -eq "triage" -and -not (Test-Path -LiteralPath $runTriage)) {
    Write-Error "missing $runTriage"
    exit 2
}
if ($Job -eq "triage" -and -not $OriginalsDir) {
    Write-Error "triage job needs -OriginalsDir"
    exit 2
}
if ($Job -eq "cleanup" -and -not (Test-Path -LiteralPath $runCleanup)) {
    Write-Error "missing $runCleanup"
    exit 2
}
if ($Job -eq "cleanup" -and -not $Queue) {
    Write-Error "cleanup job needs -Queue"
    exit 2
}

New-Item -ItemType Directory -Force -Path $OutputDir | Out-Null
$outRoot = (Resolve-Path -LiteralPath $OutputDir).Path
$hostPs = Join-Path $outRoot "run-detached-host.ps1"
$pidFile = Join-Path $outRoot "runner.pid"
$logFile = Join-Path $outRoot "runner.log"

function Quote-Ps([string]$s) {
    return "'" + ($s.Replace("'", "''")) + "'"
}

$title = "subject-extract folder run"
$invoke = $null
if ($Job -eq "triage") {
    $title = "subject-extract triage"
    $triageSeats = [Math]::Min(5, [Math]::Max(1, $Seats))
    $invoke = "& $(Quote-Ps $runTriage) -CutoutRoot $(Quote-Ps $InputDir) -OutputDir $(Quote-Ps $outRoot) -OriginalsDir $(Quote-Ps $OriginalsDir) -Model $(Quote-Ps $Model) -TimeoutSeconds $TimeoutSeconds -RepoRoot $(Quote-Ps $RepoRoot) -Seats $triageSeats -Pass $(Quote-Ps $TriagePass)"
}
elseif ($Job -eq "cleanup") {
    $title = "subject-extract cleanup"
    $cleanupSeats = [Math]::Min(5, [Math]::Max(1, $Seats))
    $invoke = "& $(Quote-Ps $runCleanup) -CutoutRoot $(Quote-Ps $InputDir) -Queue $(Quote-Ps $Queue) -OutputDir $(Quote-Ps $outRoot) -Model $(Quote-Ps $Model) -TimeoutSeconds $TimeoutSeconds -RepoRoot $(Quote-Ps $RepoRoot) -Seats $cleanupSeats"
}
else {
    $includeBlock = ""
    if ($Include) {
        $includeBlock = "-Include " + (Quote-Ps $Include) + " "
    }
    $doneBlock = ""
    if ($DoneCutoutsDir) {
        $doneBlock = "-DoneCutoutsDir " + (Quote-Ps $DoneCutoutsDir) + " "
    }
    $triageBlock = ""
    if ($TriageLog) {
        $triageBlock = "-TriageLog " + (Quote-Ps $TriageLog) + " "
    }
    $pileBlock = ""
    if ($OnlyPile) {
        $pileBlock = "-OnlyPile " + (Quote-Ps $OnlyPile) + " "
    }
    $ignoreBlock = ""
    if ($IgnoreDoneCutouts) {
        $ignoreBlock = "-IgnoreDoneCutouts "
    }
    $invoke = "& $(Quote-Ps $runFolder) -InputDir $(Quote-Ps $InputDir) -OutputDir $(Quote-Ps $outRoot) -Model $(Quote-Ps $Model) -TimeoutSeconds $TimeoutSeconds -RepoRoot $(Quote-Ps $RepoRoot) -Seats $Seats $includeBlock$doneBlock$triageBlock$pileBlock$ignoreBlock"
}

@"
`$ErrorActionPreference = 'Continue'
try { `$Host.UI.RawUI.WindowTitle = $(Quote-Ps $title) } catch {}
Start-Transcript -LiteralPath $(Quote-Ps $logFile) -Append | Out-Null
Set-Location -LiteralPath $(Quote-Ps $RepoRoot)
Write-Output ('detached host pid=' + `$PID)
Write-Output ('job=' + $(Quote-Ps $Job))
Write-Output ('input=' + $(Quote-Ps $InputDir))
Write-Output ('output=' + $(Quote-Ps $outRoot))
$invoke
`$code = `$LASTEXITCODE
Write-Output ('job exit=' + `$code)
Stop-Transcript | Out-Null
Write-Host ''
Write-Host 'Done. Press Enter to close this window.'
try { [void][Console]::ReadLine() } catch { Start-Sleep -Seconds 30 }
exit `$code
"@ | Set-Content -LiteralPath $hostPs -Encoding UTF8

$psExe = Join-Path $env:SystemRoot "System32\WindowsPowerShell\v1.0\powershell.exe"
$cmdLine = "`"$psExe`" -NoProfile -ExecutionPolicy Bypass -File `"$hostPs`""

if (-not ("Win32Detach" -as [type])) {
    Add-Type -TypeDefinition @"
using System;
using System.Runtime.InteropServices;
using System.Text;
public static class Win32Detach {
    public const uint CREATE_NEW_CONSOLE = 0x00000010;
    public const uint CREATE_NEW_PROCESS_GROUP = 0x00000200;
    public const uint CREATE_BREAKAWAY_FROM_JOB = 0x01000000;
    public const uint CREATE_UNICODE_ENVIRONMENT = 0x00000400;
    public const int STARTF_USESHOWWINDOW = 0x00000001;
    public const short SW_SHOW = 5;

    [StructLayout(LayoutKind.Sequential, CharSet = CharSet.Unicode)]
    public struct STARTUPINFO {
        public int cb;
        public string lpReserved;
        public string lpDesktop;
        public string lpTitle;
        public int dwX, dwY, dwXSize, dwYSize, dwXCountChars, dwYCountChars, dwFillAttribute, dwFlags;
        public short wShowWindow, cbReserved2;
        public IntPtr lpReserved2, hStdInput, hStdOutput, hStdError;
    }

    [StructLayout(LayoutKind.Sequential)]
    public struct PROCESS_INFORMATION {
        public IntPtr hProcess, hThread;
        public int dwProcessId, dwThreadId;
    }

    [DllImport("kernel32.dll", SetLastError = true, CharSet = CharSet.Unicode)]
    public static extern bool CreateProcess(
        string lpApplicationName,
        StringBuilder lpCommandLine,
        IntPtr lpProcessAttributes,
        IntPtr lpThreadAttributes,
        bool bInheritHandles,
        uint dwCreationFlags,
        IntPtr lpEnvironment,
        string lpCurrentDirectory,
        ref STARTUPINFO lpStartupInfo,
        out PROCESS_INFORMATION lpProcessInformation);

    [DllImport("kernel32.dll", SetLastError = true)]
    public static extern bool CloseHandle(IntPtr hObject);
}
"@
}

function Start-Breakaway([uint32]$flags) {
    $si = New-Object Win32Detach+STARTUPINFO
    $si.cb = [Runtime.InteropServices.Marshal]::SizeOf($si)
    $si.dwFlags = [Win32Detach]::STARTF_USESHOWWINDOW
    $si.wShowWindow = [Win32Detach]::SW_SHOW
    $si.lpTitle = $title
    $pi = New-Object Win32Detach+PROCESS_INFORMATION
    $sb = New-Object System.Text.StringBuilder $cmdLine
    $ok = [Win32Detach]::CreateProcess(
        $psExe, $sb, [IntPtr]::Zero, [IntPtr]::Zero, $false, $flags,
        [IntPtr]::Zero, $RepoRoot, [ref]$si, [ref]$pi)
    if (-not $ok) {
        return @{ Ok = $false; Pid = 0; Error = [Runtime.InteropServices.Marshal]::GetLastWin32Error() }
    }
    [void][Win32Detach]::CloseHandle($pi.hThread)
    [void][Win32Detach]::CloseHandle($pi.hProcess)
    return @{ Ok = $true; Pid = $pi.dwProcessId; Error = 0 }
}

$flags = [Win32Detach]::CREATE_NEW_CONSOLE -bor [Win32Detach]::CREATE_NEW_PROCESS_GROUP -bor [Win32Detach]::CREATE_BREAKAWAY_FROM_JOB -bor [Win32Detach]::CREATE_UNICODE_ENVIRONMENT
$result = Start-Breakaway $flags
if (-not $result.Ok) {
    Write-Output ("CreateProcess breakaway failed win32=" + $result.Error + "; retry without breakaway, then schtasks")
    $flags2 = [Win32Detach]::CREATE_NEW_CONSOLE -bor [Win32Detach]::CREATE_NEW_PROCESS_GROUP -bor [Win32Detach]::CREATE_UNICODE_ENVIRONMENT
    $result = Start-Breakaway $flags2
}

if (-not $result.Ok) {
    $task = "carousel-canvas-subject-extract"
    $tr = $cmdLine.Replace('"', '\"')
    & schtasks.exe /Create /TN $task /TR $cmdLine /SC ONCE /ST 23:59 /F /RL LIMITED | Out-Null
    if ($LASTEXITCODE -ne 0) {
        Write-Error "could not start detached process (CreateProcess win32=$($result.Error); schtasks failed)"
        exit 2
    }
    & schtasks.exe /Run /TN $task | Out-Null
    Start-Sleep -Seconds 2
    $proc = Get-CimInstance Win32_Process | Where-Object {
        $_.CommandLine -and $_.CommandLine -like "*run-detached-host.ps1*"
    } | Select-Object -First 1
    if (-not $proc) {
        Write-Error "schtasks ran but host process not found"
        exit 2
    }
    $result = @{ Ok = $true; Pid = $proc.ProcessId; Error = 0 }
    Write-Output "started via scheduled task (fallback)"
}

Set-Content -LiteralPath $pidFile -Value ([string]$result.Pid) -Encoding ASCII
Write-Output ("detached pid=" + $result.Pid)
Write-Output ("pid file=" + $pidFile)
Write-Output ("log=" + $logFile)
Write-Output "Cursor can close. A console window is running the folder job."
exit 0
