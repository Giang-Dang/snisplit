param(
  [Parameter(Mandatory)] [string] $HostName,
  [int] $Rounds = 2,
  [int] $GapMs = 30,
  [string] $Only,
  [int] $TimeoutMs = 8000
)
$ErrorActionPreference = 'Stop'

$h = $HostName
$ua = 'User-Agent: snisplit-probe/0.1'
$crlf = "`r`n"
function Req($lines, $eol = $crlf, $prefix = '') { $prefix + (($lines + @('', '')) -join $eol) }
function DomCase($s) { -join (0..($s.Length - 1) | ForEach-Object { if ($_ % 2 -eq 0) { "$($s[$_])".ToUpper() } else { "$($s[$_])".ToLower() } }) }
function Pad($n) { 'X-Pad: ' + ('a' * $n) }

$plain = @("HEAD / HTTP/1.1", "Host: $h", $ua, 'Connection: close')
$variants = [ordered]@{
  'control'          = @{ req = Req $plain }
  'hostcase'         = @{ req = Req @("HEAD / HTTP/1.1", "host: $h", $ua, 'Connection: close') }
  'hostspell=hoSt'   = @{ req = Req @("HEAD / HTTP/1.1", "hoSt: $h", $ua, 'Connection: close') }
  'hostdot'          = @{ req = Req @("HEAD / HTTP/1.1", "Host: $h.", $ua, 'Connection: close') }
  'hosttab'          = @{ req = Req @("HEAD / HTTP/1.1", "Host: $h`t", $ua, 'Connection: close') }
  'hostnospace'      = @{ req = Req @("HEAD / HTTP/1.1", "Host:$h", $ua, 'Connection: close') }
  'domcase'          = @{ req = Req @("HEAD / HTTP/1.1", "Host: $(DomCase $h)", $ua, 'Connection: close') }
  'methodspace'      = @{ req = Req @("HEAD  / HTTP/1.1", "Host: $h", $ua, 'Connection: close') }
  'unixeol'          = @{ req = Req $plain "`n" }
  'methodeol'        = @{ req = Req $plain $crlf $crlf }
  'split method+2'   = @{ req = Req $plain; cuts = @(2) }
  'split midsld'     = @{ req = Req $plain; cuts = @('midsld') }
  'split m+2,midsld' = @{ req = Req $plain; cuts = @(2, 'midsld') }
  'hostcase+split'   = @{ req = Req @("HEAD / HTTP/1.1", "host: $h", $ua, 'Connection: close'); cuts = @(2, 'midsld') }
  'hostpad 1024'     = @{ req = Req @("HEAD / HTTP/1.1", (Pad 1024), "Host: $h", $ua, 'Connection: close') }
  'hostpad 2048'     = @{ req = Req @("HEAD / HTTP/1.1", (Pad 2048), "Host: $h", $ua, 'Connection: close') }
  'hostpad 4096'     = @{ req = Req @("HEAD / HTTP/1.1", (Pad 4096), "Host: $h", $ua, 'Connection: close') }
  'hostpad 8192'     = @{ req = Req @("HEAD / HTTP/1.1", (Pad 8192), "Host: $h", $ua, 'Connection: close') }
}

function Resolve-Cuts($bytes, $cuts) {
  $text = [Text.Encoding]::ASCII.GetString($bytes)
  foreach ($c in $cuts) {
    if ($c -eq 'midsld') {
      $at = $text.IndexOf($h, [StringComparison]::OrdinalIgnoreCase)
      $sld = $h.Split('.')[-2]
      $sldAt = $text.IndexOf($sld, $at, [StringComparison]::OrdinalIgnoreCase)
      $sldAt + [int][Math]::Floor($sld.Length / 2)
    } else { [int]$c }
  }
}

function Send-Variant($v) {
  $bytes = [Text.Encoding]::ASCII.GetBytes($v.req)
  $cuts = @(if ($v.cuts) { Resolve-Cuts $bytes $v.cuts }) + $bytes.Length
  $tcp = [System.Net.Sockets.TcpClient]::new()
  $tcp.NoDelay = $true
  $tcp.ReceiveTimeout = $TimeoutMs
  try {
    $tcp.Connect($h, 80)
    $s = $tcp.GetStream()
    $start = 0; $sizes = @()
    foreach ($cut in $cuts) {
      if ($cut -le $start) { continue }
      $s.Write($bytes, $start, $cut - $start); $s.Flush()
      $sizes += $cut - $start; $start = $cut
      if ($start -lt $bytes.Length -and $GapMs -gt 0) { Start-Sleep -Milliseconds $GapMs }
    }
    $line = [IO.StreamReader]::new($s).ReadLine()
    $p = $sizes -join '+'
    if ($line -match '^HTTP/1\.[01] \d{3}') { "PASS pieces=$p status='$line'" } else { "FAIL pieces=$p got='$line'" }
  } catch {
    $e = $_.Exception; while ($e.InnerException) { $e = $e.InnerException }
    "FAIL $($e.GetType().Name): $($e.Message)"
  } finally { $tcp.Close() }
}

"http-variants host=$h gap=${GapMs}ms rounds=$Rounds at $(Get-Date -Format s)"
foreach ($round in 1..$Rounds) {
  foreach ($name in $variants.Keys) {
    if ($Only -and $name -notlike $Only) { continue }
    "r{0} {1,-18} {2}" -f $round, $name, (Send-Variant $variants[$name])
  }
}
