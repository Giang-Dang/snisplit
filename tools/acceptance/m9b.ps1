param(
  [string] $OutDir = $env:TEMP
)
$ErrorActionPreference = 'Continue'
. (Join-Path $PSScriptRoot 'lib.ps1')
Open-Log $OutDir 'm9b-gui.log' 'm9b-shots'
$medium = 'medium.com'
$store = 'store.steampowered.com'
$stepSeconds = 180
$clickCount = 14
$script:gui = $null

function Wait-Click($text, [scriptblock] $done) {
  Say $text
  $watch = [Diagnostics.Stopwatch]::StartNew()
  $seen = Wait-Until { $script:gui.HasExited -or (& $done) } $stepSeconds
  if ($script:gui.HasExited) { throw 'the snisplit window closed before the step was done' }
  if ($seen) { Log "  seen after $([int]$watch.Elapsed.TotalSeconds) s" } else { Log "  TIMEOUT: not seen within $stepSeconds s" }
  $seen
}
function Wait-On($text) {
  $seen = Wait-Click $text { (Rule-Count) -ge 2 }
  if ($seen) { $null = Wait-Rules-Settled 30 }
  $seen
}
function Domain-Of($file) {
  try {
    $line = @(Get-Content -LiteralPath $file.FullName -ErrorAction Stop | Where-Object { $_ -match '^\s*domains\s' }) | Select-Object -First 1
    if ($line) { ($line.Trim() -split '\s+')[1] } else { $null }
  } catch { $null }
}
function Relaunch($name) {
  $script:gui = Start-Window
  Expect "$name`: the window appears within 20 s" (Wait-Window $script:gui 20)
}

Say "m9b needs about $clickCount clicks in the snisplit window and one address typed in that window. Do not press Enter here: the script watches for the result of each click and waits up to $stepSeconds s. At the very end there is one question about the antivirus."
Log "binaries: $script:releaseFolder; elevated=$(Is-Elevated); snisplit rules: $(Rule-Count); site files: $((Site-Files).Count)"
if (-not (Refuse-Unless-Ready @($script:guiBinary))) { Log "finished $(Get-Date -Format s), refused"; exit 1 }
if ((RulesToRelay).Count -gt 0 -or (Rule-Count) -gt 0) { Log 'stale rules from an earlier run found; removing them first'; Remove-RelayRules }
Log-State 'before'
Save-OffList -RestoreAtEnd

$completed = $false
$addedPath = $null
$exitCode = 1
try {
  Relaunch '0. launch'
  Start-Sleep -Seconds 3
  $null = Save-Shot $script:gui '00-launch'

  Expect '1. Start: on' (Wait-On '1. Click the round Start button.')
  $base = Rule-Count
  Log "  base rule count: $base"
  Expect '1. at least 2 rules (Steam and Medium)' ($base -ge 2)
  Start-Sleep -Seconds 2
  $null = Save-Shot $script:gui '01-on'
  Log-State '1. on'
  Expect "1. $store resolves to the relay" (Goes-To-Relay $store)
  Expect "1. $medium resolves to the relay" (Goes-To-Relay $medium)
  $https = Fetch "https://$store/"
  $http = Fetch "http://$store/"
  Log "  https: $https; http: $http"
  Expect '1. HTTPS to the store answers 200 through 127.0.0.x' ($https -match '^200 ip=127\.0\.0\.')
  Expect '1. HTTP to the store answers 302 through 127.0.0.x' ($http -match '^302 ip=127\.0\.0\.')

  Expect '2. Medium off: off.list names medium and one rule fewer' (Wait-Click '2. Click the Medium switch to turn it Off.' { (Off-List-Names 'medium') -and ((Rule-Count) -eq ($base - 1)) })
  Start-Sleep -Seconds 2
  $null = Save-Shot $script:gui '02-medium-off'
  Expect "2. $medium does not resolve to the relay" (-not (Goes-To-Relay $medium))
  Expect "2. $store still resolves to the relay" (Goes-To-Relay $store)

  Expect '3. Medium on: off.list without medium and the base count' (Wait-Click '3. Click the Medium switch again to turn it On.' { (-not (Off-List-Names 'medium')) -and ((Rule-Count) -eq $base) })
  Expect "3. $medium resolves to the relay" (Goes-To-Relay $medium)

  $filesBefore = @(Site-Files | ForEach-Object FullName)
  $script:newSiteFiles = @()
  $added = Wait-Click '4. Click Add a site, type in the window a blocked address that is not a Steam or medium.com name, press Test, then press Add and turn on.' {
    $script:newSiteFiles = @(Site-Files | Where-Object { $filesBefore -notcontains $_.FullName })
    ($script:newSiteFiles.Count -gt 0) -and ((Rule-Count) -gt $base)
  }
  Expect '4. a new site file and one more rule' $added
  $addedFile = $null
  if ($script:newSiteFiles.Count -gt 0) { $addedFile = $script:newSiteFiles[0] }
  if ($addedFile) {
    $null = Wait-Rules-Settled 20
    Log "  rule count with the added site: $(Rule-Count)"
    $addedDomain = Domain-Of $addedFile
    Expect '4. the added site file names a domain' ([bool]$addedDomain)
    if ($addedDomain) {
      Expect '4. the added site resolves to the relay' (Goes-To-Relay $addedDomain)
      Log "  the added site: https status $(Status-Code "https://$addedDomain/")"
    }
    $addedPath = $addedFile.FullName
    Expect '5. removed: the site file is gone and the base count is back' (Wait-Click '5. Click the name of the site you added, press Remove, then press Remove again in the question.' { (-not (Test-Path -LiteralPath $addedPath)) -and ((Rule-Count) -eq $base) })
    if ($addedDomain) { Expect '5. the added site no longer resolves to the relay' (-not (Goes-To-Relay $addedDomain)) }
    $addedDomain = $null
  } else {
    Log '  no new site file was seen, so step 5 (remove it) is skipped'
  }
  Expect '5. site file count is back to the count before step 4' ((Site-Files).Count -eq $filesBefore.Count)

  Expect '6. Stop: 0 rules' (Wait-Click '6. Click the round Stop button.' { (Rule-Count) -eq 0 })
  Expect '6. 0 rules pointing at the relay' ((RulesToRelay).Count -eq 0)

  Expect '7. Start again: on' (Wait-On '7. Click Start again.')
  Expect "7. the base count ($base)" ((Rule-Count) -eq $base)
  Say 'Nothing to click: the script now ends the window abruptly (as Task Manager would) and starts it again.'
  if (-not $script:gui.HasExited) { $script:gui.Kill() }
  Expect '7. the killed window ended' ($script:gui.WaitForExit(20000))
  Start-Sleep -Seconds 2
  Expect "7. the rules are still in place after the kill ($base)" ((Rule-Count) -eq $base)
  Relaunch '7. relaunch after the kill'
  Start-Sleep -Seconds 6
  $null = Save-Shot $script:gui '07-leftovers-1'
  Start-Sleep -Seconds 4
  $null = Save-Shot $script:gui '07-leftovers-2'

  Expect '8. leftovers removed: 0 rules' (Wait-Click '8. Click Remove leftover settings.' { (Rule-Count) -eq 0 })
  Say 'Nothing to click: the script starts a second window, takes a picture of it and closes it.'
  $second = Start-Tracked $script:guiBinary $null 'Normal' $null
  Expect '8. the second window appears within 20 s' (Wait-Window $second 20)
  Start-Sleep -Seconds 4
  $null = Save-Shot $second '08-second-window'
  $closed = Close-Window $second 1 30
  Expect '8. the second window exits after one close' $closed.Exited
  Expect '8. the second window exits with code 0' ($closed.ExitCode -eq 0)
  Expect '8. the first window is still open' (-not $script:gui.HasExited)

  Expect '9. Start: on' (Wait-On '9. Click Start.')
  Say 'Nothing to click: the script closes the window once, as the X button would, while it is on.'
  $closed = Close-Window $script:gui 1 120
  Start-Sleep -Seconds 1
  Expect '9. closed once while on: the process exits within 120 s' $closed.Exited
  Expect '9. closed once while on: exit code 0' ($closed.ExitCode -eq 0)
  Expect '9. closed once while on: 0 rules left' (((Rule-Count) -eq 0) -and ((RulesToRelay).Count -eq 0))
  if ($closed.Exited -and (RulesToRelay).Count -gt 0) { Log 'RULES LEFT after the close; removing them'; Remove-RelayRules }

  Say 'Nothing to click: the script starts the window again.'
  Relaunch '10. relaunch'
  Expect '10. Start: on' (Wait-On '10. Click Start.')
  Say 'Nothing to click: the script closes the window twice in quick succession while it is on.'
  $closed = Close-Window $script:gui 2 120
  Start-Sleep -Seconds 1
  Expect '10. closed twice while on: the process exits within 120 s' $closed.Exited
  Expect '10. closed twice while on: exit code 0' ($closed.ExitCode -eq 0)
  Expect '10. closed twice while on: no poll saw the window not responding' ($closed.NotRespondingPolls -eq 0)
  Expect '10. closed twice while on: 0 rules left' (((Rule-Count) -eq 0) -and ((RulesToRelay).Count -eq 0))

  Log-State 'after'
  Log "screenshots for the lead: $script:shots"
  $null = Ask 'Last question: did Kaspersky or Windows show anything during this run? (no, or what it said)'
  $completed = $true
}
finally {
  if ($addedPath -and (Test-Path -LiteralPath $addedPath)) { Log 'the site added in step 4 is still in the site folder: remove it in the window' }
  $exitCode = Finish-Run $completed
}
exit $exitCode
