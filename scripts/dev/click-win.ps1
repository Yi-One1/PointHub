param(
  [string]$TitleContains = "积分枢纽",
  [int]$DX,
  [int]$DY
)
Add-Type @"
using System;
using System.Text;
using System.Runtime.InteropServices;
public class WinAt {
  public delegate bool EnumProc(IntPtr h, IntPtr l);
  [DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc cb, IntPtr l);
  [DllImport("user32.dll")] public static extern int GetWindowText(IntPtr h, StringBuilder s, int n);
  [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
  [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
  [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
  [DllImport("user32.dll")] public static extern void mouse_event(uint f, uint dx, uint dy, uint d, IntPtr e);
  [StructLayout(LayoutKind.Sequential)] public struct RECT { public int L, T, R, B; }
  public static IntPtr Found = IntPtr.Zero;
  public static string Needle = "";
  public static void Find() {
    EnumWindows((h, l) => {
      if (!IsWindowVisible(h)) return true;
      var sb = new StringBuilder(256); GetWindowText(h, sb, 256);
      if (sb.ToString().Contains(Needle)) { Found = h; return false; }
      return true;
    }, IntPtr.Zero);
  }
}
"@
[WinAt]::Needle = $TitleContains
[WinAt]::Find()
$h = [WinAt]::Found
if ($h -eq [IntPtr]::Zero) { Write-Output "NOT FOUND"; exit 1 }
$r = New-Object WinAt+RECT
[WinAt]::GetWindowRect($h, [ref]$r) | Out-Null
$w = $r.R - $r.L; $ht = $r.B - $r.T
Write-Output ("rect=" + $r.L + "," + $r.T + " size=" + $w + "x" + $ht)
[WinAt]::SetForegroundWindow($h) | Out-Null
Start-Sleep -Milliseconds 200
$x = $r.L + $DX; $y = $r.T + $DY
[WinAt]::SetCursorPos($x, $y) | Out-Null
Start-Sleep -Milliseconds 120
[WinAt]::mouse_event(0x0002, 0, 0, 0, [IntPtr]::Zero)
Start-Sleep -Milliseconds 60
[WinAt]::mouse_event(0x0004, 0, 0, 0, [IntPtr]::Zero)
Write-Output ("clicked " + $x + "," + $y)
