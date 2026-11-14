param(
  [string] $OutDir = $env:TEMP,
  [switch] $SwitchToSnisplit,
  [switch] $AfterReboot,
  [int] $SteamWindowSeconds = 120,
  [int] $DownloadObserveSeconds = 180
)
$ErrorActionPreference = 'Continue'
$built = Join-Path $PSScriptRoot '..\..\target\release\snisplit.exe'
$installDir = Join-Path $env:ProgramFiles 'snisplit'
$installedExe = Join-Path $installDir 'snisplit.exe'
$serviceLog = Join-Path $installDir 'logs\snisplit.log'
$part = if ($AfterReboot) { 'after-reboot' } elseif ($SwitchToSnisplit) { 'switch' } else { 'part1' }
$log = Join-Path $OutDir "m5-$part.log"
$relayAddresses = '127.0.0.2', '127.0.0.3'
$contentUrl = 'http://cache5-hkg1.steamcontent.com/'
$zapretBundleNote = 'to bring zapret back: run steam_service_install.cmd from the zapret-win-bundle zapret-winws folder as administrator'

Add-Type -Namespace Win -Name Console -MemberDefinition @'
[DllImport("kernel32.dll")] public static extern bool GenerateConsoleCtrlEvent(uint ctrlEvent, uint processGroupId);
[DllImport("kernel32.dll")] public static extern bool SetConsoleCtrlHandler(IntPtr handler, bool add);
'@

function Log {
  param([Parameter(ValueFromPipeline)] $Line)
  process { Add-Content -Path $log -Value "$Line"; Write-Host "$Line" }
}
function Say($text) { Write-Host "`n>>> $text`n" -ForegroundColor Yellow; Log ">>> $text" }
function Winws { [bool](Get-Process winws -ErrorAction SilentlyContinue) }
function Rules { @(Get-DnsClientNrptRule | Where-Object Comment -eq 'snisplit') }
function RulesToRelay { @(Get-DnsClientNrptRule | Where-Object { $ns = $_.NameServers; @($relayAddresses | Where-Object { $ns -contains $_ }).Count -gt 0 }) }
function Remove-RelayRules { RulesToRelay | ForEach-Object { Remove-DnsClientNrptRule -Name $_.Name -Force }; Clear-DnsClientCache }
function Resolve($name, $type = 'A') { ((Resolve-DnsName $name -Type $type -DnsOnly -ErrorAction SilentlyContinue | Where-Object IPAddress | ForEach-Object IPAddress | Sort-Object) -join ',') }
function Curl([string[]] $urls, [string[]] $extra = @()) {
  $targets = @($urls | ForEach-Object { '-o'; 'NUL'; $_ })
  (curl.exe -s -w '%{http_code} ip=%{remote_ip} connects=%{num_connects};' --max-time 20 @extra @targets 2>&1) -join ' '
}
function Service { Get-Service snisplit -ErrorAction SilentlyContinue }
function ServicePid { [int](Get-CimInstance Win32_Service -Filter "Name='snisplit'" -ErrorAction SilentlyContinue).ProcessId }
function Relayed { (Resolve 'store.steampowered.com') -eq '127.0.0.2' -and (Rules).Count -gt 0 }
function Wait-Until([scriptblock] $condition, [int] $seconds) {
  $t0 = Get-Date
  while (((Get-Date) - $t0).TotalSeconds -lt $seconds) {
    Clear-DnsClientCache
    if (& $condition) { return [int]((Get-Date) - $t0).TotalMilliseconds }
    Start-Sleep -Milliseconds 250
  }
  -1
}
function Log-Checks($label) {
  Log "$label store http: $(Curl 'http://store.steampowered.com/')"
  Log "$label store https: $(Curl 'https://store.steampowered.com/')"
  Log "$label content http, two requests: $(Curl $contentUrl, $contentUrl)"
  Log "$label medium http, two requests: $(Curl 'http://medium.com/', 'http://medium.com/')"
  Log "$label example.org: $(Resolve 'example.org')"
}
function Log-ServiceTail([int] $lines = 40) {
  if (Test-Path $serviceLog) { Log '--- service log tail'; Get-Content $serviceLog -Tail $lines | Log }
}
function Text-After-Time($line) { $line.Substring($line.IndexOf(' ') + 1) }
function Log-Unknown-Address {
  Log 'DNS cache entries for 23.49.104.*:'
  Get-DnsClientCache -ErrorAction SilentlyContinue | Where-Object { $_.Data -like '23.49.104.*' } | ForEach-Object { "  $($_.Entry) -> $($_.Data)" } | Log
  Get-NetTCPConnection -RemoteAddress 23.49.104.166 -ErrorAction SilentlyContinue | ForEach-Object { "  tcp $($_.LocalPort) -> $($_.RemotePort) $($_.State) by $((Get-Process -Id $_.OwningProcess -ErrorAction SilentlyContinue).Name)" } | Log
}

function Start-Run($name, [string[]] $runArgs) {
  $out = Join-Path $OutDir "m5-$name.out"
  $err = Join-Path $OutDir "m5-$name.err"
  $p = Start-Process $built -ArgumentList $runArgs -NoNewWindow -RedirectStandardOutput $out -RedirectStandardError $err -PassThru
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
    Log 'snisplit did not stop on Ctrl+C; killing it and running cleanup'
    Stop-Process -Id $run.Process.Id -Force
    & $built cleanup | Log
  }
  $exited
}
function Log-Run($run) {
  Log "exit code: $($run.Process.ExitCode)"
  Get-Content $run.Out | Where-Object { $_ -notmatch '^(relayed|closed) ' } | ForEach-Object { "  out: $_" } | Log
  Get-Content $run.Err | ForEach-Object { "  err: $_" } | Log
}

function Install-Service {
  $t0 = Get-Date
  & $built service install --profile steam --profile medium --verbose 2>&1 | ForEach-Object { "  install: $_" } | Log
  $ready = Wait-Until { Relayed } 60
  Log "service: $((Service).Status), pid $(ServicePid); relaying $ready ms after install returned ($([int]((Get-Date) - $t0).TotalMilliseconds) ms in all)"
  $ready -ge 0
}
function Save-ServiceLogs {
  $stamp = Get-Date -Format 'HHmmss'
  foreach ($name in 'snisplit.log', 'snisplit.old.log') {
    $source = Join-Path $installDir "logs\$name"
    if (Test-Path $source) { Copy-Item $source (Join-Path $OutDir "m5-$part-$stamp-$name"); Log "saved $name to $OutDir as m5-$part-$stamp-$name" }
  }
}
function Http-Code([string] $url) { curl.exe -s -o NUL -w '%{http_code}' --max-time 20 $url }
function Remove-ServiceWithoutBinary {
  Log 'removing the service without the snisplit binary: sc stop, sc delete, rules by comment and name server, the install folder'
  sc.exe stop snisplit | Out-Null
  $deadline = (Get-Date).AddSeconds(35)
  while ((Service) -and (Service).Status -ne 'Stopped' -and (Get-Date) -lt $deadline) { Start-Sleep -Milliseconds 500 }
  sc.exe delete snisplit | ForEach-Object { "  sc delete: $_" } | Log
  @(Rules) + @(RulesToRelay) | Sort-Object Name -Unique | ForEach-Object { Remove-DnsClientNrptRule -Name $_.Name -Force }
  Clear-DnsClientCache
  if ([IO.Path]::IsPathFullyQualified($installDir) -and (Test-Path $installDir)) { Remove-Item -LiteralPath $installDir -Recurse -Force -ErrorAction Continue }
}
function Uninstall-Service {
  Save-ServiceLogs
  if (Test-Path $built) { & $built service uninstall 2>&1 | ForEach-Object { "  uninstall: $_" } | Log }
  if ((Service) -or (Test-Path $installDir) -or (Rules).Count -gt 0) { Remove-ServiceWithoutBinary }
  Clear-DnsClientCache
  Log "after uninstall: service present=$([bool](Service)), folder present=$(Test-Path $installDir), snisplit rules=$((Rules).Count), store.steampowered.com -> $(Resolve 'store.steampowered.com')"
}

function Bytes-By-Host([string[]] $lines) {
  $lines | ForEach-Object {
    if ($_ -match '^closed (\S+): (\d+) bytes sent, (\d+) received(, (\d+) Host)?') { [pscustomobject]@{ Key = $Matches[1]; Received = [long]$Matches[3]; Respelled = [int]$Matches[5] } }
  } | Group-Object Key | Sort-Object { ($_.Group | Measure-Object Received -Sum).Sum } -Descending | Select-Object -First 25 | ForEach-Object {
    "{0,4} {1} received={2} respelled={3}" -f $_.Count, $_.Name, ($_.Group | Measure-Object Received -Sum).Sum, ($_.Group | Measure-Object Respelled -Sum).Sum
  }
}

"started $(Get-Date -Format s) ($part)" | Set-Content $log
Log "zapret: $((Get-Service zapret-steam -ErrorAction SilentlyContinue).Status), winws=$(Winws); windivert service present=$([bool](Get-Service windivert -ErrorAction SilentlyContinue)); snisplit service: $((Service).Status); snisplit rules: $((Rules).Count)"
$exampleBefore = Resolve 'example.org'
Log "example.org at start: $exampleBefore"

if (-not $SwitchToSnisplit -and -not $AfterReboot) {
  if (Service) { Say 'A snisplit service is already installed. Run snisplit service uninstall first. Nothing was changed.'; exit 1 }
  if (-not (Test-Path $built)) { Say "$built is missing (removed by the antivirus?). Nothing was changed."; exit 1 }
  try {
    Stop-Service zapret-steam -Force
    $deadline = (Get-Date).AddSeconds(15)
    while ((Winws) -and (Get-Date) -lt $deadline) { Start-Sleep -Milliseconds 500 }
    Log "zapret stopped: $((Get-Service zapret-steam).Status), winws=$(Winws)"
    if (Winws) { throw 'winws still running' }
    Log "control, direct: store http $(Curl 'http://store.steampowered.com/'); store https $(Curl 'https://store.steampowered.com/')"

    Say 'Part 1a: start-up time and Ctrl+C during start-up. No action needed.'
    foreach ($i in 1..3) {
      $run = Start-Run "ready-$i" @('run')
      $ready = Wait-Ready $run
      Log "routed run $i ready after $ready ms; store https: $(Curl 'https://store.steampowered.com/')"
      $null = Send-CtrlC $run
      Log-Run $run
      Log "rules after run ${i}: $((Rules).Count)"
    }
    foreach ($delay in 50, 100, 200, 500, 1000, 2000) {
      $run = Start-Run "ctrlc-$delay" @('run')
      Start-Sleep -Milliseconds $delay
      $exited = Send-CtrlC $run
      $readyLine = [bool](Select-String $run.Out -Pattern 'Ctrl\+C to stop' -Quiet -ErrorAction SilentlyContinue)
      Log "Ctrl+C at $delay ms: exited on its own=$exited, had reached ready=$readyLine, snisplit rules left=$((Rules).Count), rules pointing at the relay=$((RulesToRelay).Count)"
      Log-Run $run
      if ((RulesToRelay).Count -gt 0) { Log 'RULES LEFT after Ctrl+C; removing by name server'; Remove-RelayRules }
    }

    Say 'Part 1b: the service. No action needed yet.'
    $steamBefore = @(Get-Process steam -ErrorAction SilentlyContinue).Count -gt 0
    Log "Steam running before the service starts: $steamBefore"
    if (-not (Install-Service)) { throw 'the service did not start relaying within 60 s' }
    Log-Checks 'service'
    Say "NOW: if Steam was already open, use it for a minute (Store page, library). Watching $SteamWindowSeconds s; the DNS cache is dumped at the end."
    Start-Sleep $SteamWindowSeconds
    Log-Unknown-Address
    Log-ServiceTail 20

    Say 'Part 1c: killing the service process. No action needed.'
    $old = ServicePid
    Stop-Process -Id $old -Force
    $t0 = Get-Date
    Log "killed pid $old; example.org right after the kill: $(Resolve 'example.org') (start: $exampleBefore)"
    $back = Wait-Until { $p = ServicePid; $p -ne 0 -and $p -ne $old -and (Relayed) } 60
    Log "back after the kill: $back ms (new pid $(ServicePid)); example.org: $(Resolve 'example.org')"
    Log-Checks 'after kill'

    Say 'Part 1d: stop, start, uninstall. No action needed.'
    Stop-Service snisplit
    Clear-DnsClientCache
    Log "after Stop-Service: $((Service).Status), snisplit rules=$((Rules).Count)"
    Start-Service snisplit
    Log "after Start-Service: relaying after $(Wait-Until { Relayed } 60) ms"
    Uninstall-Service
  }
  catch { Log "error: $_" }
  finally {
    if (Service) { Log 'service still installed; uninstalling'; Uninstall-Service }
    if ((Rules).Count -gt 0 -and (Test-Path $built)) { & $built cleanup | Log }
    if ((RulesToRelay).Count -gt 0) { Log 'rules pointing at the relay remain; removing by name server'; Remove-RelayRules }
    Start-Service zapret-steam
    Start-Sleep 2
    Log "zapret restored: $((Get-Service zapret-steam).Status), winws=$(Winws); snisplit rules left: $((Rules).Count)"
  }
}

if ($SwitchToSnisplit) {
  if (-not (Test-Path $built)) { Say "$built is missing (removed by the antivirus?). Nothing was changed."; exit 1 }
  Log $zapretBundleNote
  $zapretRemoved = $false
  try {
    if (Service) { throw 'a snisplit service is already installed; run snisplit service uninstall first' }
    Say 'Switching this PC from zapret to snisplit. First snisplit is proven with zapret stopped; only then are zapret-steam and windivert removed (the bundle folder is not touched).'
    Stop-Service zapret-steam -Force
    $deadline = (Get-Date).AddSeconds(15)
    while ((Winws) -and (Get-Date) -lt $deadline) { Start-Sleep -Milliseconds 500 }
    if (Winws) { throw 'winws still running' }
    if (-not (Install-Service)) { throw 'the service did not start relaying within 60 s' }
    Log-Checks 'switched'
    $store = Http-Code 'https://store.steampowered.com/'
    $content = Http-Code $contentUrl
    Log "proof before removing zapret: store https $store, content http $content"
    if ($store -ne '200' -or $content -ne '200') { throw "snisplit did not pass (store https $store, content http $content); zapret is kept" }
    'net stop zapret-steam', 'sc.exe delete zapret-steam', 'sc.exe stop windivert', 'sc.exe delete windivert' | ForEach-Object {
      $command = $_
      (cmd.exe /c "$command 2>&1") | ForEach-Object { "  ${command}: $_" } | Log
    }
    $zapretRemoved = -not (Get-Service zapret-steam -ErrorAction SilentlyContinue)
    Log "zapret-steam present=$(-not $zapretRemoved), windivert present=$([bool](Get-Service windivert -ErrorAction SilentlyContinue)), winws=$(Winws)"
    Say 'Done. Reboot now, then run this script with -AfterReboot.'
  }
  catch {
    Log "error: $_"
    if (-not $zapretRemoved) {
      if (Service) { Uninstall-Service }
      if ((RulesToRelay).Count -gt 0) { Remove-RelayRules }
      Start-Service zapret-steam -ErrorAction SilentlyContinue
      Log "rolled back: zapret $((Get-Service zapret-steam -ErrorAction SilentlyContinue).Status), winws=$(Winws), snisplit service present=$([bool](Service)), snisplit rules=$((Rules).Count)"
    }
    Say "Something failed; see $log. $zapretBundleNote"
  }
  Log $zapretBundleNote
}

if ($AfterReboot) {
  $boot = (Get-CimInstance Win32_OperatingSystem).LastBootUpTime
  Log "booted $($boot.ToString('s')); service: $((Service).Status), pid $(ServicePid); zapret present=$([bool](Get-Service zapret-steam -ErrorAction SilentlyContinue)), winws=$(Winws)"
  Log "relaying: $(Wait-Until { Relayed } 30) ms"
  $dns = @(Get-DnsClientServerAddress -AddressFamily IPv4 | Where-Object ServerAddresses | ForEach-Object ServerAddresses | Where-Object { $_ -notlike '127.*' })[0]
  $direct = ((Resolve-DnsName example.org -Type A -Server $dns -DnsOnly -ErrorAction SilentlyContinue | Where-Object IPAddress | ForEach-Object IPAddress | Sort-Object) -join ',')
  Log "example.org through the system: $(Resolve 'example.org'); asked directly from $dns`: $direct"
  Log-Checks 'after reboot'
  Log-ServiceTail 30
  $before = if (Test-Path $serviceLog) { @(Get-Content $serviceLog).Count } else { 0 }
  Say "NOW: if Steam did not log in on its own, exit it fully and start it again (say which in the chat). Then start a game download. Watching $DownloadObserveSeconds s."
  $ports = @{}
  $end = (Get-Date).AddSeconds($DownloadObserveSeconds)
  while ((Get-Date) -lt $end) {
    $steamProcs = @(Get-Process steam -ErrorAction SilentlyContinue | ForEach-Object Id)
    Get-NetTCPConnection -State Established -ErrorAction SilentlyContinue | Where-Object { $steamProcs -contains [int]$_.OwningProcess } | ForEach-Object { $ports["$($_.RemoteAddress):$($_.RemotePort)"] = 1 }
    Start-Sleep 2
  }
  Log "steam.exe remote endpoints seen: $(($ports.Keys | Sort-Object) -join ' ')"
  Log-Unknown-Address
  Log 'restarting the service once, so open connections close and report their bytes'
  Restart-Service snisplit
  Log "after the restart: relaying after $(Wait-Until { Relayed } 60) ms; snisplit rules=$((Rules).Count)"
  $window = @(Get-Content $serviceLog | Select-Object -Skip $before | ForEach-Object { Text-After-Time $_ })
  Log 'connections that closed during the window, bytes received per host and port:'
  Bytes-By-Host $window | Log
  Log "failed or refused lines in the window: $(@($window | Where-Object { $_ -match '^(failed|refused) ' }).Count)"
  $window | Where-Object { $_ -match '^(failed|refused) ' } | Select-Object -First 10 | ForEach-Object { "  $_" } | Log
  Log $zapretBundleNote
}

Log "example.org at the end: $(Resolve 'example.org') (start: $exampleBefore)"
Say "Done. Log: $log. You can close this window."
Log "finished $(Get-Date -Format s)"
