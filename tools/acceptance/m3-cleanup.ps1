param(
  [string] $OutDir = $env:TEMP,
  [int] $BrowseSeconds = 10
)
$ErrorActionPreference = 'Continue'
$exe = (Resolve-Path (Join-Path $PSScriptRoot '..\..\target\release\snisplit.exe')).Path
$log = Join-Path $OutDir 'm3-cleanup.log'

function Log {
  param([Parameter(ValueFromPipeline)] $Line)
  process { "$Line" | Tee-Object -FilePath $log -Append }
}
function Say($text) { Write-Host "`n>>> $text`n" -ForegroundColor Yellow; Log ">>> $text" }
function RulesToRelay { @(Get-DnsClientNrptRule | Where-Object { $_.NameServers -contains '127.0.0.2' -or $_.NameServers -contains '127.0.0.3' }) }
function Resolve($name) { ((Resolve-DnsName $name -Type A -DnsOnly -ErrorAction SilentlyContinue | Where-Object IPAddress | ForEach-Object IPAddress) -join ',') }

function Start-Medium($name) {
  $out = Join-Path $OutDir "m3-cleanup-$name.out"
  $err = Join-Path $OutDir "m3-cleanup-$name.err"
  $p = Start-Process $exe -ArgumentList 'run', '--profile', 'medium', '--verbose' -RedirectStandardOutput $out -RedirectStandardError $err -PassThru -WindowStyle Hidden
  $t0 = Get-Date
  $ready = $false
  while (-not $ready -and -not $p.HasExited -and ((Get-Date) - $t0).TotalSeconds -lt 90) {
    $ready = [bool](Select-String $out -Pattern 'Ctrl\+C to stop' -Quiet -ErrorAction SilentlyContinue)
    if (-not $ready) { Start-Sleep -Milliseconds 200 }
  }
  Log "$name ready=$ready after $([int]((Get-Date) - $t0).TotalMilliseconds) ms; rules pointing at the relay: $((RulesToRelay).Count); medium.com -> $(Resolve 'medium.com')" | Out-Host
  @{ Process = $p; Out = $out; Err = $err }
}

function Stop-Hard($run, $name) {
  Stop-Process -Id $run.Process.Id -Force
  $run.Process.WaitForExit(10000) | Out-Null
  Log "$name killed; rules pointing at the relay: $((RulesToRelay).Count); medium.com -> $(Resolve 'medium.com')"
  Get-Content $run.Out | ForEach-Object { "  $name out: $_" } | Log
  Get-Content $run.Err | ForEach-Object { "  $name err: $_" } | Log
}

"started $(Get-Date -Format s)" | Set-Content $log
Log "zapret: $((Get-Service zapret-steam).Status); rules pointing at the relay before: $((RulesToRelay).Count)"
try {
  $first = Start-Medium 'first'
  Say "Open https://medium.com/ in your Chrome now and check that the page renders. Waiting $BrowseSeconds s."
  Start-Sleep $BrowseSeconds
  Stop-Hard $first 'first'
  $second = Start-Medium 'second'
  Stop-Hard $second 'second'
  Log '--- snisplit cleanup'
  & $exe cleanup 2>&1 | ForEach-Object { "  $_" } | Log
  Log "cleanup exit code: $LASTEXITCODE"
}
catch { Log "error: $_" }
finally {
  Log "after: rules pointing at the relay: $((RulesToRelay).Count); medium.com -> $(Resolve 'medium.com')"
  if ((RulesToRelay).Count -gt 0) {
    Log 'rules remain; removing by name server'
    RulesToRelay | ForEach-Object { Remove-DnsClientNrptRule -Name $_.Name -Force }
    Clear-DnsClientCache
    Log "rules pointing at the relay now: $((RulesToRelay).Count)"
  }
}
Say 'Done. You can close this window.'
Log "finished $(Get-Date -Format s)"
