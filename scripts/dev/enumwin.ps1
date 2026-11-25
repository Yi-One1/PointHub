param([string]$ProcessName = "pointhub")

# List top-level windows of a process, with visibility and title.
# ASCII only on purpose (PowerShell 5.1 + zh-CN ANSI parsing).

Add-Type @"
using System;
using System.Text;
using System.Collections.Generic;
using System.Runtime.InteropServices;
public class WinEnum {
  public delegate bool EnumProc(IntPtr h, IntPtr l);
  [DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc cb, IntPtr l);
  [DllImport("user32.dll")] public static extern int GetWindowText(IntPtr h, StringBuilder s, int n);
  [DllImport("user32.dll")] public static extern int GetClassName(IntPtr h, StringBuilder s, int n);
  [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
  [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
  public static List<string> List(uint target) {
    var o = new List<string>();
    EnumWindows(delegate(IntPtr h, IntPtr l) {
      uint pid;
      GetWindowThreadProcessId(h, out pid);
      if (pid == target) {
        var t = new StringBuilder(256);
        GetWindowText(h, t, 256);
        var c = new StringBuilder(256);
        GetClassName(h, c, 256);
        o.Add(h.ToInt64().ToString() + "|" + IsWindowVisible(h).ToString() + "|" + c.ToString() + "|" + t.ToString());
      }
      return true;
    }, IntPtr.Zero);
    return o;
  }
}
"@

$procs = Get-Process -Name $ProcessName -ErrorAction SilentlyContinue
if (-not $procs) { Write-Output "NO PROCESS"; exit 1 }
foreach ($p in $procs) {
  Write-Output ("PID " + $p.Id + " MainWindowHandle=" + $p.MainWindowHandle)
  [WinEnum]::List([uint32]$p.Id) | ForEach-Object { Write-Output ("  " + $_) }
}
