param(
  [string] $OutDir = $env:TEMP,
  [int] $DownloadObserveSeconds = 120,
  [switch] $SkipSteam
)
$ErrorActionPreference = 'Continue'
$exe = (Resolve-Path (Join-Path $PSScriptRoot '..\..\target\release\snisplit.exe')).Path
$log = Join-Path $OutDir 'm4-acceptance.log'
$relayAddresses = '127.0.0.2', '127.0.0.3', '127.0.0.4'
$contentUrl = 'http://cache5-hkg1.steamcontent.com/'

Add-Type -Namespace Win -Name Console -MemberDefinition @'
[DllImport("kernel32.dll")] public static extern bool GenerateConsoleCtrlEvent(uint ctrlEvent, uint processGroupId);
[DllImport("kernel32.dll")] public static extern bool SetConsoleCtrlHandler(IntPtr handler, bool add);
'@

function Log {
  param([Parameter(ValueFromPipeline)] $Line)
  process { "$Line" | Tee-Object -FilePath $log -Append }
}
function Say($text) { Write-Host "`n>>> $text`n" -ForegroundColor Yellow; Log ">>> $text" }
function Winws { [bool](Get-Process winws -ErrorAction SilentlyContinue) }
function Rules { @(Get-DnsClientNrptRule | Where-Object Comment -eq 'snisplit') }
function RulesToRelay { @(Get-DnsClientNrptRule | Where-Object { $ns = $_.NameServers; @($relayAddresses | Where-Object { $ns -contains $_ }).Count -gt 0 }) }
function Remove-RelayRules { RulesToRelay | ForEach-Object { Remove-DnsClientNrptRule -Name $_.Name -Force }; Clear-DnsClientCache }
function Resolve($name, $type = 'A') { ((Resolve-DnsName $name -Type $type -DnsOnly -ErrorAction SilentlyContinue | Where-Object IPAddress | ForEach-Object IPAddress | Sort-Object) -join ',') }
function Curl([string[]] $urls, [string[]] $extra = @()) {
  (curl.exe -s -o NUL -w '%{http_code} ip=%{remote_ip} connects=%{num_connects};' --max-time 20 @extra @urls 2>&1) -join ' '
}

function Start-Snisplit($name, [string[]] $runArgs) {
  $out = Join-Path $OutDir "m4-$name.out"
  $err = Join-Path $OutDir "m4-$name.err"
  $p = Start-Process $exe -ArgumentList $runArgs -NoNewWindow -RedirectStandardOutput $out -RedirectStandardError $err -PassThru
  $t0 = Get-Date
  $ready = $false
  while (-not $ready -and -not $p.HasExited -and ((Get-Date) - $t0).TotalSeconds -lt 90) {
    $ready = [bool](Select-String $out -Pattern 'Ctrl\+C to stop' -Quiet -ErrorAction SilentlyContinue)
    if (-not $ready) { Start-Sleep -Milliseconds 200 }
  }
  Log "snisplit $($runArgs -join ' '): ready=$ready after $([int]((Get-Date) - $t0).TotalMilliseconds) ms"
  @{ Process = $p; Out = $out; Err = $err; Ready = $ready }
}

function Stop-Snisplit($run) {
  [Win.Console]::SetConsoleCtrlHandler([IntPtr]::Zero, $true) | Out-Null
  [Win.Console]::GenerateConsoleCtrlEvent(0, 0) | Out-Null
  $exited = $run.Process.WaitForExit(20000)
  Start-Sleep -Milliseconds 300
  [Win.Console]::SetConsoleCtrlHandler([IntPtr]::Zero, $false) | Out-Null
  if (-not $exited) {
    Log 'snisplit did not stop on Ctrl+C; killing it and running cleanup'
    Stop-Process -Id $run.Process.Id -Force
    & $exe cleanup | Log
  }
  Log "snisplit exit code: $($run.Process.ExitCode)"
  Log '--- snisplit output'
  Get-Content $run.Out | Log
  Get-Content $run.Err | ForEach-Object { "stderr: $_" } | Log
}

function Closed-Lines($run) { @(Get-Content $run.Out | Where-Object { $_ -match '^closed ' }) }

"started $(Get-Date -Format s)" | Set-Content $log
Log "zapret before: $((Get-Service zapret-steam).Status), winws=$(Winws); snisplit rules before: $((Rules).Count)"
$exampleBefore = Resolve 'example.org'
Log "example.org before: $exampleBefore"

Say 'Part 1: medium.com over HTTP (zapret keeps running). No action needed.'
Log "control, direct, two requests: $(Curl 'http://medium.com/', 'http://medium.com/')"
$profileFile = Join-Path $OutDir 'm4-medium-copy.profile'
@('name medium-copy', 'domains medium.com', 'tls-ports 443', 'tls-split 1', 'http-ports 80', 'http-method host-spell') | Set-Content $profileFile -Encoding ascii
$run = Start-Snisplit 'file' @('run', '--no-routing', '--profile-file', $profileFile, '--verbose')
try {
  if (-not $run.Ready) { throw 'profile-file run did not start' }
  Log "profile file, https via 127.0.0.4: $(Curl 'https://medium.com/' @('--resolve', 'medium.com:443:127.0.0.4'))"
  Log "profile file, http via 127.0.0.4, two requests: $(Curl 'http://medium.com/', 'http://medium.com/' @('--resolve', 'medium.com:80:127.0.0.4'))"
}
catch { Log "error: $_" }
finally { Stop-Snisplit $run }

$run = Start-Snisplit 'medium' @('run', '--profile', 'medium', '--verbose')
try {
  if (-not $run.Ready) { throw 'medium run did not start' }
  Log "resolve medium.com: $(Resolve 'medium.com')"
  Log "http, two requests on one connection: $(Curl 'http://medium.com/', 'http://medium.com/')"
  Log "https: $(Curl 'https://medium.com/')"
  Log "example.org during: $(Resolve 'example.org')"
}
catch { Log "error: $_" }
finally { Stop-Snisplit $run }
Log "after part 1: rules pointing at the relay: $((RulesToRelay).Count); medium.com -> $(Resolve 'medium.com')"
if ((RulesToRelay).Count -gt 0) {
  Log 'PART 1 CLEANUP CHECK FAILED: removing rules by name server; Steam part skipped, zapret untouched'
  Remove-RelayRules
  Say 'Part 1 failed its cleanup check. Steam part skipped; zapret was not touched. You can close this window.'
  exit 1
}

if ($SkipSteam) { Log 'Part 2 skipped (-SkipSteam)' } else {
Say 'Part 2: Steam over HTTP with zapret stopped. Later you will be asked to start a small download.'
$ipv6Name = 'ipv6check-http.steamserver.net'
$ipv6Before = "A=[$(Resolve $ipv6Name 'A')] AAAA=[$(Resolve $ipv6Name 'AAAA')]"
Log "$ipv6Name before: $ipv6Before"
$run = $null
try {
  Stop-Service zapret-steam -Force
  $deadline = (Get-Date).AddSeconds(15)
  while ((Winws) -and (Get-Date) -lt $deadline) { Start-Sleep -Milliseconds 500 }
  Log "zapret stopped: $((Get-Service zapret-steam).Status), winws=$(Winws)"
  if (Winws) { throw 'winws still running; Steam part skipped' }
  Log "control, direct: store http $(Curl 'http://store.steampowered.com/'); content $(Curl $contentUrl)"
  $run = Start-Snisplit 'steam' @('run', '--verbose')
  if (-not $run.Ready) { throw 'steam run did not start' }
  Log "resolve store.steampowered.com: $(Resolve 'store.steampowered.com')"
  Log "store http: $(Curl 'http://store.steampowered.com/')"
  Log "content http: $(Curl $contentUrl)"
  Log "store http, two requests on one connection: $(Curl 'http://store.steampowered.com/', 'http://store.steampowered.com/about/')"
  Log "store https: $(Curl 'https://store.steampowered.com/')"
  Log "$ipv6Name during: A=[$(Resolve $ipv6Name 'A')] AAAA=[$(Resolve $ipv6Name 'AAAA')]"
  Log "example.org during: $(Resolve 'example.org')"
  $before = (Closed-Lines $run).Count
  Say "NOW (optional): in Steam, start a small download or update. Watching $DownloadObserveSeconds s. Pause it afterwards if you like."
  $ports = @{}
  $end = (Get-Date).AddSeconds($DownloadObserveSeconds)
  $steamProcs = @(Get-Process steam -ErrorAction SilentlyContinue | ForEach-Object Id)
  while ((Get-Date) -lt $end) {
    Get-NetTCPConnection -State Established -ErrorAction SilentlyContinue | Where-Object { $steamProcs -contains [int]$_.OwningProcess } | ForEach-Object { $ports["$($_.RemoteAddress):$($_.RemotePort)"] = 1 }
    Start-Sleep 2
  }
  Log "steam.exe remote endpoints seen: $(($ports.Keys | Sort-Object) -join ' ')"
  $closed = @(Closed-Lines $run | Select-Object -Skip $before)
  Log "connections closed during the download window: $($closed.Count)"
  $closed | ForEach-Object { if ($_ -match '^closed ([^:]+):(\d+): (\d+) bytes sent, (\d+) received, (\d+)') { "{0}:{1} received={2} respelled={3}" -f ($Matches[1] -replace '^[^.]+', '*'), $Matches[2], $Matches[4], $Matches[5] } } | Group-Object { ($_ -split ' ')[0] } | ForEach-Object { "{0,4} {1} received={2}" -f $_.Count, $_.Name, (($_.Group | ForEach-Object { [long](($_ -split ' ')[1] -replace 'received=') }) | Measure-Object -Sum).Sum } | Log
}
catch { Log "error: $_" }
finally {
  if ($run) { Stop-Snisplit $run }
  if ((Rules).Count -gt 0) { & $exe cleanup | Log }
  if ((RulesToRelay).Count -gt 0) { Log 'rules pointing at the relay remain; removing by name server'; Remove-RelayRules }
  Start-Service zapret-steam
  Start-Sleep 2
  Log "zapret restored: $((Get-Service zapret-steam).Status), winws=$(Winws); snisplit rules left: $((Rules).Count)"
  Log "$ipv6Name after: A=[$(Resolve $ipv6Name 'A')] AAAA=[$(Resolve $ipv6Name 'AAAA')]"
}
}
Log "example.org after: $(Resolve 'example.org') (before: $exampleBefore)"
Say 'Done. You can close this window.'
Log "finished $(Get-Date -Format s)"
