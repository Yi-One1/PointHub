Add-Type -AssemblyName System.Windows.Forms
foreach ($s in [System.Windows.Forms.Screen]::AllScreens) {
  Write-Output ("SCREEN " + $s.DeviceName + " bounds=" + $s.Bounds.Width + "x" + $s.Bounds.Height + " work=" + $s.WorkingArea.Width + "x" + $s.WorkingArea.Height)
}
Add-Type -AssemblyName System.Drawing
$g = [System.Drawing.Graphics]::FromHwnd([IntPtr]::Zero)
Write-Output ("DPI " + $g.DpiX + " x " + $g.DpiY)
$g.Dispose()
