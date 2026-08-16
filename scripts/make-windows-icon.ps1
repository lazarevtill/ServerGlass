# Reshape the app mark into the .ico Windows asks for.
#
#   .\scripts\make-windows-icon.ps1
#
# The mark is drawn in code by scripts/make-icons.swift, and that is the only place it exists. That
# script is Swift on CoreGraphics, so it runs on macOS and nowhere else. This reads the 1024px
# master it committed and downsamples that into the sizes Explorer, the taskbar and Alt-Tab want.
# Nothing here draws anything: a second drawing would be a second mark, drifting from the first the
# moment the palette changes, which is the failure this repository already keeps a list of.
#
# The 256px entry is PNG and every smaller one is a 32-bit BMP, which is the split the format
# settled on rather than an arbitrary one. PNG entries have been legal at any size since Vista, but
# only the shell reads them: GDI+ — everything built on System.Drawing.Icon, including the check at
# the bottom of this script — returns the next size down and then throws on the 256. An icon that
# the taskbar renders and half the tooling cannot open is worse than a slightly larger file.
#
# UTF-8 with a BOM, for the reason spelled out at the top of check.ps1.

$ErrorActionPreference = 'Stop'

Add-Type -AssemblyName System.Drawing

$root = Split-Path -Parent $PSScriptRoot
$source = Join-Path $root 'apps\ios\Assets.xcassets\AppIcon.appiconset\icon-1024.png'
$output = Join-Path $root 'apps\windows\ServerGlass\Assets\ServerGlass.ico'

if (-not (Test-Path $source)) {
    Write-Host "The 1024px master is missing: $source" -ForegroundColor Red
    Write-Host "Regenerate it on a Mac: swift scripts/make-icons.swift"
    exit 1
}

# 16 for the window's title bar and the task list, 32 for the taskbar and the Start Menu shortcut,
# 48 for Explorer's medium icons, 256 for its large ones and for Alt-Tab on a high-DPI display. The
# rest are the intermediate steps Windows would otherwise get by scaling one of those badly.
$sizes = @(16, 24, 32, 48, 64, 128, 256)

# A 32-bit BMP as an .ico entry, which is not quite a .bmp file: no file header, the height field
# doubled because the entry carries a transparency mask stacked under the colour data, and the rows
# bottom-up. The mask is left all-zero — with 32 bits per pixel the alpha channel is what Windows
# actually composites, and the mask exists only because the format predates alpha.
function ConvertTo-IconBitmap($bitmap) {
    $size = $bitmap.Width
    $area = New-Object System.Drawing.Rectangle 0, 0, $size, $size
    $locked = $bitmap.LockBits($area, [System.Drawing.Imaging.ImageLockMode]::ReadOnly,
        [System.Drawing.Imaging.PixelFormat]::Format32bppArgb)

    $pixels = New-Object byte[] ($size * $size * 4)
    try {
        for ($row = 0; $row -lt $size; $row++) {
            # Bottom-up: the last row of the image is the first row of the entry.
            $from = [IntPtr]::Add($locked.Scan0, $locked.Stride * ($size - 1 - $row))
            [System.Runtime.InteropServices.Marshal]::Copy($from, $pixels, $row * $size * 4, $size * 4)
        }
    }
    finally {
        $bitmap.UnlockBits($locked)
    }

    # Each mask row is padded to a 4-byte boundary, as every BMP row is.
    $maskStride = [math]::Ceiling($size / 8 / 4) * 4
    $mask = New-Object byte[] ($maskStride * $size)

    $out = New-Object System.IO.MemoryStream
    $writer = New-Object System.IO.BinaryWriter $out
    try {
        $writer.Write([uint32]40)             # BITMAPINFOHEADER size
        $writer.Write([int32]$size)           # width
        $writer.Write([int32]($size * 2))     # height: colour data plus the mask
        $writer.Write([uint16]1)              # planes
        $writer.Write([uint16]32)             # bits per pixel
        $writer.Write([uint32]0)              # BI_RGB, uncompressed
        $writer.Write([uint32]($pixels.Length + $mask.Length))
        $writer.Write([int32]0)               # pixels per metre, horizontal: unused
        $writer.Write([int32]0)               # pixels per metre, vertical: unused
        $writer.Write([uint32]0)              # palette entries used
        $writer.Write([uint32]0)              # palette entries required
        $writer.Write($pixels)
        $writer.Write($mask)
        $writer.Flush()
        # The leading comma matters: PowerShell enumerates a returned array into the pipeline, and
        # the caller would get an Object[] of boxed bytes. BinaryWriter then binds Write(char[])
        # instead of Write(byte[]) and writes two bytes per pixel byte, producing an entry whose
        # length no longer matches the one recorded in the directory. The file still writes; only
        # the shell reading it back finds out.
        return , $out.ToArray()
    }
    finally {
        $writer.Dispose()
        $out.Dispose()
    }
}

$master = [System.Drawing.Image]::FromFile($source)
$encoded = @{}

try {
    foreach ($size in $sizes) {
        $bitmap = New-Object System.Drawing.Bitmap $size, $size
        $graphics = [System.Drawing.Graphics]::FromImage($bitmap)
        try {
            $graphics.InterpolationMode = [System.Drawing.Drawing2D.InterpolationMode]::HighQualityBicubic
            $graphics.PixelOffsetMode = [System.Drawing.Drawing2D.PixelOffsetMode]::HighQuality
            $graphics.SmoothingMode = [System.Drawing.Drawing2D.SmoothingMode]::HighQuality
            $graphics.CompositingQuality = [System.Drawing.Drawing2D.CompositingQuality]::HighQuality
            $graphics.DrawImage($master, (New-Object System.Drawing.Rectangle 0, 0, $size, $size))
        }
        finally {
            $graphics.Dispose()
        }

        if ($size -eq 256) {
            $stream = New-Object System.IO.MemoryStream
            $bitmap.Save($stream, [System.Drawing.Imaging.ImageFormat]::Png)
            $encoded[$size] = $stream.ToArray()
            $stream.Dispose()
        }
        else {
            $encoded[$size] = ConvertTo-IconBitmap $bitmap
        }

        $bitmap.Dispose()
    }
}
finally {
    $master.Dispose()
}

# ICONDIR, then one 16-byte ICONDIRENTRY per size, then the PNG payloads.
$out = New-Object System.IO.MemoryStream
$writer = New-Object System.IO.BinaryWriter $out
try {
    $writer.Write([uint16]0)              # reserved
    $writer.Write([uint16]1)              # type: icon
    $writer.Write([uint16]$sizes.Count)

    $offset = 6 + (16 * $sizes.Count)
    foreach ($size in $sizes) {
        $bytes = $encoded[$size]
        # 256 is written as 0: the field is one byte, so 256 does not fit and 0 means "256".
        $dimension = if ($size -eq 256) { 0 } else { $size }
        $writer.Write([byte]$dimension)   # width
        $writer.Write([byte]$dimension)   # height
        $writer.Write([byte]0)            # palette entries: none, this is truecolour
        $writer.Write([byte]0)            # reserved
        $writer.Write([uint16]1)          # colour planes
        $writer.Write([uint16]32)         # bits per pixel
        $writer.Write([uint32]$bytes.Length)
        $writer.Write([uint32]$offset)
        $offset += $bytes.Length
    }

    foreach ($size in $sizes) {
        $writer.Write([byte[]]$encoded[$size])
    }

    $writer.Flush()
    New-Item -ItemType Directory -Force -Path (Split-Path $output) | Out-Null
    [System.IO.File]::WriteAllBytes($output, $out.ToArray())
}
finally {
    $writer.Dispose()
    $out.Dispose()
}

# Prove the file is readable rather than merely written. GDI+ is the strictest consumer in common
# use, so an entry it can open at its own size is one the shell can certainly draw. The 256 is PNG,
# which GDI+ cannot decode at all, so it is checked for presence in the directory instead.
foreach ($size in ($sizes | Where-Object { $_ -ne 256 })) {
    $icon = New-Object System.Drawing.Icon $output, $size, $size
    try {
        if ($icon.Width -ne $size -or $icon.Height -ne $size) {
            Write-Host "The $size px entry came back as $($icon.Width)x$($icon.Height)." -ForegroundColor Red
            exit 1
        }
        $icon.ToBitmap().Dispose()
    }
    finally {
        $icon.Dispose()
    }
}

$directory = [System.IO.File]::ReadAllBytes($output)
$count = [BitConverter]::ToUInt16($directory, 4)
if ($count -ne $sizes.Count) {
    Write-Host "The icon directory lists $count entries, not $($sizes.Count)." -ForegroundColor Red
    exit 1
}

$written = Get-Item $output
Write-Host "Wrote $($written.FullName) ($($sizes -join ', ') px, $([math]::Round($written.Length / 1KB)) KB)" -ForegroundColor Green
