param(
  [string]$Path = "shot.png",
  [string]$Rows = "70,300,700",
  [int]$XFrom = 980,
  [int]$XTo = 1136,
  [int]$Step = 12
)

Add-Type -AssemblyName System.Drawing
$full = (Resolve-Path $Path).Path
$bmp = New-Object System.Drawing.Bitmap($full)
Write-Output ("SIZE " + $bmp.Width + "x" + $bmp.Height)

foreach ($y in ($Rows -split ',')) {
  $yy = [int]$y
  $out = @()
  for ($x = $XFrom; $x -lt [Math]::Min($XTo, $bmp.Width); $x += $Step) {
    $c = $bmp.GetPixel($x, $yy)
    $out += ("{0}:{1:X2}{2:X2}{3:X2}" -f $x, $c.R, $c.G, $c.B)
  }
  Write-Output ("y=" + $yy + "  " + ($out -join "  "))
}
$bmp.Dispose()
