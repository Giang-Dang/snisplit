param(
  [string] $OutDir = $env:TEMP,
  [switch] $AfterRestart
)
$ErrorActionPreference = 'Continue'
$built = Join-Path $PSScriptRoot '..\..\target\release\snisplit.exe'
$log = Join-Path $OutDir 'm5-foreground.log'
$relayAddresses = '127.0.0.2', '127.0.0.3', '127.0.0.4'

Add-Type -Namespace Win -Name Console -MemberDefinition @'
[DllImport("kernel32.dll")] public static extern bool GenerateConsoleCtrlEvent(uint ctrlEvent, uint processGroupId);
[DllImport("kernel32.dll")] public static extern bool SetConsoleCtrlHandler(IntPtr handler, bool add);
'@

function Log {
  param([Parameter(ValueFromPipeline)] $Line)
  process { Add-Content -Path $log -Value "$Line"; Write-Host "$Line" }
}
function Say($text) { Write-Host "`n>>> $text`n" -ForegroundColor Yellow; Add-Content -Path $log -Value ">>> $text" }
function Rules { @(Get-DnsClientNrptRule | Where-Object Comment -eq 'snisplit') }
function RulesToRelay { @(Get-DnsClientNrptRule | Where-Object { $ns = $_.NameServers; @($relayAddresses | Where-Object { $ns -contains $_ }).Count -gt 0 }) }
function Remove-RelayRules { @(Rules) + @(RulesToRelay) | Sort-Object Name -Unique | ForEach-Object { Remove-DnsClientNrptRule -Name $_.Name -Force }; Clear-DnsClientCache }
function Resolve($name) { ((Resolve-DnsName $name -Type A -DnsOnly -ErrorAction SilentlyContinue | Where-Object IPAddress | ForEach-Object IPAddress | Sort-Object) -join ',') }
function Curl([string] $url) { (curl.exe -s -o NUL -w '%{http_code} ip=%{remote_ip}' --max-time 20 $url 2>&1) -join ' ' }

function Start-Run($name) {
  $out = Join-Path $OutDir "m5-$name.out"
  $err = Join-Path $OutDir "m5-$name.err"
  $p = Start-Process $built -ArgumentList 'run' -NoNewWindow -RedirectStandardOutput $out -RedirectStandardError $err -PassThru
  @{ Process = $p; Out = $out; Err = $err; Started = Get-Date }
}
function Wait-Ready($run) {
  while (-not $run.Process.HasExited -and ((Get-Date) - $run.Started).TotalSeconds -lt 60) {
    if (Select-String $run.Out -Pattern 'Ctrl\+C to stop' -Quiet -ErrorAction SilentlyContinue) { return [int]((Get-Date) - $run.Started).TotalMilliseconds }
    Start-Sleep -Milliseconds 50
  }
  -1
}
function Send-CtrlC($run) {
  [Win.Console]::SetConsoleCtrlHandler([IntPtr]::Zero, $true) | Out-Null
  [Win.Console]::GenerateConsoleCtrlEvent(0, 0) | Out-Null
  $exited = $run.Process.WaitForExit(30000)
  Start-Sleep -Milliseconds 300
  [Win.Console]::SetConsoleCtrlHandler([IntPtr]::Zero, $false) | Out-Null
  if (-not $exited) {
    Log 'snisplit did not stop on Ctrl+C; killing it and removing rules by name'
    Stop-Process -Id $run.Process.Id -Force
    Remove-RelayRules
  }
  $exited
}
function Log-Run($run) {
  Log "exit code: $($run.Process.ExitCode)"
  Get-Content $run.Out | Where-Object { $_ -notmatch '^(relayed|closed) ' } | ForEach-Object { "  out: $_" } | Log
  Get-Content $run.Err | ForEach-Object { "  err: $_" } | Log
}

"started $(Get-Date -Format s)" | Add-Content $log
if ($AfterRestart) {
  $boot = (Get-CimInstance Win32_OperatingSystem).LastBootUpTime
  Log "after a restart with snisplit running (booted $($boot.ToString('s'))): snisplit rules=$((Rules).Count), rules pointing at the relay=$((RulesToRelay).Count), store.steampowered.com -> $(Resolve 'store.steampowered.com'), example.org -> $(Resolve 'example.org')"
  if ((RulesToRelay).Count -gt 0) { Log 'rules were left; removing them'; Remove-RelayRules; Log "rules now: $((RulesToRelay).Count)" }
  Say "Done. Log: $log."
  exit 0
}
Log "snisplit binary present=$(Test-Path $built); snisplit rules: $((Rules).Count); example.org: $(Resolve 'example.org')"
if (-not (Test-Path $built)) { Say "$built is missing (removed by the antivirus?). Nothing was changed."; exit 1 }
if ((Rules).Count -gt 0) { Log 'stale snisplit rules found; removing them first'; Remove-RelayRules }

Say 'Start-up time and Ctrl+C during start-up. No action needed.'
foreach ($i in 1..3) {
  $run = Start-Run "ready-$i"
  $ready = Wait-Ready $run
  Log "routed run $i ready after $ready ms; store https: $(Curl 'https://store.steampowered.com/')"
  $null = Send-CtrlC $run
  Log-Run $run
  Log "rules after run ${i}: $((Rules).Count)"
}
foreach ($delay in 50, 100, 200, 500, 1000, 2000) {
  $run = Start-Run "ctrlc-$delay"
  Start-Sleep -Milliseconds $delay
  $exited = Send-CtrlC $run
  $readyLine = [bool](Select-String $run.Out -Pattern 'Ctrl\+C to stop' -Quiet -ErrorAction SilentlyContinue)
  Log "Ctrl+C at $delay ms: exited on its own=$exited, had reached ready=$readyLine, snisplit rules left=$((Rules).Count), rules pointing at the relay=$((RulesToRelay).Count)"
  Log-Run $run
  if ((RulesToRelay).Count -gt 0) { Log 'RULES LEFT after Ctrl+C; removing them'; Remove-RelayRules }
}

Say 'Closing the window: snisplit opens in a new window. When it says Ctrl+C to stop, close that window with its X button.'
$windowed = Start-Process $built -ArgumentList 'run' -PassThru
$closedInTime = $windowed.WaitForExit(180000)
Start-Sleep 2
Log "closed with X: exited=$closedInTime, exit code $(if ($closedInTime) { $windowed.ExitCode }), snisplit rules left=$((Rules).Count), rules pointing at the relay=$((RulesToRelay).Count)"
if (-not $closedInTime) { Stop-Process -Id $windowed.Id -Force }
if ((RulesToRelay).Count -gt 0) { Log 'RULES LEFT after closing the window; removing them'; Remove-RelayRules }

Log "example.org at the end: $(Resolve 'example.org'); snisplit rules left: $((Rules).Count)"
Say "Done. Log: $log. You can close this window."
Log "finished $(Get-Date -Format s)"
