param(
  [string]$TitleContains = "",
  [int]$MinWidth = 300,
  [int]$MaxWidth = 2000,
  [string]$Out = "shot-win.png"
)

# Capture a specific window by its own render surface (PrintWindow), so the
# result does not depend on z-order. Needed because WebView2 windows may not
# come to the foreground reliably, and CopyFromScreen would grab other apps.
# ASCII only on purpose (PowerShell 5.1 + zh-CN ANSI parsing).

Add-Type -AssemblyName System.Drawing

Add-Type @"
using System;
using System.Text;
using System.Collections.Generic;
using System.Drawing;
using System.Runtime.InteropServices;
public class WinShot {
  public delegate bool EnumProc(IntPtr h, IntPtr l);
  [DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc cb, IntPtr l);
  [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
  [DllImport("user32.dll")] public static extern int GetWindowText(IntPtr h, StringBuilder s, int n);
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
  [DllImport("user32.dll")] public static extern bool PrintWindow(IntPtr h, IntPtr hdc, uint flags);
  [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
  [StructLayout(LayoutKind.Sequential)]
  public struct RECT { public int Left; public int Top; public int Right; public int Bottom; }

  public static IntPtr Best = IntPtr.Zero;
  public static string Needle = "";
  public static int MinW = 0;
  public static int MaxW = 100000;

  public static void Find() {
    EnumWindows(delegate(IntPtr h, IntPtr l) {
      if (!IsWindowVisible(h)) return true;
      var sb = new StringBuilder(512);
      GetWindowText(h, sb, 512);
      string t = sb.ToString();
      if (Needle.Length > 0 && t.IndexOf(Needle, StringComparison.Ordinal) < 0) return true;
      RECT r; GetWindowRect(h, out r);
      int w = r.Right - r.Left;
      if (w < MinW || w > MaxW) return true;
      Best = h;
      return false;
    }, IntPtr.Zero);
  }

  public static int[] RectOf(IntPtr h) {
    RECT r; GetWindowRect(h, out r);
    return new int[] { r.Left, r.Top, r.Right - r.Left, r.Bottom - r.Top };
  }

  public static bool Grab(IntPtr h, string path, int w, int ht) {
    using (Bitmap bmp = new Bitmap(w, ht)) {
      using (Graphics g = Graphics.FromImage(bmp)) {
        IntPtr hdc = g.GetHdc();
        bool ok = PrintWindow(h, hdc, 2);
        g.ReleaseHdc(hdc);
        if (!ok) return false;
      }
      bmp.Save(path, System.Drawing.Imaging.ImageFormat.Png);
    }
    return true;
  }
}
"@ -ReferencedAssemblies "System.Drawing"

[WinShot]::Needle = $TitleContains
[WinShot]::MinW = $MinWidth
[WinShot]::MaxW = $MaxWidth
[WinShot]::Find()
$h = [WinShot]::Best
if ($h -eq [IntPtr]::Zero) { Write-Output "NOTFOUND"; exit 1 }

[WinShot]::SetForegroundWindow($h) | Out-Null
Start-Sleep -Milliseconds 500
$r = [WinShot]::RectOf($h)
$w = $r[2]; $ht = $r[3]
if ($w -le 0 -or $ht -le 0) { Write-Output "BADSIZE"; exit 1 }

$full = (Resolve-Path -LiteralPath ".").Path + "\" + $Out
$ok = [WinShot]::Grab($h, $full, $w, $ht)
Write-Output ("PrintWindow=" + $ok + " " + $w + "x" + $ht + " -> " + $full)
