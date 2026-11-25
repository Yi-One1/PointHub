param(
  [string]$ProcessName = "pointhub",
  [int]$MinWidth = 40,
  [int]$MaxWidth = 200,
  [int]$OffsetX = 0,
  [int]$OffsetY = 0
)

# Click the center of the matching window (used to expand the floating ball).
# ASCII only on purpose (PowerShell 5.1 + zh-CN ANSI parsing).

Add-Type @"
using System;
using System.Collections.Generic;
using System.Runtime.InteropServices;
public class WinClick {
  public delegate bool EnumProc(IntPtr h, IntPtr l);
  [DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc cb, IntPtr l);
  [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
  [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
  [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
  [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
  [DllImport("user32.dll")] public static extern void mouse_event(uint f, uint dx, uint dy, uint d, IntPtr e);
  [StructLayout(LayoutKind.Sequential)]
  public struct RECT { public int Left; public int Top; public int Right; public int Bottom; }

  public static List<IntPtr> Windows(uint target) {
    var o = new List<IntPtr>();
    EnumWindows(delegate(IntPtr h, IntPtr l) {
      uint pid; GetWindowThreadProcessId(h, out pid);
      if (pid == target && IsWindowVisible(h)) o.Add(h);
      return true;
    }, IntPtr.Zero);
    return o;
  }

  public static int[] Rect(IntPtr h) {
    RECT r;
    GetWindowRect(h, out r);
    return new int[] { r.Left, r.Top, r.Right - r.Left, r.Bottom - r.Top };
  }

  public static void Click(int x, int y) {
    SetCursorPos(x, y);
    System.Threading.Thread.Sleep(120);
    mouse_event(0x0002, 0, 0, 0, IntPtr.Zero);
    System.Threading.Thread.Sleep(60);
    mouse_event(0x0004, 0, 0, 0, IntPtr.Zero);
  }
}
"@

$procs = Get-Process -Name $ProcessName -ErrorAction SilentlyContinue
if (-not $procs) { Write-Output "NO PROCESS"; exit 1 }

$hit = $null
foreach ($p in $procs) {
  foreach ($h in [WinClick]::Windows([uint32]$p.Id)) {
    $r = [WinClick]::Rect($h)
    if ($r[2] -lt $MinWidth -or $r[2] -gt $MaxWidth) { continue }
    $hit = @{ H = $h; R = $r }
    break
  }
  if ($hit) { break }
}

if (-not $hit) { Write-Output "NOTFOUND"; exit 1 }

$r = $hit.R
$x = $r[0] + [int]($r[2] / 2) + $OffsetX
$y = $r[1] + [int]($r[3] / 2) + $OffsetY
[WinClick]::SetForegroundWindow($hit.H) | Out-Null
Start-Sleep -Milliseconds 200
[WinClick]::Click($x, $y)
Write-Output ("CLICK " + $x + "," + $y + " on " + $r[2] + "x" + $r[3])
