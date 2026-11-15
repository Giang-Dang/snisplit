param(
  [string] $OutDir = $env:TEMP
)
$ErrorActionPreference = 'Continue'
$built = Join-Path $PSScriptRoot '..\..\target\release\snisplit-gui.exe'
$log = Join-Path $OutDir 'm9b-gui.log'
$appFolder = Join-Path $env:APPDATA 'snisplit'
$sites = Join-Path $appFolder 'sites'
$offList = Join-Path $appFolder 'off.list'
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
function Addresses($name) { @(Resolve-DnsName $name -Type A -DnsOnly -ErrorAction SilentlyContinue | Where-Object IPAddress | ForEach-Object IPAddress | Sort-Object) }
function Resolve($name) { (Addresses $name) -join ',' }
function Goes-To-Relay($name) { $found = Addresses $name; ($found.Count -gt 0) -and (@($found | Where-Object { $_ -notlike '127.0.0.*' }).Count -eq 0) }
function Fetch([string] $url) { (curl.exe -s -o NUL -w '%{http_code} ip=%{remote_ip}' --max-time 20 $url 2>&1) -join ' ' }
function Status-Code([string] $url) { (curl.exe -s -o NUL -w '%{http_code}' --max-time 20 $url 2>&1) -join ' ' }
function Is-Elevated { ([Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator) }
function Snisplit-Processes { @(Get-Process snisplit* -ErrorAction SilentlyContinue) }
function Site-File-Count { @(Get-ChildItem $sites -Filter *.profile -ErrorAction SilentlyContinue).Count }
function Off-List-Names-Medium { if (Test-Path $offList) { @(Get-Content $offList | Where-Object { $_.Trim() -eq 'medium' }).Count -gt 0 } else { $false } }
function Log-State($step) {
  Clear-DnsClientCache
  Log "$step | processes: $((Snisplit-Processes).Name -join ',') | snisplit rules: $((Rules).Count), rules pointing at the relay: $((RulesToRelay).Count) | site files: $(Site-File-Count) | off.list names medium: $(Off-List-Names-Medium)"
  Log "  store.steampowered.com -> $(Resolve 'store.steampowered.com'); https: $(Fetch 'https://store.steampowered.com/')"
  Log "  medium.com -> $(Resolve 'medium.com')"
  Log "  example.org -> $(Resolve 'example.org')"
}
function Log-Added($when) {
  if (-not $script:addedDomain) { Log "  the added site: no address given"; return }
  Clear-DnsClientCache
  Log "  the added site $when goes to the relay: $(Goes-To-Relay $script:addedDomain); https status: $(Status-Code "https://$script:addedDomain/")"
}

"started $(Get-Date -Format s)" | Add-Content $log
Log "snisplit-gui binary present=$(Test-Path $built); elevated=$(Is-Elevated); snisplit rules: $((Rules).Count); site files: $(Site-File-Count)"
if (-not (Is-Elevated)) { Say 'Run this script from an elevated PowerShell (Run as administrator). Nothing was changed.'; exit 1 }
if ((Snisplit-Processes).Count -gt 0) { Say "A snisplit process is running ($((Snisplit-Processes).Name -join ', ')). Close it, then run this script again. Nothing was changed."; exit 1 }
if (-not (Test-Path $built)) { Say "$built is missing (not built, or removed by the antivirus?). Build it with: cargo build --release -p snisplit-gui. Nothing was changed."; exit 1 }
if (Off-List-Names-Medium) { Say "Medium is switched off in the window from an earlier run. Start snisplit-gui, switch Medium on, close it, then run this script again. Nothing was changed."; exit 1 }
if ((RulesToRelay).Count -gt 0) { Log 'stale rules pointing at the relay found; removing them first'; Remove-RelayRules }
Log-State 'before'

$gui = Start-Process $built -PassThru
Start-Sleep 3
Log "window: process $($gui.Id), title '$((Get-Process -Id $gui.Id -ErrorAction SilentlyContinue).MainWindowTitle)'"
$null = Ask '0. Does the window show a round Start button, Steam and Medium rows with switches, and Add a site? (y/n)'

Wait-Owner '1. Press the round Start button and wait until the line under it says On.'
$null = Ask '   Does the line under the button say On, with a sentence that sites open through snisplit? (y/n)'
Log-State 'step 1, after Start'
Log "  medium.com goes to the relay: $(Goes-To-Relay 'medium.com')"

Wait-Owner '2. Nothing to click: the script now fetches https://store.steampowered.com/ and the http page through snisplit.'
Log "  curl store https: $(Fetch 'https://store.steampowered.com/'); store http: $(Fetch 'http://store.steampowered.com/')"

Wait-Owner '3. Click the Medium switch to turn it Off, and wait until the line under the button says On again.'
$null = Ask '   While it changed, did you see Applying your change... and Turning off beside the switch? (y/n)'
Log-State 'step 3, Medium switched off while on'
Log "  medium.com goes to the relay: $(Goes-To-Relay 'medium.com') (expected False); store goes to the relay: $(Goes-To-Relay 'store.steampowered.com') (expected True)"

Wait-Owner '4. Click the Medium switch again to turn it On, and wait for On.'
Log-State 'step 4, Medium switched on again'
Log "  medium.com goes to the relay: $(Goes-To-Relay 'medium.com') (expected True)"

Say '5. Add a site while snisplit is on: click Add a site, type a blocked address that is not a Steam or medium.com name, press Test, and if the window offers it press Add and turn on. Wait for On.'
Write-Host 'Type the same address here so the script can check it. It is not written to the log.'
$script:addedDomain = (Read-Host 'Address').Trim()
Wait-Owner '   When the new row is in the list and the line under the button says On again, continue.'
$null = Ask '   What did the add panel say after the test? Answer with a number: 1 blocked and snisplit can open it, 2 already opens, 3 snisplit cannot open it, 4 could not test'
Log-State 'step 5, after adding a site while on'
Log-Added 'after adding'

Wait-Owner '6. Remove that site while on: click its name, press Remove, then Remove again in the question that appears. Wait for On.'
Log-State 'step 6, after removing the site while on'
Log-Added 'after removing'

Wait-Owner '7. Press the round Stop button and wait until the line under it says Off.'
Log-State 'step 7, after Stop'

Wait-Owner '8. Press Start again and wait until it says On.'
Log-State 'step 8, on again before the kill'

Wait-Owner '9. Nothing to click: the script now ends snisplit-gui abruptly (as Task Manager would) while it is on, and starts it again.'
Stop-Process -Id $gui.Id -Force
$killed = $gui.WaitForExit(20000)
Start-Sleep 2
Log "step 9: killed while on: exited=$killed; snisplit rules: $((Rules).Count) (expected above 0)"
$gui = Start-Process $built -PassThru
Start-Sleep 5
Log "window again: process $($gui.Id), title '$((Get-Process -Id $gui.Id -ErrorAction SilentlyContinue).MainWindowTitle)'"
$null = Ask '   Does the window say Off, with leftovers, that snisplit did not get to turn off properly last time, and offer Remove leftover settings? (y/n)'

Wait-Owner '10. Press Remove leftover settings and wait until the line under the button says Off.'
Log-State 'step 10, after removing the leftovers'
Log "  snisplit rules: $((Rules).Count) (expected 0)"

Say '11. With this window open, the script starts a second snisplit-gui.'
$second = Start-Process $built -PassThru
Start-Sleep 4
$null = Ask '   Does the second window say Already open, with a Try again button? (y/n)'
Wait-Owner '   Close the second window (the one that says Already open) with its X button.'
$secondClosed = $second.WaitForExit(20000)
Log "step 11: second window closed=$secondClosed, exit code $(if ($secondClosed) { $second.ExitCode }); processes: $((Snisplit-Processes).Name -join ',')"

Wait-Owner '12. In the remaining window press Start and wait until it says On.'
Log-State 'step 12, on again before the close'
Say '13. Close the window with its X button while it is on. The window should say Turning off... and close by itself. The script waits up to 120 s.'
$closedInTime = $gui.WaitForExit(120000)
Start-Sleep 2
Log "step 13: closed while on: exited=$closedInTime, exit code $(if ($closedInTime) { $gui.ExitCode })"
Log-State 'step 13, after closing'
if (-not $closedInTime) { Log 'snisplit-gui did not exit within 120 s; it was left running for you to look at' }
if ($closedInTime -and (RulesToRelay).Count -gt 0) { Log 'RULES LEFT after closing the window; removing them'; Remove-RelayRules; Log "rules now: $((RulesToRelay).Count)" }
$null = Ask '   Did Kaspersky or Windows show anything during this run? (no, or what it said)'

Say "Done. Log: $log. You can close this window."
Log "finished $(Get-Date -Format s)"
