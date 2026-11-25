param(
  [string]$TitleContains = "",
  [string]$Out = "shot-top.png"
)

# Bring the target window to HWND_TOPMOST, capture its screen region, then
# restore. WebView2 renders through DirectComposition, so PrintWindow returns
# a blank surface; a real screen grab is the only faithful capture.
# ASCII only on purpose (PowerShell 5.1 + zh-CN ANSI parsing).

Add-Type -AssemblyName System.Drawing

Add-Type @"
using System;
using System.Text;
using System.Collections.Generic;
using System.Drawing;
using System.Runtime.InteropServices;
public class WinTop {
  public delegate bool EnumProc(IntPtr h, IntPtr l);
  [DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc cb, IntPtr l);
  [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
  [DllImport("user32.dll")] public static extern int GetWindowText(IntPtr h, StringBuilder s, int n);
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
  [DllImport("user32.dll")] public static extern bool SetWindowPos(IntPtr h, IntPtr after, int x, int y, int cx, int cy, uint flags);
  [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
  [StructLayout(LayoutKind.Sequential)]
  public struct RECT { public int Left; public int Top; public int Right; public int Bottom; }

  public static IntPtr Best = IntPtr.Zero;
  public static string Needle = "";

  public static void Find() {
    EnumWindows(delegate(IntPtr h, IntPtr l) {
      if (!IsWindowVisible(h)) return true;
      var sb = new StringBuilder(512);
      GetWindowText(h, sb, 512);
      if (sb.ToString().IndexOf(Needle, StringComparison.Ordinal) < 0) return true;
      Best = h;
      return false;
    }, IntPtr.Zero);
  }

  public static int[] RectOf(IntPtr h) {
    RECT r; GetWindowRect(h, out r);
    return new int[] { r.Left, r.Top, r.Right - r.Left, r.Bottom - r.Top };
  }
}
"@ -ReferencedAssemblies "System.Drawing"

[WinTop]::Needle = $TitleContains
[WinTop]::Find()
$h = [WinTop]::Best
if ($h -eq [IntPtr]::Zero) { Write-Output "NOTFOUND"; exit 1 }

$HWND_TOPMOST = [IntPtr](-1)
$HWND_NOTOPMOST = [IntPtr](-2)
$SWP = 0x0001 -bor 0x0002 -bor 0x0040   # NOSIZE | NOMOVE | SHOWWINDOW

[WinTop]::SetWindowPos($h, $HWND_TOPMOST, 0, 0, 0, 0, $SWP) | Out-Null
[WinTop]::SetForegroundWindow($h) | Out-Null
Start-Sleep -Milliseconds 1200

$r = [WinTop]::RectOf($h)
$w = $r[2]; $ht = $r[3]
$bmp = New-Object System.Drawing.Bitmap($w, $ht)
$g = [System.Drawing.Graphics]::FromImage($bmp)
$g.CopyFromScreen($r[0], $r[1], 0, 0, $bmp.Size)
$bmp.Save($Out, [System.Drawing.Imaging.ImageFormat]::Png)
$g.Dispose(); $bmp.Dispose()

[WinTop]::SetWindowPos($h, $HWND_NOTOPMOST, 0, 0, 0, 0, $SWP) | Out-Null
Write-Output ("OK " + $w + "x" + $ht + " -> " + $Out)
