param(
  [int] $WaitForRestartSeconds = 600,
  [int] $ObserveSeconds = 120,
  [string] $OutDir = $env:TEMP,
  [switch] $WithDummyNrptRule
)
$dummySuffix = '.nrpt-probe.example'
$ErrorActionPreference = 'Continue'
$log = Join-Path $OutDir 'steam-dns-check.log'
$dnsLog = 'Microsoft-Windows-DNS-Client/Operational'
$names = @{}

function Log {
  param([Parameter(ValueFromPipeline)] $Line)
  process { "$Line" | Tee-Object -FilePath $log -Append }
}
function Remember-Processes { Get-Process | ForEach-Object { $names[$_.Id] = $_.Name } }

"started $(Get-Date -Format s)" | Set-Content $log
$wasEnabled = (Get-WinEvent -ListLog $dnsLog).IsEnabled
wevtutil sl $dnsLog /e:true
$start = Get-Date
try {
  if ($WithDummyNrptRule) {
    Add-DnsClientNrptRule -Namespace $dummySuffix -NameServers 127.0.0.2 -Comment 'snisplit spike'
    Log "dummy NRPT rule for $dummySuffix added"
  }
  $old = @(Get-Process steam -ErrorAction SilentlyContinue | ForEach-Object Id)
  Log "steam.exe running at start: $($old.Count)"
  $deadline = $start.AddSeconds($WaitForRestartSeconds)
  $fresh = $null
  while (-not $fresh -and (Get-Date) -lt $deadline) {
    Remember-Processes
    $fresh = Get-Process steam -ErrorAction SilentlyContinue | Where-Object { $old -notcontains $_.Id -and $_.StartTime -gt $start } | Select-Object -First 1
    Start-Sleep 1
  }
  if (-not $fresh) { Log 'no Steam restart seen; reading what was logged anyway' }
  else {
    Log "new steam.exe pid $($fresh.Id) started $($fresh.StartTime.ToString('s')); observing $ObserveSeconds s"
    $end = (Get-Date).AddSeconds($ObserveSeconds)
    while ((Get-Date) -lt $end) { Remember-Processes; Start-Sleep 1 }
  }
  Remember-Processes
  $events = Get-WinEvent -LogName $dnsLog -ErrorAction Stop | Where-Object { $_.TimeCreated -ge $start -and $_.Id -eq 3006 }
  Log "query events since start: $(@($events).Count)"
  Log '--- lookups of Steam names, by process ---'
  $events | Where-Object { "$($_.Properties[0].Value)" -match 'steam|valve' } | ForEach-Object {
    $n = if ($names.ContainsKey($_.ProcessId)) { $names[$_.ProcessId] } else { "pid $($_.ProcessId)" }
    "{0} {1}" -f $n, $_.Properties[0].Value
  } | Group-Object | Sort-Object Count -Descending | ForEach-Object { "{0,4} {1}" -f $_.Count, $_.Name } | Log
  $helperPids = @(Get-Process steamwebhelper -ErrorAction SilentlyContinue | ForEach-Object Id)
  $steamNames = @($events | Where-Object { $helperPids -contains $_.ProcessId -and "$($_.Properties[0].Value)" -match 'steampowered|steamstatic|steamcommunity|steamcontent' })
  $doh = @(Resolve-DnsName chrome.cloudflare-dns.com -Type A -ErrorAction SilentlyContinue | Where-Object IPAddress | ForEach-Object IPAddress)
  $conns = @(Get-NetTCPConnection -State Established -ErrorAction SilentlyContinue | Where-Object { $helperPids -contains [int]$_.OwningProcess -and $_.RemotePort -eq 443 })
  Log "steamwebhelper system lookups of Steam web names: $($steamNames.Count)"
  Log "steamwebhelper connections on 443: $($conns.Count); to the DoH endpoint ($($doh -join ',')): $(@($conns | Where-Object { $doh -contains $_.RemoteAddress }).Count)"
  Log "verdict: $(if ($steamNames.Count -gt 0) { 'PASS steamwebhelper uses the system resolver' } else { 'FAIL steamwebhelper did not use the system resolver' })"
  Log '--- all processes that made lookups ---'
  $events | ForEach-Object { if ($names.ContainsKey($_.ProcessId)) { $names[$_.ProcessId] } else { "pid $($_.ProcessId)" } } | Group-Object | Sort-Object Count -Descending | ForEach-Object { "{0,5} {1}" -f $_.Count, $_.Name } | Log
}
catch { Log "error: $_" }
finally {
  if ($WithDummyNrptRule) {
    Get-DnsClientNrptRule | Where-Object Namespace -eq $dummySuffix | ForEach-Object { Remove-DnsClientNrptRule -Name $_.Name -Force }
    Log "dummy NRPT rules left: $((Get-DnsClientNrptRule | Where-Object Namespace -eq $dummySuffix | Measure-Object).Count)"
  }
  if (-not $wasEnabled) { wevtutil sl $dnsLog /e:false }
  Log "dns log enabled after: $((Get-WinEvent -ListLog $dnsLog).IsEnabled)"
  Log "finished $(Get-Date -Format s)"
}
