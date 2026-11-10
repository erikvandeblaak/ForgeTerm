# Draws the sidebar image of the Windows installer (welcome and finish pages) in
# ForgeTerm's own colours (crates/forgeterm-app/src/theme.rs) with the logo of
# assets/branding/forgeterm-logo.svg. Called by package-windows.ps1; writes one BMP.
#
# NSIS's page is 164x314; drawn at 2x so it stays sharp on high-DPI screens.

param([Parameter(Mandatory)][string]$Out)

$ErrorActionPreference = "Stop"
Add-Type -AssemblyName System.Drawing

function C([int]$rgb) { [System.Drawing.Color]::FromArgb(255, ($rgb -shr 16) -band 255, ($rgb -shr 8) -band 255, $rgb -band 255) }
$obsidian = C 0x090D14; $graphite = C 0x111827; $border = C 0x253247
$cyan = C 0x4DD8E8; $violet = C 0xA78BFA; $mint = C 0x55E6B5
$text = C 0xD6DEEB; $muted = C 0x8B98AD

$w = 328; $h = 628
$bmp = New-Object System.Drawing.Bitmap $w, $h, ([System.Drawing.Imaging.PixelFormat]::Format24bppRgb)
$g = [System.Drawing.Graphics]::FromImage($bmp)
$g.SmoothingMode = [System.Drawing.Drawing2D.SmoothingMode]::AntiAlias
$g.TextRenderingHint = [System.Drawing.Text.TextRenderingHint]::AntiAliasGridFit

# Background: graphite at the top fading to obsidian.
$bg = New-Object System.Drawing.Drawing2D.LinearGradientBrush (New-Object System.Drawing.Point 0, 0), (New-Object System.Drawing.Point 0, $h), $graphite, $obsidian
$g.FillRectangle($bg, 0, 0, $w, $h)

# Logo (64-unit SVG coordinates scaled to 2.25x = 144 px, centred).
$s = 2.25; $ox = ($w - 64 * $s) / 2; $oy = 150
function P([double]$x, [double]$y) { New-Object System.Drawing.PointF ([float]($ox + $x * $s)), ([float]($oy + $y * $s)) }
function RoundRect([double]$x, [double]$y, [double]$rw, [double]$rh, [double]$r) {
    $p = New-Object System.Drawing.Drawing2D.GraphicsPath
    $X = $ox + $x * $s; $Y = $oy + $y * $s; $W = $rw * $s; $H = $rh * $s; $d = 2 * $r * $s
    $p.AddArc($X, $Y, $d, $d, 180, 90); $p.AddArc($X + $W - $d, $Y, $d, $d, 270, 90)
    $p.AddArc($X + $W - $d, $Y + $H - $d, $d, $d, 0, 90); $p.AddArc($X, $Y + $H - $d, $d, $d, 90, 90)
    $p.CloseFigure(); $p
}
$tile = RoundRect 2 2 60 60 14
$g.FillPath((New-Object System.Drawing.SolidBrush $graphite), $tile)
$g.DrawPath((New-Object System.Drawing.Pen $border, ([float](2 * $s))), $tile)
$f = [System.Drawing.PointF[]]@((P 19 15), (P 45 15), (P 45 22), (P 27 22), (P 27 29), (P 40 29), (P 40 36), (P 27 36), (P 27 49), (P 19 49))
$forge = New-Object System.Drawing.Drawing2D.LinearGradientBrush (P 0 0), (P 64 64), $cyan, $violet
$g.FillPolygon($forge, $f)
$g.FillPath((New-Object System.Drawing.SolidBrush $mint), (RoundRect 33 43 13 6 1.5))

# Name and edition, centred under the logo.
$center = New-Object System.Drawing.StringFormat
$center.Alignment = [System.Drawing.StringAlignment]::Center
$g.DrawString("ForgeTerm", (New-Object System.Drawing.Font "Segoe UI Semibold", 30, ([System.Drawing.GraphicsUnit]::Pixel)),
    (New-Object System.Drawing.SolidBrush $text), (New-Object System.Drawing.RectangleF 0, 330, $w, 50), $center)
$g.DrawString("Erik van de Blaak Edition", (New-Object System.Drawing.Font "Segoe UI", 17, ([System.Drawing.GraphicsUnit]::Pixel)),
    (New-Object System.Drawing.SolidBrush $muted), (New-Object System.Drawing.RectangleF 0, 376, $w, 30), $center)

# Accent line and signature at the bottom.
$line = New-Object System.Drawing.Drawing2D.LinearGradientBrush (New-Object System.Drawing.Point 64, 0), (New-Object System.Drawing.Point ($w - 64), 0), $cyan, $violet
$g.FillRectangle($line, 64, 540, $w - 128, 3)
$g.DrawString("Created by Erik van de Blaak", (New-Object System.Drawing.Font "Segoe UI", 15, ([System.Drawing.GraphicsUnit]::Pixel)),
    (New-Object System.Drawing.SolidBrush $muted), (New-Object System.Drawing.RectangleF 0, 556, $w, 26), $center)

$g.Dispose()
$bmp.Save($Out, [System.Drawing.Imaging.ImageFormat]::Bmp)
$bmp.Dispose()
