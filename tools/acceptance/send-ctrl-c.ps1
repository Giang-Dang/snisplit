param(
  [Parameter(Mandatory)] [uint32] $ProcessId,
  [ValidateSet(0, 1)] [uint32] $ControlEvent = 0
)
if (-not ('SnisplitAcceptance.CtrlSender' -as [type])) {
  Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
namespace SnisplitAcceptance {
  public static class CtrlSender {
    public delegate bool Handler(uint controlType);
    private static readonly Handler swallow = SwallowEvent;
    private static bool SwallowEvent(uint controlType) { return true; }
    [DllImport("kernel32.dll", SetLastError = true)] private static extern bool FreeConsole();
    [DllImport("kernel32.dll", SetLastError = true)] private static extern bool AttachConsole(uint processId);
    [DllImport("kernel32.dll", SetLastError = true)] private static extern bool SetConsoleCtrlHandler(Handler handler, bool add);
    [DllImport("kernel32.dll", SetLastError = true)] private static extern bool GenerateConsoleCtrlEvent(uint controlEvent, uint processGroup);
    public static int Send(uint processId, uint controlEvent) {
      FreeConsole();
      if (!AttachConsole(processId)) { return 2; }
      if (!SetConsoleCtrlHandler(swallow, true)) { return 4; }
      if (!GenerateConsoleCtrlEvent(controlEvent, 0)) { return 3; }
      return 0;
    }
  }
}
'@
}
$result = [SnisplitAcceptance.CtrlSender]::Send($ProcessId, $ControlEvent)
Start-Sleep -Seconds 2
exit $result
