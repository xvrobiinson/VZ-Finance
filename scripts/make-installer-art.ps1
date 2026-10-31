# Makes the installer's artwork from the VZ Labs logo (installer\branding\
# vz-labs-logo.png, white on black): the setup program's icon and the two
# wizard pictures at 100%, 150% and 200% display scaling. Run it again after
# changing the logo:  pwsh scripts\make-installer-art.ps1

$ErrorActionPreference = "Stop"
Add-Type -AssemblyName System.Drawing
$dir = Join-Path $PSScriptRoot "..\installer\branding"
$logo = [System.Drawing.Bitmap]::FromFile((Join-Path $dir "vz-labs-logo.png"))

# A square around the mark and the word LABS, with a little room around it.
$crop = New-Object System.Drawing.Rectangle 267, 365, 1390, 1390
$black = [System.Drawing.Color]::Black

function New-Canvas([int]$w, [int]$h) {
    $bmp = New-Object System.Drawing.Bitmap $w, $h, ([System.Drawing.Imaging.PixelFormat]::Format24bppRgb)
    $g = [System.Drawing.Graphics]::FromImage($bmp)
    $g.Clear($black)
    $g.InterpolationMode = [System.Drawing.Drawing2D.InterpolationMode]::HighQualityBicubic
    $g.SmoothingMode = [System.Drawing.Drawing2D.SmoothingMode]::HighQuality
    $g.PixelOffsetMode = [System.Drawing.Drawing2D.PixelOffsetMode]::HighQuality
    return @($bmp, $g)
}

function Draw-Logo($g, [int]$x, [int]$y, [int]$side) {
    $g.DrawImage($logo, (New-Object System.Drawing.Rectangle $x, $y, $side, $side), $crop, [System.Drawing.GraphicsUnit]::Pixel)
}

# Corner picture (top right of every page): the logo filling the square.
foreach ($s in 55, 83, 110) {
    $bmp, $g = New-Canvas $s $s
    Draw-Logo $g 0 0 $s
    $g.Dispose(); $bmp.Save((Join-Path $dir "wizard-small-$s.bmp"), [System.Drawing.Imaging.ImageFormat]::Bmp); $bmp.Dispose()
}

# Side picture (welcome and finish pages): the logo on black, centred.
foreach ($size in @(@(164, 314), @(246, 471), @(328, 628))) {
    $w, $h = $size
    $bmp, $g = New-Canvas $w $h
    $side = [int]($w * 0.86)
    Draw-Logo $g ([int](($w - $side) / 2)) ([int](($h - $side) / 2)) $side
    $g.Dispose(); $bmp.Save((Join-Path $dir "wizard-$w.bmp"), [System.Drawing.Imaging.ImageFormat]::Bmp); $bmp.Dispose()
}

# The setup program's icon: PNG images inside one .ico (Windows Vista and later).
$sizes = 16, 24, 32, 48, 64, 128, 256
$pngs = foreach ($s in $sizes) {
    $bmp, $g = New-Canvas $s $s
    Draw-Logo $g 0 0 $s
    $g.Dispose()
    $ms = New-Object System.IO.MemoryStream
    $bmp.Save($ms, [System.Drawing.Imaging.ImageFormat]::Png); $bmp.Dispose()
    , $ms.ToArray()
}
$out = New-Object System.IO.MemoryStream
$w = New-Object System.IO.BinaryWriter $out
$w.Write([uint16]0); $w.Write([uint16]1); $w.Write([uint16]$sizes.Count)
$offset = 6 + 16 * $sizes.Count
for ($i = 0; $i -lt $sizes.Count; $i++) {
    $s = $sizes[$i]; $len = $pngs[$i].Length
    $w.Write([byte]($s % 256)); $w.Write([byte]($s % 256)); $w.Write([byte]0); $w.Write([byte]0)
    $w.Write([uint16]1); $w.Write([uint16]32); $w.Write([uint32]$len); $w.Write([uint32]$offset)
    $offset += $len
}
foreach ($p in $pngs) { $w.Write($p) }
$w.Flush()
[System.IO.File]::WriteAllBytes((Join-Path $dir "vz-labs.ico"), $out.ToArray())
$logo.Dispose()
"made installer art in $dir"
