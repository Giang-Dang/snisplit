param(
  [string] $OutDir = $env:TEMP
)
$ErrorActionPreference = 'Continue'
$built = Join-Path $PSScriptRoot '..\..\target\release\snisplit-gui.exe'
$log = Join-Path $OutDir 'm9-gui.log'
$sites = Join-Path $env:APPDATA 'snisplit\sites'
$relayAddresses = 2..20 | ForEach-Object { "127.0.0.$_" }

function Log {
  param([Parameter(ValueFromPipeline)] $Line)
  process { Add-Content -Path $log -Value "$Line"; Write-Host "$Line" }
}
function Say($text) { Write-Host "`n>>> $text`n" -ForegroundColor Yellow; Add-Content -Path $log -Value ">>> $text" }
function Ask($question) { $answer = Read-Host $question; Add-Content -Path $log -Value "  ${question}: $answer"; $answer }
function Wait-Owner($text) { Say $text; $null = Read-Host 'Press Enter when done' }
function Rules { @(Get-DnsClientNrptRule | Where-Object Comment -eq 'snisplit') }
function RulesToRelay { @(Get-DnsClientNrptRule | Where-Object { $ns = $_.NameServers; @($relayAddresses | Where-Object { $ns -contains $_ }).Count -gt 0 }) }
function Remove-RelayRules { @(Rules) + @(RulesToRelay) | Sort-Object Name -Unique | ForEach-Object { Remove-DnsClientNrptRule -Name $_.Name -Force }; Clear-DnsClientCache }
function Resolve($name) { ((Resolve-DnsName $name -Type A -DnsOnly -ErrorAction SilentlyContinue | Where-Object IPAddress | ForEach-Object IPAddress | Sort-Object) -join ',') }
function Curl([string] $url) { (curl.exe -s -o NUL -w '%{http_code} ip=%{remote_ip}' --max-time 20 $url 2>&1) -join ' ' }
function Is-Elevated { ([Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator) }
function Snisplit-Processes { @(Get-Process snisplit* -ErrorAction SilentlyContinue) }
function Site-Files { @(Get-ChildItem $sites -Filter *.profile -ErrorAction SilentlyContinue | ForEach-Object Name) -join ', ' }
function Log-State($step) {
  Clear-DnsClientCache
  Log "$step | processes: $((Snisplit-Processes).Name -join ',') | snisplit rules: $((Rules).Count), rules pointing at the relay: $((RulesToRelay).Count) | site files: $(Site-Files)"
  Log "  store.steampowered.com -> $(Resolve 'store.steampowered.com'); https: $(Curl 'https://store.steampowered.com/')"
  Log "  example.org -> $(Resolve 'example.org')"
}

"started $(Get-Date -Format s)" | Add-Content $log
Log "snisplit-gui binary present=$(Test-Path $built); elevated=$(Is-Elevated); snisplit rules: $((Rules).Count); site files: $(Site-Files)"
if (-not (Is-Elevated)) { Say 'Run this script from an elevated PowerShell (Run as administrator). Nothing was changed.'; exit 1 }
if (-not (Test-Path $built)) { Say "$built is missing (not built, or removed by the antivirus?). Build it with: cargo build --release -p snisplit-gui. Nothing was changed."; exit 1 }
if ((Snisplit-Processes).Count -gt 0) { Say "A snisplit process is running ($((Snisplit-Processes).Name -join ', ')). Stop it, then run this script again. Nothing was changed."; exit 1 }
if ((RulesToRelay).Count -gt 0) { Log 'stale rules pointing at the relay found; removing them first'; Remove-RelayRules }
Log-State 'before'

$gui = Start-Process $built -PassThru
Start-Sleep 3
Log "window: process $($gui.Id), title '$((Get-Process -Id $gui.Id -ErrorAction SilentlyContinue).MainWindowTitle)'"
$null = Ask '0. Does the window show a Start button, the steam and medium sites and an Add a site field? (y/n)'

Wait-Owner '1. Press Start in the window and wait until the status says Running.'
$null = Ask '   What does the status line say?'
Log-State 'step 1, after Start'

Wait-Owner '2. Nothing to click: the script now fetches https://store.steampowered.com/ through 127.0.0.2.'
Log "  curl store https: $(Curl 'https://store.steampowered.com/'); store http: $(Curl 'http://store.steampowered.com/')"
Log-State 'step 2'

Say '3. Add a site: type a blocked domain that is not a Steam or medium.com name into the Add a site field, press Check, wait for the table, then press Add if it is offered.'
$domain = Ask '   Which domain did you type?'
Wait-Owner '   When the log says the site was added and the status says Running again, continue.'
$null = Ask '   What did the window say under the table (the Add button, Not blocked, or No method worked)?'
Log-State 'step 3, after adding a site'
if ($domain) {
  Log "  $domain -> $(Resolve $domain); https: $(Curl "https://$domain/")"
}

Wait-Owner '4. Press Stop and wait until the status says Stopped.'
Log-State 'step 4, after Stop'
if ($domain) { Log "  $domain -> $(Resolve $domain)" }

Wait-Owner '5. Press Start again and wait until the status says Running. Then close the window with its X button while it runs.'
$closedInTime = $gui.WaitForExit(120000)
Start-Sleep 2
Log "step 5: closed while running: exited=$closedInTime, exit code $(if ($closedInTime) { $gui.ExitCode })"
Log-State 'step 5, after closing'
if (-not $closedInTime) { Log 'snisplit-gui did not exit within 120 s; it was left running for you to look at' }
if ($closedInTime -and (RulesToRelay).Count -gt 0) { Log 'RULES LEFT after closing the window; removing them'; Remove-RelayRules; Log "rules now: $((RulesToRelay).Count)" }

Say "Done. Log: $log. The site you added stays in $sites; remove it in the window or delete the file. You can close this window."
Log "finished $(Get-Date -Format s)"
