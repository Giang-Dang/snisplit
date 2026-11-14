param(
  [string] $OutDir = $env:TEMP
)
$ErrorActionPreference = 'Continue'
$installDir = Join-Path $env:ProgramFiles 'snisplit'
$log = Join-Path $OutDir 'm5-restore.log'

function Log($line) { Add-Content -Path $log -Value "$line"; Write-Host "$line" }
function Rules { @(Get-DnsClientNrptRule | Where-Object Comment -eq 'snisplit') }
function RulesToRelay { @(Get-DnsClientNrptRule | Where-Object { $ns = $_.NameServers; @('127.0.0.2', '127.0.0.3', '127.0.0.4' | Where-Object { $ns -contains $_ }).Count -gt 0 }) }
function State {
  $service = Get-CimInstance Win32_Service -Filter "Name='snisplit'" -ErrorAction SilentlyContinue
  "snisplit service: $(if ($service) { "$($service.State), pid $($service.ProcessId)" } else { 'not installed' }); folder present=$(Test-Path $installDir); snisplit rules=$((Rules).Count); rules pointing at the relay=$((RulesToRelay).Count); zapret: $((Get-Service zapret-steam -ErrorAction SilentlyContinue).Status), winws=$([bool](Get-Process winws -ErrorAction SilentlyContinue))"
}

"started $(Get-Date -Format s)" | Set-Content $log
Log "before: $(State)"
if (Get-Service snisplit -ErrorAction SilentlyContinue) {
  sc.exe stop snisplit | Out-Null
  $deadline = (Get-Date).AddSeconds(35)
  while ((Get-Service snisplit).Status -ne 'Stopped' -and (Get-Date) -lt $deadline) { Start-Sleep -Milliseconds 500 }
  Log "snisplit service status after stop: $((Get-Service snisplit).Status)"
  Log "sc delete: $((sc.exe delete snisplit) -join ' ')"
}
$saved = Join-Path $OutDir "m5-restore-$(Get-Date -Format 'HHmmss')-snisplit.log"
if (Test-Path (Join-Path $installDir 'logs\snisplit.log')) { Copy-Item (Join-Path $installDir 'logs\snisplit.log') $saved; Log "saved the service log as $saved" }
@(Rules) + @(RulesToRelay) | Sort-Object Name -Unique | ForEach-Object { Remove-DnsClientNrptRule -Name $_.Name -Force }
Clear-DnsClientCache
if ([IO.Path]::IsPathFullyQualified($installDir) -and (Test-Path $installDir)) { Remove-Item -LiteralPath $installDir -Recurse -Force -ErrorAction Continue }
$zapret = Get-Service zapret-steam -ErrorAction SilentlyContinue
if ($zapret -and $zapret.Status -ne 'Running') { Start-Service zapret-steam }
Start-Sleep 2
Log "after: $(State)"
Log "store.steampowered.com -> $(((Resolve-DnsName store.steampowered.com -Type A -DnsOnly -ErrorAction SilentlyContinue | Where-Object IPAddress).IPAddress) -join ',')"
Write-Host "`n>>> Done. Log: $log. You can close this window.`n" -ForegroundColor Yellow
