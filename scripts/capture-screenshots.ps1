# Captures the dock and the launcher over a bright test pattern into target\shots.
# Run from the repository root after:
#   cargo build --release --example launcher_demo --features perf-trace
param([string]$Prefix = 'shot', [string]$Demo = 'target\release\examples\launcher_demo.exe', [string]$Phase = 'now', [string[]]$Themes = @('dark','light'), [int[]]$Dpis = @(96,120), [string[]]$States = @('dock','launcher'), [ValidateSet('pattern','wallpaper')][string]$Backdrop = 'pattern')
$ErrorActionPreference = 'Stop'
New-Item -ItemType Directory -Force 'target\shots' | Out-Null
Add-Type -AssemblyName System.Windows.Forms,System.Drawing
Add-Type -ReferencedAssemblies System.Windows.Forms @'
using System;
using System.Windows.Forms;
public class CaptureBackdrop : Form {
 protected override bool ShowWithoutActivation { get { return true; } }
 protected override CreateParams CreateParams { get { var p=base.CreateParams; p.ExStyle |= 0x08000000; return p; } }
}
'@
$form = New-Object CaptureBackdrop
$form.FormBorderStyle = 'None'
$form.StartPosition = 'Manual'
$screen = [Windows.Forms.Screen]::PrimaryScreen.Bounds
$form.Bounds = New-Object Drawing.Rectangle($screen.Left,($screen.Bottom-700),$screen.Width,700)
$form.TopMost = $true
$form.add_Paint({param($sender,$e)
 if ($Backdrop -eq 'wallpaper') {
  # Soft diagonal gradients, as on a desktop wallpaper, for README images.
  $area = New-Object Drawing.Rectangle(0,0,$sender.Width,700)
  $b = New-Object Drawing.Drawing2D.LinearGradientBrush($area,[Drawing.ColorTranslator]::FromHtml('#1b2a6b'),[Drawing.ColorTranslator]::FromHtml('#c2577a'),20.0)
  $blend = New-Object Drawing.Drawing2D.ColorBlend(3)
  $blend.Colors = @([Drawing.ColorTranslator]::FromHtml('#14204f'),[Drawing.ColorTranslator]::FromHtml('#5b3f9e'),[Drawing.ColorTranslator]::FromHtml('#e08a6b'))
  $blend.Positions = @(0.0,0.55,1.0)
  $b.InterpolationColors = $blend
  $e.Graphics.FillRectangle($b,$area); $b.Dispose()
  return
 }
 # A bright pattern shows how much the acrylic blur lets through.
 $colors = @('#ff345c','#ffd731','#25dfbe','#35a8ff','#ae67ff')
 for($x=0;$x -lt $sender.Width;$x+=80) { for($y=0;$y -lt 700;$y+=80) {
  $b=New-Object Drawing.SolidBrush([Drawing.ColorTranslator]::FromHtml($colors[(($x/80)+($y/80))%5])); $e.Graphics.FillRectangle($b,$x,$y,80,80); $b.Dispose()
 } }
})
$form.Show()
[Windows.Forms.Application]::DoEvents()
$originalAppData=$env:APPDATA
try {
 foreach($theme in $Themes) { foreach($dpi in $Dpis) { foreach($state in $States) {
  $env:APPDATA=Join-Path $PWD 'target\capture-data'
  New-Item -ItemType Directory -Force "$env:APPDATA\wisp-demo" | Out-Null
  "theme = `"$theme`"`nposition = `"bottom`"`ndock = true`nhide_taskbar = false`nfile_search = false" | Set-Content "$env:APPDATA\wisp-demo\config.toml"
  $env:WISP_CAPTURE='1'; $env:DOCK_DEMO_DPI="$dpi"; $env:WISP_CAPTURE_QUERY=if($state -eq 'results8') {'e'} elseif($state -eq 'results1') {'zzzzzzzz'} else {'time'}; $env:WISP_FRAME_TIMING='1'; $env:WISP_CAPTURE_ROWS=if($state.StartsWith('results')) {$state.Substring(7)} else {'3'}
  if ($state.StartsWith('morph')) { $env:WISP_CAPTURE_MORPH=([int]$state.Substring(5)/100.0).ToString([Globalization.CultureInfo]::InvariantCulture) } else { Remove-Item Env:\WISP_CAPTURE_MORPH -ErrorAction SilentlyContinue }
  $arg = if($state.StartsWith('results') -or $state -eq 'launcher' -or $state.StartsWith('morph')) { '--show' } else { '--capture' }
  $p=Start-Process $Demo -ArgumentList $arg -PassThru -WindowStyle Hidden -RedirectStandardError (Join-Path $PWD "target\shots\$Prefix-$Phase-$state-$theme-$dpi.log")
  $until=[DateTime]::UtcNow.AddSeconds(3)
  while([DateTime]::UtcNow -lt $until) { [Windows.Forms.Application]::DoEvents(); Start-Sleep -Milliseconds 30 }
  $bmp=New-Object Drawing.Bitmap($screen.Width,700)
  $g=[Drawing.Graphics]::FromImage($bmp)
  $g.CopyFromScreen($screen.Left,($screen.Bottom-700),0,0,$bmp.Size)
  $path=Join-Path $PWD "target\shots\$Prefix-$Phase-$state-$theme-$dpi.png"
  $bmp.Save($path,[Drawing.Imaging.ImageFormat]::Png); $g.Dispose(); $bmp.Dispose()
  $p.WaitForExit()
  Write-Output $path
 } } }
} finally { $env:APPDATA=$originalAppData; $form.Close(); $form.Dispose(); Remove-Item Env:\WISP_CAPTURE,Env:\WISP_CAPTURE_QUERY,Env:\DOCK_DEMO_DPI,Env:\WISP_CAPTURE_MORPH,Env:\WISP_CAPTURE_ROWS,Env:\WISP_FRAME_TIMING -ErrorAction SilentlyContinue }


