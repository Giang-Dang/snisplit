param(
  [string] $OutDir = $env:TEMP,
  [int] $SteamObserveSeconds = 150,
  [int] $WaitForSteamSeconds = 300,
  [int] $ChromeWaitSeconds = 90
)
$ErrorActionPreference = 'Continue'
$exe = (Resolve-Path (Join-Path $PSScriptRoot '..\..\target\release\snisplit.exe')).Path
$log = Join-Path $OutDir 'm3-acceptance.log'
$dnsLog = 'Microsoft-Windows-DNS-Client/Operational'
$steamUrls = 'https://store.steampowered.com/',
  'https://steamcommunity.com/',
  'https://api.steampowered.com/ISteamDirectory/GetCMListForConnect/v1/?cellid=0&maxcount=3',
  'https://login.steampowered.com/'

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
function Resolve($name) { ((Resolve-DnsName $name -Type A -DnsOnly -ErrorAction SilentlyContinue | Where-Object IPAddress | ForEach-Object IPAddress) -join ',') }
function Curl($url) { (curl.exe -s -o NUL -w '%{http_code} ip=%{remote_ip} t=%{time_total}' --max-time 20 $url 2>&1) -join ' ' }

function Start-Snisplit($name, [string[]] $runArgs) {
  $out = Join-Path $OutDir "m3-$name.out"
  $err = Join-Path $OutDir "m3-$name.err"
  $p = Start-Process $exe -ArgumentList $runArgs -NoNewWindow -RedirectStandardOutput $out -RedirectStandardError $err -PassThru
  $deadline = (Get-Date).AddSeconds(20)
  while (-not (Select-String $out -Pattern 'Ctrl\+C to stop' -Quiet -ErrorAction SilentlyContinue) -and -not $p.HasExited -and (Get-Date) -lt $deadline) { Start-Sleep -Milliseconds 200 }
  Log "snisplit $($runArgs -join ' '): running=$(-not $p.HasExited)"
  @{ Process = $p; Out = $out; Err = $err }
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

function Wait-ForChrome($name) {
  $flag = Join-Path $OutDir "m3-chrome-$name.done"
  Remove-Item $flag -ErrorAction SilentlyContinue
  Log "chrome window $name open"
  $deadline = (Get-Date).AddSeconds($ChromeWaitSeconds)
  while (-not (Test-Path $flag) -and (Get-Date) -lt $deadline) { Start-Sleep 1 }
  Log "chrome window $name closed, flag seen=$(Test-Path $flag)"
}

"started $(Get-Date -Format s)" | Set-Content $log
Log "zapret before: $((Get-Service zapret-steam).Status), winws=$(Winws); snisplit rules before: $((Rules).Count)"

Say 'Part 1: medium.com with NRPT rules (zapret keeps running). No action needed.'
$run = Start-Snisplit 'medium' @('run', '--profile', 'medium', '--verbose')
try {
  Log "rules: $((Rules | ForEach-Object { "$($_.Namespace)->$($_.NameServers)" }) -join ' ')"
  Log "resolve medium.com: $(Resolve 'medium.com')"
  Log "resolve www.medium.com: $(Resolve 'www.medium.com')"
  Log "resolve example.org: $(Resolve 'example.org')"
  Log "curl https://medium.com/: $(Curl 'https://medium.com/')"
  Log "curl https://help.medium.com/: $(Curl 'https://help.medium.com/')"
  Wait-ForChrome 'medium'
}
finally { Stop-Snisplit $run }
Log "after stop: rules=$((Rules).Count) resolve medium.com: $(Resolve 'medium.com')"

Say 'Part 2: Steam with zapret stopped. When asked below, restart Steam and open the Store.'
$dnsLogWasEnabled = (Get-WinEvent -ListLog $dnsLog).IsEnabled
wevtutil sl $dnsLog /e:true
$run = $null
try {
  Stop-Service zapret-steam -Force
  $deadline = (Get-Date).AddSeconds(15)
  while ((Winws) -and (Get-Date) -lt $deadline) { Start-Sleep -Milliseconds 500 }
  Log "zapret stopped: $((Get-Service zapret-steam).Status), winws=$(Winws)"
  if (Winws) { throw 'winws still running; Steam part skipped' }
  Log "control without snisplit, store: $(Curl 'https://store.steampowered.com/')"
  $run = Start-Snisplit 'steam' @('run', '--verbose')
  $start = Get-Date
  Log "rules: $((Rules).Count)"
  Log "resolve store.steampowered.com: $(Resolve 'store.steampowered.com')"
  Log "resolve cmp1-hkg1.steamserver.net: $(Resolve 'cmp1-hkg1.steamserver.net')"
  foreach ($u in $steamUrls) { Log "curl $u : $(Curl $u)" }
  Log "curl http://store.steampowered.com/ (HTTP, expected blocked until milestone 4): $(Curl 'http://store.steampowered.com/')"
  Wait-ForChrome 'steam'
  $old = @(Get-Process steam -ErrorAction SilentlyContinue | ForEach-Object Id)
  Say "NOW: exit Steam fully (Steam menu > Exit), start it again, log in, open the Store and a game page. Waiting up to $WaitForSteamSeconds s for Steam to start, then watching $SteamObserveSeconds s."
  $deadline = (Get-Date).AddSeconds($WaitForSteamSeconds)
  $fresh = $null
  $names = @{}
  while (-not $fresh -and (Get-Date) -lt $deadline) {
    Get-Process | ForEach-Object { $names[$_.Id] = $_.Name }
    $fresh = Get-Process steam -ErrorAction SilentlyContinue | Where-Object { $old -notcontains $_.Id -and $_.StartTime -gt $start } | Select-Object -First 1
    Start-Sleep 1
  }
  if ($fresh) {
    Log "new steam.exe started $($fresh.StartTime.ToString('s'))"
    $end = (Get-Date).AddSeconds($SteamObserveSeconds)
    while ((Get-Date) -lt $end) { Get-Process | ForEach-Object { $names[$_.Id] = $_.Name }; Start-Sleep 1 }
  } else { Log 'no Steam restart seen' }
  $helper = @(Get-Process steamwebhelper -ErrorAction SilentlyContinue | ForEach-Object Id)
  $events = @(Get-WinEvent -LogName $dnsLog -ErrorAction SilentlyContinue | Where-Object { $_.TimeCreated -ge $start -and $_.Id -eq 3006 })
  $helperSteam = @($events | Where-Object { $helper -contains $_.ProcessId -and "$($_.Properties[0].Value)" -match 'steampowered|steamstatic|steamcommunity' })
  Log "steamwebhelper system lookups of Steam web names: $($helperSteam.Count)"
  $events | Where-Object { "$($_.Properties[0].Value)" -match 'steam|valve' } | ForEach-Object {
    $n = if ($names.ContainsKey($_.ProcessId)) { $names[$_.ProcessId] } else { "pid $($_.ProcessId)" }
    "{0} {1}" -f $n, $_.Properties[0].Value
  } | Group-Object | Sort-Object Count -Descending | Select-Object -First 25 | ForEach-Object { "{0,4} {1}" -f $_.Count, $_.Name } | Log
  $steamProcs = @(Get-Process steam, steamwebhelper -ErrorAction SilentlyContinue | ForEach-Object Id)
  $conns = @(Get-NetTCPConnection -State Established -ErrorAction SilentlyContinue | Where-Object { $steamProcs -contains [int]$_.OwningProcess })
  Log "steam established connections: $($conns.Count); to 127.0.0.2: $(@($conns | Where-Object RemoteAddress -eq '127.0.0.2').Count); ports to 127.0.0.2: $((@($conns | Where-Object RemoteAddress -eq '127.0.0.2' | ForEach-Object RemotePort | Sort-Object -Unique)) -join ',')"
}
catch { Log "error: $_" }
finally {
  if ($run) { Stop-Snisplit $run }
  if ((Rules).Count -gt 0) { & $exe cleanup | Log }
  Start-Service zapret-steam
  Start-Sleep 2
  Log "zapret restored: $((Get-Service zapret-steam).Status), winws=$(Winws); snisplit rules left: $((Rules).Count)"
  if (-not $dnsLogWasEnabled) { wevtutil sl $dnsLog /e:false }
  Log "after: resolve store.steampowered.com: $(Resolve 'store.steampowered.com'); curl store: $(Curl 'https://store.steampowered.com/')"
}
Say 'Done. You can close this window.'
Log "finished $(Get-Date -Format s)"
