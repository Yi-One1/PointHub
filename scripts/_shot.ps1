param(
  [string]$ProcessName = "pointhub",
  [string]$Out = "shot.png",
  [int]$MinWidth = 400,
  [int]$MaxWidth = 100000
)

# Capture the largest visible top-level window of a process to PNG.
# Picks by size instead of title: the app has two windows (main + floating ball)
# and the title is Chinese, which is fragile to pass through a shell.
# ASCII only on purpose (PowerShell 5.1 + zh-CN ANSI parsing).

Add-Type -AssemblyName System.Drawing

Add-Type @"
using System;
using System.Text;
using System.Collections.Generic;
using System.Runtime.InteropServices;
public class WinCap {
  public delegate bool EnumProc(IntPtr h, IntPtr l);
  [DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc cb, IntPtr l);
  [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
  [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
  [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
  [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr h, int c);
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
}
"@

$procs = Get-Process -Name $ProcessName -ErrorAction SilentlyContinue
if (-not $procs) { Write-Output "NO PROCESS"; exit 1 }

$best = [IntPtr]::Zero
$bestArea = 0
$bestRect = $null
foreach ($p in $procs) {
  foreach ($h in [WinCap]::Windows([uint32]$p.Id)) {
    $r = [WinCap]::Rect($h)
    if ($r[2] -lt $MinWidth) { continue }
    if ($r[2] -gt $MaxWidth) { continue }
    $area = $r[2] * $r[3]
    if ($area -gt $bestArea) { $bestArea = $area; $best = $h; $bestRect = $r }
  }
}

if ($best -eq [IntPtr]::Zero) { Write-Output "NOTFOUND"; exit 1 }

[WinCap]::ShowWindow($best, 9) | Out-Null
[WinCap]::SetForegroundWindow($best) | Out-Null
Start-Sleep -Milliseconds 600

$r = [WinCap]::Rect($best)
$w = $r[2]
$ht = $r[3]
if ($w -le 0 -or $ht -le 0) { Write-Output "BADSIZE"; exit 1 }

$bmp = New-Object System.Drawing.Bitmap($w, $ht)
$g = [System.Drawing.Graphics]::FromImage($bmp)
$g.CopyFromScreen($r[0], $r[1], 0, 0, $bmp.Size)
$bmp.Save($Out, [System.Drawing.Imaging.ImageFormat]::Png)
$g.Dispose()
$bmp.Dispose()

Write-Output ("OK " + $w + "x" + $ht + " -> " + $Out)
