param(
  [string] $OutDir = $env:TEMP,
  [string] $Version = '0.2.0',
  [string] $Msi = ''
)
$ErrorActionPreference = 'Continue'
. (Join-Path $PSScriptRoot 'lib.ps1')
Warn-Do-Not-Close
if (Refuse-OutDir-In-Repo $OutDir) { exit 1 }
Open-Log $OutDir 'm7b-installer.log' 'm7b-installer-output'
if (-not $Msi) { $Msi = Join-Path $PSScriptRoot "..\..\target\installer\bin\Release\snisplit-$Version-windows-x86_64.msi" }

$installFolder = Join-Path $env:ProgramFiles 'snisplit'
$installedCli = Join-Path $installFolder 'snisplit.exe'
$installedGui = Join-Path $installFolder 'snisplit-gui.exe'
$machineFolder = Join-Path $env:ProgramData 'snisplit'
$shortcut = Join-Path $env:ProgramData 'Microsoft\Windows\Start Menu\Programs\snisplit.lnk'
$expectedFiles = @('LICENSE-APACHE', 'LICENSE-MIT', 'README.md', 'snisplit-gui.exe', 'snisplit.exe')

function Machine-Path-Entries { @([Environment]::GetEnvironmentVariable('Path', 'Machine') -split ';' | Where-Object { $_ } | ForEach-Object { $_.TrimEnd('\') }) }
function Path-Has-Install-Folder { (Machine-Path-Entries) -contains $installFolder }
function Uninstall-Entries { @(Get-ItemProperty 'HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\*' -ErrorAction SilentlyContinue | Where-Object DisplayName -eq 'snisplit') }
function Same-Content($left, $right) { (Get-FileHash -LiteralPath $left -Algorithm SHA256).Hash -eq (Get-FileHash -LiteralPath $right -Algorithm SHA256).Hash }
function App-Data-Fingerprint {
  if (-not (Test-Path -LiteralPath $script:appFolder)) { return 'absent' }
  @(Get-ChildItem -LiteralPath $script:appFolder -Recurse -File | Sort-Object FullName | ForEach-Object { "$($_.FullName)=$((Get-FileHash -LiteralPath $_.FullName -Algorithm SHA256).Hash)" }) -join ';'
}
function Invoke-Msiexec([string[]] $arguments, $logName) {
  $logPath = Join-Path $script:outputs $logName
  $process = Start-Process -FilePath 'msiexec.exe' -ArgumentList (@($arguments) + @('/qn', '/norestart', '/l*v', "`"$logPath`"")) -PassThru -Wait
  Log "  msiexec $($arguments -join ' '): exit code $($process.ExitCode), log $logPath"
  $process.ExitCode
}

$Msi = Resolve-Full $Msi
Log "m7b-installer: unattended; installs $Msi, starts the installed snisplit run, kills it, uninstalls. elevated=$(Is-Elevated); snisplit rules: $(Rule-Count); rules pointing at the relay: $((RulesToRelay).Count)"
if (-not (Refuse-Unless-Ready @($Msi, $script:cliBinary, $script:guiBinary))) { Log "finished $(Get-Date -Format s), refused"; exit 1 }
if ((Test-Path -LiteralPath $installFolder) -or (Uninstall-Entries).Count -gt 0) { Say "snisplit is already installed ($installFolder or an Apps entry). Uninstall it from Settings, Apps, then run this script again. Nothing was changed."; exit 1 }
if ((RulesToRelay).Count -gt 0 -or (Rule-Count) -gt 0) { Log 'stale rules from an earlier run found; removing them first'; Remove-RelayRules }
$appDataBefore = App-Data-Fingerprint
Log-State 'before'

$completed = $false
$exitCode = 1
$installed = $false
try {
  Say 'a. install silently'
  $installCode = Invoke-Msiexec @('/i', "`"$Msi`"") 'a-install.log'
  $installed = $installCode -eq 0
  Expect 'a. msiexec /i exits with code 0' $installed
  $present = @(Get-ChildItem -LiteralPath $installFolder -File -ErrorAction SilentlyContinue | ForEach-Object Name)
  Log "  installed files: $($present -join ', ')"
  Expect "a. $installFolder holds exactly $($expectedFiles -join ', ')" ($present.Count -eq $expectedFiles.Count -and @(Compare-Object $present $expectedFiles).Count -eq 0)
  Expect 'a. the installed snisplit.exe is the built target\release\snisplit.exe' ((Test-Path -LiteralPath $installedCli) -and (Same-Content $installedCli $script:cliBinary))
  Expect 'a. the installed snisplit-gui.exe is the built target\release\snisplit-gui.exe' ((Test-Path -LiteralPath $installedGui) -and (Same-Content $installedGui $script:guiBinary))
  Expect "a. the machine PATH lists $installFolder" (Path-Has-Install-Folder)
  Expect "a. the all-users Start menu has $shortcut" (Test-Path -LiteralPath $shortcut)
  $entries = Uninstall-Entries
  Log "  Apps entries: $(@($entries | ForEach-Object { "$($_.DisplayName) $($_.DisplayVersion) by $($_.Publisher)" }) -join '; ')"
  Expect "a. exactly one Apps entry named snisplit, version $Version" ($entries.Count -eq 1 -and $entries[0].DisplayVersion -eq $Version)

  Say 'b. the installed snisplit run, then a kill that leaves its rules behind'
  $run = Start-Tracked $installedCli @('run') 'Minimized' 'b-run'
  Expect 'b. the installed run reports it is on within 90 s' (Wait-Run-On $run 90)
  $null = Wait-Rules-Settled 30
  $base = Rule-Count
  Log "  rule count while on: $base"
  Expect 'b. the installed run added at least 2 rules (Steam and Medium)' ($base -ge 2)
  Kill-If-Running $run
  $null = $run.WaitForExit(20000)
  Expect 'b. the killed run has exited' $run.HasExited
  Expect "b. the rules are still in place after the kill ($base)" ((Rule-Count) -eq $base)
  Start-Sleep -Seconds 10
  Expect "b. the rules are still in place 10 s after the kill ($base)" ((Rule-Count) -eq $base)

  Say 'c. uninstall silently'
  $uninstallCode = Invoke-Msiexec @('/x', "`"$Msi`"") 'c-uninstall.log'
  $installed = $uninstallCode -ne 0
  Expect 'c. msiexec /x exits with code 0' ($uninstallCode -eq 0)
  $actionLines = @(Get-Content -LiteralPath (Join-Path $script:outputs 'c-uninstall.log') -ErrorAction SilentlyContinue | Where-Object { $_ -match 'RemoveLeftoverRules|WixQuietExec' })
  $actionLines | ForEach-Object { Log "  uninstall log: $_" }
  Expect 'c. the uninstall log names the RemoveLeftoverRules action' ($actionLines.Count -gt 0)
  $null = Wait-Rules 0 30
  Expect 'c. 0 snisplit rules after the uninstall' ((Rule-Count) -eq 0)
  Expect 'c. 0 rules pointing at the relay after the uninstall' ((RulesToRelay).Count -eq 0)
  Expect "c. $installFolder is gone" (-not (Test-Path -LiteralPath $installFolder))
  Expect "c. $machineFolder is gone" (-not (Test-Path -LiteralPath $machineFolder))
  Expect "c. $shortcut is gone" (-not (Test-Path -LiteralPath $shortcut))
  Expect "c. the machine PATH no longer lists $installFolder" (-not (Path-Has-Install-Folder))
  Expect 'c. no Apps entry named snisplit' ((Uninstall-Entries).Count -eq 0)
  Expect "c. $script:appFolder is byte for byte as before the install" ((App-Data-Fingerprint) -eq $appDataBefore)

  Log-State 'after'
  $completed = $true
}
finally {
  if ($installed) { Log '  the package is still installed; uninstalling it before ending'; $null = Invoke-Msiexec @('/x', "`"$Msi`"") 'z-uninstall.log' }
  $exitCode = Finish-Run $completed
}
exit $exitCode
