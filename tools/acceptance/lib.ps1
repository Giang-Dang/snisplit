$script:releaseFolder = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..\..\target\release'))
$script:guiBinary = Join-Path $script:releaseFolder 'snisplit-gui.exe'
$script:cliBinary = Join-Path $script:releaseFolder 'snisplit.exe'
$script:ctrlSender = Join-Path $PSScriptRoot 'send-ctrl-c.ps1'
$script:appFolder = Join-Path $env:APPDATA 'snisplit'
$script:sitesFolder = Join-Path $script:appFolder 'sites'
$script:offList = Join-Path $script:appFolder 'off.list'
$script:relayAddresses = 2..20 | ForEach-Object { "127.0.0.$_" }
$script:started = @()
$script:checks = 0
$script:failures = @()
$script:offListSaved = $null
$script:log = $null
$script:shots = $null
$script:outputs = $null
$script:ctrlCWaitSeconds = 15
$script:stopWaitSeconds = 75

Add-Type -AssemblyName System.Drawing
if (-not ('SnisplitAcceptance.Native' -as [type])) {
  Add-Type -Namespace SnisplitAcceptance -Name Native -MemberDefinition @'
[StructLayout(LayoutKind.Sequential)] public struct Rect { public int Left; public int Top; public int Right; public int Bottom; }
[DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr window, out Rect rect);
[DllImport("user32.dll")] public static extern bool PrintWindow(IntPtr window, IntPtr deviceContext, uint flags);
[DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
[DllImport("kernel32.dll")] public static extern bool SetConsoleCtrlHandler(IntPtr handler, bool add);
'@
}
$null = [SnisplitAcceptance.Native]::SetProcessDPIAware()
$null = [SnisplitAcceptance.Native]::SetConsoleCtrlHandler([IntPtr]::Zero, $false)

function Open-Log($outDir, $logName, $shotsName) {
  $script:log = Join-Path $outDir $logName
  $script:shots = Join-Path $outDir $shotsName
  $script:outputs = Join-Path $script:shots 'output'
  $null = New-Item -ItemType Directory -Force -Path $script:shots
  $null = New-Item -ItemType Directory -Force -Path $script:outputs
  Add-Content -Path $script:log -Value "started $(Get-Date -Format s)"
}
function Log {
  param([Parameter(ValueFromPipeline)] $Line)
  process { Add-Content -Path $script:log -Value "$Line"; Write-Host "$Line" }
}
function Say($text) { Write-Host "`n>>> $text`n" -ForegroundColor Yellow; Add-Content -Path $script:log -Value ">>> $text" }
function Ask($question) { $answer = Read-Host $question; Add-Content -Path $script:log -Value "  ${question}: $answer"; $answer }
function Expect($label, $condition) {
  $script:checks++
  if ($condition) { Log "  PASS  $label" } else { Log "  FAIL  $label"; $script:failures += $label }
}

function Is-Elevated { ([Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator) }
function Snisplit-Processes { @(Get-Process snisplit, snisplit-gui -ErrorAction SilentlyContinue) }
function Site-Files { @(Get-ChildItem -LiteralPath $script:sitesFolder -Filter *.profile -ErrorAction SilentlyContinue) }
function Off-List-Names($name) {
  if (-not (Test-Path -LiteralPath $script:offList)) { return $false }
  @(Get-Content -LiteralPath $script:offList | Where-Object { $_.Trim() -eq $name }).Count -gt 0
}

function Rules { @(Get-DnsClientNrptRule | Where-Object Comment -eq 'snisplit') }
function RulesToRelay { @(Get-DnsClientNrptRule | Where-Object { $servers = $_.NameServers; @($script:relayAddresses | Where-Object { $servers -contains $_ }).Count -gt 0 }) }
function Rule-Count { (Rules).Count }
function Remove-RelayRules {
  $foreign = @(RulesToRelay | Where-Object { $_.Comment -ne 'snisplit' })
  if ($foreign.Count -gt 0) {
    $namespaces = @($foreign | ForEach-Object { $_.Namespace }) -join ', '
    Log "  $($foreign.Count) rule(s) point at 127.0.0.2-20 but do not carry Comment 'snisplit' (they may belong to another tool); removing them too. Namespaces: $namespaces"
  }
  @(Rules) + @(RulesToRelay) | Sort-Object Name -Unique | ForEach-Object { Remove-DnsClientNrptRule -Name $_.Name -Force }
  Clear-DnsClientCache
}
function Addresses($name) { @(Resolve-DnsName $name -Type A -DnsOnly -ErrorAction SilentlyContinue | Where-Object IPAddress | ForEach-Object IPAddress | Sort-Object) }
function Resolve($name) { (Addresses $name) -join ',' }
function Goes-To-Relay($name) {
  Clear-DnsClientCache
  $found = Addresses $name
  ($found.Count -gt 0) -and (@($found | Where-Object { $_ -notlike '127.0.0.*' }).Count -eq 0)
}
function Fetch([string] $url) { (curl.exe -s -o NUL -w '%{http_code} ip=%{remote_ip}' --max-time 20 $url 2>&1) -join ' ' }
function Status-Code([string] $url) { (curl.exe -s -o NUL -w '%{http_code}' --max-time 20 $url 2>&1) -join ' ' }
function Log-State($step) {
  Clear-DnsClientCache
  Log "$step | processes: $((Snisplit-Processes).Name -join ',') | snisplit rules: $(Rule-Count), rules pointing at the relay: $((RulesToRelay).Count) | site files: $((Site-Files).Count) | off.list names medium: $(Off-List-Names 'medium')"
  Log "  store.steampowered.com -> $(Resolve 'store.steampowered.com'); medium.com -> $(Resolve 'medium.com')"
}

function Wait-Until([scriptblock] $untilTrue, [int] $limitSeconds) {
  $deadline = (Get-Date).AddSeconds($limitSeconds)
  while ((Get-Date) -lt $deadline) {
    if (& $untilTrue) { return $true }
    Start-Sleep -Milliseconds 700
  }
  [bool](& $untilTrue)
}
function Wait-Stable([scriptblock] $condition, [ValidateRange(4, 10000)] [int] $polls, [int] $limitSeconds, [int] $pollMilliseconds = 700) {
  $deadline = (Get-Date).AddSeconds($limitSeconds)
  $streak = 0
  while ($true) {
    if (& $condition) { $streak++ } else { $streak = 0 }
    if ($streak -ge $polls) { return $true }
    if ((Get-Date) -ge $deadline) { return $false }
    Start-Sleep -Milliseconds $pollMilliseconds
  }
}
function Added-Paths($currentPaths, $pathsBefore) {
  if ($null -eq $pathsBefore) { return @() }
  @($currentPaths | Where-Object { $pathsBefore -notcontains $_ })
}
function Resolve-Full($path) { [IO.Path]::GetFullPath($ExecutionContext.SessionState.Path.GetUnresolvedProviderPathFromPSPath($path)).TrimEnd('\', '/') }
function Is-Inside($path, $folder) {
  $full = Resolve-Full $path
  $root = Resolve-Full $folder
  ($full -eq $root) -or $full.StartsWith("$root\", [StringComparison]::OrdinalIgnoreCase)
}
function Refuse-OutDir-In-Repo($outDir) {
  $repository = Join-Path $PSScriptRoot '..\..'
  if (Is-Inside $outDir $repository) {
    Say "-OutDir is inside the repository ($(Resolve-Full $repository)). Screenshots and logs can name your own sites; pick a folder outside it. Nothing was changed."
    return $true
  }
  $false
}
function Warn-Do-Not-Close {
  Write-Host 'Do not close this window while the script runs. If it closes anyway, run `snisplit cleanup` from an elevated prompt and check that Medium is switched on in the snisplit window.' -ForegroundColor Red
}
function Kill-If-Running($process) {
  try { if (-not $process.HasExited) { $process.Kill() } } catch { $null = $_ }
}
function Wait-Rules([int] $count, [int] $limitSeconds) { Wait-Until { (Rule-Count) -eq $count } $limitSeconds }
function Wait-Rules-Settled([int] $limitSeconds) {
  $script:settledSeen = @()
  Wait-Until {
    $script:settledSeen = @($script:settledSeen) + (Rule-Count)
    $recent = @($script:settledSeen | Select-Object -Last 4)
    ($recent.Count -eq 4) -and ($recent[0] -gt 0) -and (@($recent | Sort-Object -Unique).Count -eq 1)
  } $limitSeconds
}

function Read-Shared($path) {
  if (-not $path -or -not (Test-Path -LiteralPath $path)) { return '' }
  try {
    $stream = [IO.File]::Open($path, [IO.FileMode]::Open, [IO.FileAccess]::Read, [IO.FileShare]::ReadWrite -bor [IO.FileShare]::Delete)
    $reader = New-Object IO.StreamReader $stream
    try { $reader.ReadToEnd() } finally { $reader.Dispose() }
  } catch { '' }
}
function Start-Tracked($binary, [string[]] $arguments, $style, $outputName) {
  $options = @{ FilePath = $binary; PassThru = $true; WindowStyle = $style }
  if ($arguments) { $options.ArgumentList = $arguments }
  $outFile = $null
  $errFile = $null
  if ($outputName) {
    $outFile = Join-Path $script:outputs "$outputName.out.txt"
    $errFile = Join-Path $script:outputs "$outputName.err.txt"
    $options.RedirectStandardOutput = $outFile
    $options.RedirectStandardError = $errFile
  }
  $process = Start-Process @options
  $null = $process.Handle
  $script:started += $process
  $process | Add-Member -NotePropertyName OutFile -NotePropertyValue $outFile -PassThru | Add-Member -NotePropertyName ErrFile -NotePropertyValue $errFile -PassThru
}
function Start-Window { Start-Tracked $script:guiBinary $null 'Normal' $null }
function Start-Run($outputName) { Start-Tracked $script:cliBinary @('run') 'Minimized' $outputName }
function Run-Is-On($process) { (Read-Shared $process.OutFile) -match 'Ctrl\+C to stop' }
function Wait-Run-On($process, [int] $limitSeconds) {
  $null = Wait-Until { $process.HasExited -or (Run-Is-On $process) } $limitSeconds
  (-not $process.HasExited) -and (Run-Is-On $process)
}
function Invoke-Cli([string[]] $arguments, $outputName, [int] $limitSeconds) {
  $process = Start-Tracked $script:cliBinary $arguments 'Minimized' $outputName
  $exited = $process.WaitForExit($limitSeconds * 1000)
  if (-not $exited) { Kill-If-Running $process; $null = $process.WaitForExit(10000) }
  [pscustomobject]@{
    Exited = $exited
    ExitCode = $(if ($exited) { $process.ExitCode })
    Out = Read-Shared $process.OutFile
    Err = Read-Shared $process.ErrFile
  }
}
function Close-Started {
  foreach ($process in $script:started) {
    if (-not $process.HasExited) {
      Log "  ending $($process.ProcessName) (process $($process.Id)), which this script started"
      Kill-If-Running $process
      $null = $process.WaitForExit(20000)
    }
  }
}

function Send-ControlEvent($process, [int] $controlEvent) {
  $hostBinary = (Get-Process -Id $PID).Path
  $arguments = @('-NoProfile', '-ExecutionPolicy', 'Bypass', '-File', "`"$script:ctrlSender`"", '-ProcessId', $process.Id, '-ControlEvent', $controlEvent)
  $sender = Start-Process -FilePath $hostBinary -ArgumentList $arguments -WindowStyle Hidden -PassThru
  $null = $sender.Handle
  $script:started += $sender
  if (-not $sender.WaitForExit(20000)) { Kill-If-Running $sender; return 'timed out' }
  $sender.ExitCode
}
function Stop-Run($process) {
  $watch = [Diagnostics.Stopwatch]::StartNew()
  $method = 'Ctrl+C'
  $sent = Send-ControlEvent $process 0
  Log "  Ctrl+C sent to snisplit run: sender result $sent (0 means delivered)"
  $exited = $process.WaitForExit($script:ctrlCWaitSeconds * 1000)
  if (-not $exited -and -not ((Read-Shared $process.OutFile) -match '(?m)^stopping')) {
    $method = 'Ctrl+Break'
    $sent = Send-ControlEvent $process 1
    Log "  run did not react to Ctrl+C within $($script:ctrlCWaitSeconds) s; Ctrl+Break sent: sender result $sent"
  }
  if (-not $exited) { $exited = $process.WaitForExit($script:stopWaitSeconds * 1000) }
  if (-not $exited) {
    $method = 'kill'
    Log '  run did not stop after the console signals; killing it and running snisplit cleanup. The clean stop of run was NOT exercised.'
    Kill-If-Running $process
    $null = $process.WaitForExit(20000)
    $cleanup = Invoke-Cli @('cleanup') 'cleanup-after-kill' 120
    Log "  snisplit cleanup after the kill: exit code $($cleanup.ExitCode)"
  }
  $watch.Stop()
  $result = [pscustomobject]@{
    Method = $method
    ExitCode = $(if ($method -ne 'kill') { $process.ExitCode })
    Seconds = [math]::Round($watch.Elapsed.TotalSeconds, 1)
    SaidRulesLeftZero = (Read-Shared $process.OutFile) -match 'snisplit DNS rules left: 0'
  }
  Log "  stop of run: method $($result.Method), exit code $($result.ExitCode), $($result.Seconds) s, printed 'rules left: 0': $($result.SaidRulesLeftZero)"
  $result
}

function Wait-Window($process, [int] $limitSeconds) {
  $null = Wait-Until { $process.Refresh(); $process.HasExited -or ($process.MainWindowHandle -ne [IntPtr]::Zero) } $limitSeconds
  (-not $process.HasExited) -and ($process.MainWindowHandle -ne [IntPtr]::Zero)
}
function Looks-Blank($bitmap) {
  $colours = @{}
  for ($x = 0; $x -lt 16; $x++) {
    for ($y = 0; $y -lt 16; $y++) {
      $colours[$bitmap.GetPixel([int](($bitmap.Width - 1) * $x / 15), [int](($bitmap.Height - 1) * $y / 15)).ToArgb()] = $true
    }
  }
  $colours.Count -le 1
}
function Save-Shot($process, $name) {
  $process.Refresh()
  $window = $process.MainWindowHandle
  $path = Join-Path $script:shots "$name.png"
  if ($process.HasExited -or $window -eq [IntPtr]::Zero) { Log "  shot $name`: the process has no window"; return $null }
  $rect = New-Object SnisplitAcceptance.Native+Rect
  if (-not [SnisplitAcceptance.Native]::GetWindowRect($window, [ref] $rect)) { Log "  shot $name`: no window rectangle"; return $null }
  $width = $rect.Right - $rect.Left
  $height = $rect.Bottom - $rect.Top
  if ($width -le 0 -or $height -le 0) { Log "  shot $name`: empty window rectangle"; return $null }
  $bitmap = New-Object System.Drawing.Bitmap $width, $height
  $graphics = [System.Drawing.Graphics]::FromImage($bitmap)
  $context = $graphics.GetHdc()
  $printed = [SnisplitAcceptance.Native]::PrintWindow($window, $context, 2)
  $graphics.ReleaseHdc($context)
  $graphics.Dispose()
  try {
    if (-not $printed) { Log "  shot $name`: the window could not be printed"; return $null }
    $bitmap.Save($path, [System.Drawing.Imaging.ImageFormat]::Png)
    $blank = Looks-Blank $bitmap
    Log "  shot $name`: $path$(if ($blank) { ' (all one colour: probably blank)' })"
    $path
  } finally { $bitmap.Dispose() }
}
function Close-Window($process, [int] $times, [int] $limitSeconds) {
  $watch = [Diagnostics.Stopwatch]::StartNew()
  $accepted = 0
  $notResponding = 0
  $polls = 0
  for ($sent = 0; $sent -lt $times; $sent++) {
    $process.Refresh()
    if (-not $process.HasExited -and $process.CloseMainWindow()) { $accepted++ }
    if ($sent + 1 -lt $times) { Start-Sleep -Milliseconds 300 }
  }
  while (-not $process.HasExited -and $watch.Elapsed.TotalSeconds -lt $limitSeconds) {
    $process.Refresh()
    $polls++
    if (-not $process.HasExited -and $process.MainWindowHandle -ne [IntPtr]::Zero -and -not $process.Responding) { $notResponding++ }
    Start-Sleep -Milliseconds 500
  }
  $exited = $process.WaitForExit(0)
  $watch.Stop()
  $result = [pscustomobject]@{
    Exited = $exited
    ExitCode = $(if ($exited) { $process.ExitCode })
    Seconds = [math]::Round($watch.Elapsed.TotalSeconds, 1)
    ClosesAccepted = $accepted
    Polls = $polls
    NotRespondingPolls = $notResponding
  }
  Log "  close x$times`: exited $($result.Exited), exit code $($result.ExitCode), $($result.Seconds) s, close messages accepted $accepted, polls $polls, polls not responding $notResponding"
  $result
}

function Save-OffList([switch] $RestoreAtEnd) {
  $existed = Test-Path -LiteralPath $script:offList
  $bytes = $null
  if ($existed) { $bytes = [IO.File]::ReadAllBytes($script:offList) }
  $script:offListSaved = [pscustomobject]@{ Existed = $existed; Bytes = $bytes; Changed = [bool]$RestoreAtEnd }
  Log "  off.list saved: existed $existed$(if ($existed) { ", $($bytes.Length) bytes" })"
}
function Add-OffListName($name) {
  $script:offListSaved.Changed = $true
  $null = New-Item -ItemType Directory -Force -Path $script:appFolder
  [byte[]] $before = @()
  if ($script:offListSaved.Existed) { $before = $script:offListSaved.Bytes }
  [byte[]] $separator = @()
  if ($before.Length -gt 0 -and $before[$before.Length - 1] -ne 10) { $separator = 13, 10 }
  [byte[]] $added = [Text.Encoding]::ASCII.GetBytes("$name`r`n")
  [IO.File]::WriteAllBytes($script:offList, [byte[]]($before + $separator + $added))
}
function Off-List-Is-As-Saved {
  if (-not $script:offListSaved.Existed) { return -not (Test-Path -LiteralPath $script:offList) }
  if (-not (Test-Path -LiteralPath $script:offList)) { return $false }
  [Convert]::ToBase64String([IO.File]::ReadAllBytes($script:offList)) -eq [Convert]::ToBase64String($script:offListSaved.Bytes)
}
function Restore-OffList {
  if (-not $script:offListSaved -or -not $script:offListSaved.Changed) { return }
  if ($script:offListSaved.Existed) { [IO.File]::WriteAllBytes($script:offList, $script:offListSaved.Bytes) }
  elseif (Test-Path -LiteralPath $script:offList) { Remove-Item -LiteralPath $script:offList -Force }
  $script:offListSaved.Changed = $false
  Log "  off.list restored: same as before the run: $(Off-List-Is-As-Saved)"
}

function Refuse-Unless-Ready($binaries) {
  if (-not (Is-Elevated)) { Say 'Run this script from an elevated PowerShell (Run as administrator). Nothing was changed.'; return $false }
  if ((Snisplit-Processes).Count -gt 0) { Say "A snisplit process is running ($((Snisplit-Processes).Name -join ', ')). Close it, then run this script again. Nothing was changed."; return $false }
  foreach ($binary in $binaries) {
    if (-not (Test-Path -LiteralPath $binary)) { Say "$binary is missing (not built, or removed by the antivirus?). Nothing was changed."; return $false }
  }
  if (Off-List-Names 'medium') { Say 'Medium is switched off from an earlier run. Start snisplit-gui, switch Medium on, close it, then run this script again. Nothing was changed.'; return $false }
  if (Off-List-Names 'steam') { Say 'Steam is switched off from an earlier run. Start snisplit-gui, switch Steam on, close it, then run this script again. Nothing was changed.'; return $false }
  $true
}
function Rules-In-Place { @(@(Rules) + @(RulesToRelay) | Sort-Object Name -Unique).Count }
function Finish-Run($completed) {
  if (-not $completed) { Log "the run was aborted $(Get-Date -Format s)"; $script:failures += 'the run did not complete' }
  Close-Started
  Restore-OffList
  $left = Rules-In-Place
  Log "rules still in place right after ending the processes: $left"
  if ($left -gt 0) { Remove-RelayRules }
  Start-Sleep -Seconds 5
  $later = Rules-In-Place
  Log "rules in place 5 s later: $later"
  if ($later -gt 0) { Remove-RelayRules }
  Log "at the end: snisplit rules: $(Rule-Count); rules pointing at the relay: $((RulesToRelay).Count); snisplit processes: $((Snisplit-Processes).Count)"
  if ($script:failures.Count -eq 0) {
    Say "RESULT: PASS ($($script:checks) checks)"
  } else {
    Say "RESULT: FAIL ($($script:failures.Count) of $($script:checks) checks failed)"
    $script:failures | ForEach-Object { Log "  - $_" }
  }
  Log "finished $(Get-Date -Format s)"
  if ($script:failures.Count -eq 0) { 0 } else { 1 }
}
