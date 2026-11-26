# PLATFORM: Windows only (Win32 SuspendThread through P/Invoke).
#   macOS: thread_suspend on the task port. Linux: SIGSTOP to the thread id (tgkill).
#
# Fault injection for tools\gui-test.ps1: freezes the "crucible feeder" thread of a running
# program again and again, so the playback queue runs dry and dropouts keep coming. This is how
# the window is tested against the worst case without needing a slow computer.
#
#   pwsh -File tools\freeze-thread.ps1 <process name> <seconds> <freeze ms> <every ms>
param([string]$Name = 'crucible-app', [int]$Seconds = 10, [int]$Freeze = 70, [int]$Every = 250)
Add-Type @'
using System;
using System.Runtime.InteropServices;
public static class Th2 {
  [DllImport("kernel32.dll")] public static extern IntPtr OpenThread(uint access, bool inherit, uint id);
  [DllImport("kernel32.dll")] public static extern uint SuspendThread(IntPtr h);
  [DllImport("kernel32.dll")] public static extern uint ResumeThread(IntPtr h);
  [DllImport("kernel32.dll")] public static extern bool CloseHandle(IntPtr h);
  [DllImport("kernel32.dll")] public static extern int GetThreadDescription(IntPtr h, out IntPtr text);
  public static string Name(IntPtr h) { IntPtr p; if (GetThreadDescription(h, out p) < 0 || p == IntPtr.Zero) return ""; return Marshal.PtrToStringUni(p); }
}
'@
$p = Get-Process -Name $Name -ErrorAction SilentlyContinue | Select-Object -First 1
if (-not $p) { 'no process'; exit 1 }
$feeder = [IntPtr]::Zero
foreach ($t in $p.Threads) {
  $h = [Th2]::OpenThread(0x0002 -bor 0x0800 -bor 0x0040, $false, [uint32]$t.Id)
  if ($h -eq [IntPtr]::Zero) { continue }
  if ([Th2]::Name($h) -eq 'crucible feeder') { $feeder = $h; break }
  [void][Th2]::CloseHandle($h)
}
if ($feeder -eq [IntPtr]::Zero) { 'no feeder thread'; exit 2 }
$end = (Get-Date).AddSeconds($Seconds); $n = 0
while ((Get-Date) -lt $end) {
  [void][Th2]::SuspendThread($feeder); Start-Sleep -Milliseconds $Freeze; [void][Th2]::ResumeThread($feeder)
  $n++; Start-Sleep -Milliseconds ($Every - $Freeze)
}
[void][Th2]::CloseHandle($feeder)
"froze the feeder $n times"
