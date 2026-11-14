param(
  [string] $OutDir = $env:TEMP
)
$ErrorActionPreference = 'Continue'
$built = Join-Path $PSScriptRoot '..\..\target\release\snisplit.exe'
$log = Join-Path $OutDir 'remove-zapret.log'
$rollback = 'to bring zapret back: run steam_service_install.cmd from the zapret-win-bundle zapret-winws folder as administrator, after snisplit cleanup'

function Log($line) { Add-Content -Path $log -Value "$line"; Write-Host "$line" }
function State {
  "zapret-steam: $((Get-Service zapret-steam -ErrorAction SilentlyContinue).Status); windivert present=$([bool](Get-Service windivert -ErrorAction SilentlyContinue)); winws=$([bool](Get-Process winws -ErrorAction SilentlyContinue)); snisplit binary present=$(Test-Path $built); snisplit rules=$(@(Get-DnsClientNrptRule | Where-Object Comment -eq 'snisplit').Count)"
}

"started $(Get-Date -Format s)" | Set-Content $log
Log "before: $(State)"
if (-not (Test-Path $built)) {
  Log "$built is missing (removed by the antivirus?); zapret was not touched"
  exit 1
}
'net stop zapret-steam', 'sc.exe delete zapret-steam', 'sc.exe stop windivert', 'sc.exe delete windivert' | ForEach-Object {
  $command = $_
  cmd.exe /c "$command 2>&1" | ForEach-Object { Log "  ${command}: $_" }
}
Start-Sleep 2
Log "after: $(State)"
Log $rollback
Write-Host "`n>>> Done. Start snisplit from an administrator terminal with: $built run. Log: $log`n" -ForegroundColor Yellow
