param(
  [string] $OutDir = $env:TEMP
)
$ErrorActionPreference = 'Continue'
. (Join-Path $PSScriptRoot 'lib.ps1')
Warn-Do-Not-Close
if (Refuse-OutDir-In-Repo $OutDir) { exit 1 }
Open-Log $OutDir 'm9c.log' 'm9c-shots'
$medium = 'medium.com'
$store = 'store.steampowered.com'

Log "m9c: unattended; no clicks and no questions. Binaries: $script:releaseFolder; elevated=$(Is-Elevated); snisplit rules: $(Rule-Count); rules pointing at the relay: $((RulesToRelay).Count); site files: $((Site-Files).Count)"
if (-not (Refuse-Unless-Ready @($script:guiBinary, $script:cliBinary))) { Log "finished $(Get-Date -Format s), refused"; exit 1 }
if ((RulesToRelay).Count -gt 0 -or (Rule-Count) -gt 0) { Log 'stale rules from an earlier run found; removing them first'; Remove-RelayRules }
Log-State 'before'

$completed = $false
$exitCode = 1
try {
  Say 'b. snisplit run, then checks through it'
  $run = Start-Run 'b-run'
  Expect 'b. run reports it is on within 90 s' (Wait-Run-On $run 90)
  $null = Wait-Rules-Settled 30
  $base = Rule-Count
  Log "  base rule count: $base"
  Expect 'b. run added at least 2 rules (Steam and Medium)' ($base -ge 2)
  Log-State 'b. run on'
  Expect "b. $store resolves to the relay" (Goes-To-Relay $store)
  Expect "b. $medium resolves to the relay" (Goes-To-Relay $medium)
  $https = Fetch "https://$store/"
  $http = Fetch "http://$store/"
  Log "  https: $https; http: $http"
  Expect 'b. HTTPS to the store answers 200 through 127.0.0.x' ($https -match '^200 ip=127\.0\.0\.')
  Expect 'b. HTTP to the store answers 302 through 127.0.0.x' ($http -match '^302 ip=127\.0\.0\.')

  Say 'c. a second snisplit run while the first is on'
  $second = Invoke-Cli @('run') 'c-second-run' 15
  Log "  second run: exited $($second.Exited), exit code $($second.ExitCode), stderr: $($second.Err.Trim())"
  Expect 'c. the second run exits within 15 s' $second.Exited
  Expect 'c. the second run exits with code 1' ($second.ExitCode -eq 1)
  Expect 'c. the second run says already running on stderr' ($second.Err -match 'already running')
  Expect 'c. the first run is still running' (-not $run.HasExited)
  Expect "c. rule count unchanged ($base)" ((Rule-Count) -eq $base)

  Say 'd. snisplit-gui while run is on (expected: Already open)'
  $gui = Start-Window
  Expect 'd. the window appears within 20 s' (Wait-Window $gui 20)
  Start-Sleep -Seconds 4
  $null = Save-Shot $gui 'd-already-open'
  $closed = Close-Window $gui 1 30
  Expect 'd. the window exits after one close' $closed.Exited
  Expect 'd. the window exits with code 0' ($closed.ExitCode -eq 0)
  Expect 'd. no poll saw the window not responding' ($closed.NotRespondingPolls -eq 0)
  Expect 'd. run is still running' (-not $run.HasExited)
  Expect "d. rule count unchanged ($base)" ((Rule-Count) -eq $base)

  Say 'e. clean stop of run with Ctrl+C'
  $stop = Stop-Run $run
  Expect 'e. run stopped on Ctrl+C' ($stop.Method -eq 'Ctrl+C')
  Expect 'e. run exits with code 0' ($stop.ExitCode -eq 0)
  Expect 'e. run printed snisplit DNS rules left: 0' $stop.SaidRulesLeftZero
  Expect 'e. 0 snisplit rules' ((Rule-Count) -eq 0)
  Expect 'e. 0 rules pointing at the relay' ((RulesToRelay).Count -eq 0)

  Say 'f. off.list honoured by run'
  Save-OffList
  Add-OffListName 'medium'
  $run = Start-Run 'f-run'
  Expect 'f. run reports it is on within 90 s' (Wait-Run-On $run 90)
  $null = Wait-Rules-Settled 30
  Log "  rule count with medium off: $(Rule-Count)"
  Expect "f. one rule fewer than the base ($($base - 1))" ((Rule-Count) -eq ($base - 1))
  Expect "f. $medium does not resolve to the relay" (-not (Goes-To-Relay $medium))
  Expect "f. $store still resolves to the relay" (Goes-To-Relay $store)
  $stop = Stop-Run $run
  Expect 'f. run stopped on Ctrl+C' ($stop.Method -eq 'Ctrl+C')
  Expect 'f. run exits with code 0' ($stop.ExitCode -eq 0)
  Expect 'f. 0 snisplit rules' ((Rule-Count) -eq 0)
  Restore-OffList
  Expect 'f. off.list is exactly as before the run' (Off-List-Is-As-Saved)

  Say 'g. run killed while on, then the window shows the leftovers, then snisplit cleanup'
  $run = Start-Run 'g-run'
  Expect 'g. run reports it is on within 90 s' (Wait-Run-On $run 90)
  $null = Wait-Rules-Settled 30
  Kill-If-Running $run
  Log "  run ended after the kill: $($run.WaitForExit(20000))"
  Start-Sleep -Seconds 2
  Expect "g. the rules are still in place after the kill ($base)" ((Rule-Count) -eq $base)
  $gui = Start-Window
  Expect 'g. the window appears within 20 s' (Wait-Window $gui 20)
  Start-Sleep -Seconds 6
  $null = Save-Shot $gui 'g-leftovers-1'
  Start-Sleep -Seconds 4
  $null = Save-Shot $gui 'g-leftovers-2'
  $closed = Close-Window $gui 1 30
  Expect 'g. the window exits after one close' $closed.Exited
  Expect 'g. the window exits with code 0' ($closed.ExitCode -eq 0)
  Expect "g. the rules are still in place after the window ($base)" ((Rule-Count) -eq $base)
  $cleanup = Invoke-Cli @('cleanup') 'g-cleanup' 120
  Log "  cleanup: exited $($cleanup.Exited), exit code $($cleanup.ExitCode), stderr: $($cleanup.Err.Trim())"
  Expect 'g. snisplit cleanup exits with code 0' ($cleanup.ExitCode -eq 0)
  Expect 'g. 0 snisplit rules after cleanup' ((Rule-Count) -eq 0)
  Expect 'g. 0 rules pointing at the relay after cleanup' ((RulesToRelay).Count -eq 0)

  Log-State 'after'
  Log '  the screenshots and the captured output of snisplit can contain the names of your own sites and stay on this PC'
  Log "screenshots for the lead: $script:shots (d-already-open: Already open with Try again; g-leftovers: the leftovers sentence and Remove leftover settings)"
  $completed = $true
}
finally {
  $exitCode = Finish-Run $completed
}
exit $exitCode
