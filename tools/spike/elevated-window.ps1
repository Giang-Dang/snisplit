$ErrorActionPreference = 'Continue'
$here = $env:TEMP
$log = Join-Path $here 'elevated-window.log'
$etl = Join-Path $here 'window.etl'
$spike = $PSScriptRoot
$tls = Join-Path $spike 'tls-split-probe.ps1'
$http = Join-Path $spike 'http-variants.ps1'
$relay = Join-Path $spike 'loopback-relay.ps1'
$big = Join-Path $spike 'big-hello.py'
$steamHosts = 'store.steampowered.com', 'steamcommunity.com', 'api.steampowered.com', 'login.steampowered.com', 'cache5-hkg1.steamcontent.com'
$nrptSuffix = '.nrpt-probe.example'
$dnsLog = 'Microsoft-Windows-DNS-Client/Operational'

function Log {
  param([Parameter(ValueFromPipeline)] $Line)
  process { "$Line" | Tee-Object -FilePath $log -Append }
}
function Winws { [bool](Get-Process winws -ErrorAction SilentlyContinue) }

"started $(Get-Date -Format s)" | Set-Content $log
Log "zapret before: $((Get-Service zapret-steam).Status), winws=$(Winws)"
$dnsLogWasEnabled = (Get-WinEvent -ListLog $dnsLog).IsEnabled
wevtutil sl $dnsLog /e:true
$dnsLogStart = Get-Date

Log '=== part 1: Steam probes with zapret stopped ==='
Log 'pktmon filters before (removed for this run):'
pktmon filter list | Log
pktmon filter remove | Out-Null
pktmon filter add -p 443 | Out-Null
pktmon filter add -p 80 | Out-Null
pktmon start --capture --comp nics --pkt-size 200 --file-name $etl | Log
try {
  Stop-Service zapret-steam -Force
  $deadline = (Get-Date).AddSeconds(15)
  while ((Winws) -and (Get-Date) -lt $deadline) { Start-Sleep -Milliseconds 500 }
  Log "zapret stopped: $((Get-Service zapret-steam).Status), winws=$(Winws)"
  if (Winws) { throw 'winws still running; probes skipped' }
  foreach ($h in $steamHosts) { pwsh -NoProfile -File $tls -HostName $h -Rounds 2 -TimeoutMs 5000 | Log }
  pwsh -NoProfile -File $tls -HostName medium.com -Rounds 1 -TimeoutMs 5000 | Log
  foreach ($mode in 'single', 'split1') {
    $out = Join-Path $here "relay-steam-$mode.txt"
    $rp = Start-Process pwsh -ArgumentList '-NoProfile', '-File', $relay, '-Upstream', 'store.steampowered.com', '-Mode', $mode, '-Seconds', '20' -RedirectStandardOutput $out -NoNewWindow -PassThru
    Start-Sleep 3
    Log "large hello via relay, $mode"
    python -I $big store.steampowered.com 127.0.0.2 2 | Log
    $rp.WaitForExit()
    Get-Content $out | Log
  }
  pwsh -NoProfile -File $http -HostName store.steampowered.com -Rounds 1 -TimeoutMs 5000 | Log
  pwsh -NoProfile -File $http -HostName cache5-hkg1.steamcontent.com -Rounds 1 -TimeoutMs 5000 | Log
}
catch { Log "error: $_" }
finally {
  Start-Service zapret-steam
  Start-Sleep 2
  Log "zapret restored: $((Get-Service zapret-steam).Status), winws=$(Winws)"
  pktmon stop | Log
  pktmon filter remove | Out-Null
  pktmon etl2pcap $etl --out (Join-Path $here 'window.pcapng') | Log
}

Log '=== part 2: NRPT ==='
$listener = Start-Job -ArgumentList 75 {
  param($seconds)
  $u = New-Object System.Net.Sockets.UdpClient([System.Net.IPEndPoint]::new([System.Net.IPAddress]::Parse('127.0.0.2'), 53))
  $u.Client.ReceiveTimeout = 1000
  $end = (Get-Date).AddSeconds($seconds)
  while ((Get-Date) -lt $end) {
    $ep = New-Object System.Net.IPEndPoint([System.Net.IPAddress]::Any, 0)
    try {
      $q = $u.Receive([ref]$ep)
      $i = 12; $labels = @()
      while ($q[$i] -ne 0) { $labels += [Text.Encoding]::ASCII.GetString($q, $i + 1, $q[$i]); $i += $q[$i] + 1 }
      $qtype = $q[$i + 1] * 256 + $q[$i + 2]
      $r = [byte[]]($q[0..($i + 4)])
      $r[2] = 0x81; $r[3] = 0x80; $r[6] = 0; $r[7] = 0; $r[8] = 0; $r[9] = 0; $r[10] = 0; $r[11] = 0
      $answer = [byte[]]@(0xc0, 0x0c, 0, 1, 0, 1, 0, 0, 0, 5, 0, 4, 127, 0, 0, 2)
      if ($qtype -eq 1) { $r[7] = 1; $r = $r + $answer }
      $u.Send($r, $r.Length, $ep) | Out-Null
      "{0} query {1} type {2} from {3}" -f (Get-Date -Format 'HH:mm:ss'), ($labels -join '.'), $qtype, $ep
    } catch {}
  }
  $u.Close()
}
try {
  Add-DnsClientNrptRule -Namespace $nrptSuffix -NameServers 127.0.0.2 -Comment 'snisplit spike'
  Clear-DnsClientCache
  Start-Sleep 2
  Log "rules: $((Get-DnsClientNrptRule | Where-Object Namespace -eq $nrptSuffix | Measure-Object).Count)"
  Log "listed name: $((Resolve-DnsName a.nrpt-probe.example -Type A -DnsOnly -ErrorAction SilentlyContinue | Select-Object -ExpandProperty IPAddress) -join ',')"
  Log "other name: $((Resolve-DnsName example.org -Type A -DnsOnly -ErrorAction SilentlyContinue | Select-Object -ExpandProperty IPAddress) -join ',')"
  Log 'chrome window open for 45 s'
  Start-Sleep 45
  Receive-Job $listener -Wait | Log
  Clear-DnsClientCache
  $t = Measure-Command { $stale = Resolve-DnsName b.nrpt-probe.example -Type A -DnsOnly -ErrorAction SilentlyContinue }
  Log "stale rule, listener gone: answer='$($stale.IPAddress)' after $([int]$t.TotalMilliseconds) ms"
  Log "other name with stale rule: $((Resolve-DnsName example.org -Type A -DnsOnly -ErrorAction SilentlyContinue | Select-Object -ExpandProperty IPAddress) -join ',')"
}
catch { Log "error: $_" }
finally {
  Stop-Job $listener -ErrorAction SilentlyContinue; Remove-Job $listener -Force -ErrorAction SilentlyContinue
  Get-DnsClientNrptRule | Where-Object Namespace -eq $nrptSuffix | ForEach-Object { Remove-DnsClientNrptRule -Name $_.Name -Force }
  Clear-DnsClientCache
  Log "rules left: $((Get-DnsClientNrptRule | Where-Object Namespace -eq $nrptSuffix | Measure-Object).Count)"
}

Log '=== part 3: which processes asked the system resolver for Steam and medium names ==='
try {
  Get-WinEvent -LogName $dnsLog -ErrorAction Stop | Where-Object { $_.TimeCreated -ge $dnsLogStart -and $_.Id -eq 3006 } | ForEach-Object {
    $name = $_.Properties[0].Value
    if ($name -match 'steam|medium|nrpt-probe') {
      $p = Get-Process -Id $_.ProcessId -ErrorAction SilentlyContinue
      "{0} pid={1}" -f $name, $(if ($p) { $p.Name } else { $_.ProcessId })
    }
  } | Group-Object | Sort-Object Count -Descending | ForEach-Object { "{0,4} {1}" -f $_.Count, $_.Name } | Log
}
catch { Log "dns log read error: $_" }
finally { if (-not $dnsLogWasEnabled) { wevtutil sl $dnsLog /e:false } }
Log "finished $(Get-Date -Format s)"
