#Requires -Version 5.1
<#
    Generates assets/icon.ico.

    The glyph is the same eighth note as the tray icon drawn in src/art.rs, laid
    out in the same 32x32 design space and sampled with supersampling for
    anti-aliasing. Everything is plain arithmetic and BinaryWriter, so the script
    needs no drawing library and produces byte-identical output on any machine.

    The .ico is committed, so this only has to be run when the glyph changes:

        powershell -File assets/make_icon.ps1
#>
[CmdletBinding()]
param(
    [string]$Out
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

# `$PSScriptRoot` is empty when the script is dot-sourced or invoked oddly, so
# fall back to the command's own path.
$scriptDir = if ($PSScriptRoot) {
    $PSScriptRoot
} else {
    Split-Path -Parent $MyInvocation.MyCommand.Definition
}
if (-not $Out) {
    $Out = Join-Path $scriptDir 'icon.ico'
}

# Sizes Windows actually asks for. 256 needs the doubled height field below.
$sizes = @(16, 24, 32, 48, 64, 128, 256)

# Light glyph, matching the tray icon (240,240,240) in src/art.rs.
$inkB = 240
$inkG = 240
$inkR = 240

function New-IconPixels {
    param([int]$Size)

    # Design space is 32x32; oversample more where the pixels are chunky.
    $samples = if ($Size -le 64) { 4 } else { 2 }
    $step = 32.0 / $Size
    $stepDiv = $step / $samples
    $inv = 1.0 / ($samples * $samples)

    $pixels = New-Object 'byte[]' ($Size * $Size * 4)

    for ($y = 0; $y -lt $Size; $y++) {
        for ($x = 0; $x -lt $Size; $x++) {
            $hits = 0
            $baseY = $y * $step
            $baseX = $x * $step
            for ($sy = 0; $sy -lt $samples; $sy++) {
                $gy = $baseY + ($sy + 0.5) * $stepDiv
                for ($sx = 0; $sx -lt $samples; $sx++) {
                    $gx = $baseX + ($sx + 0.5) * $stepDiv

                    # Elliptical note head.
                    $dx = ($gx - 11.0) / 7.0
                    $dy = ($gy - 23.0) / 5.5
                    $inside = ($dx * $dx + $dy * $dy) -le 1.0

                    # Stem.
                    if (-not $inside) {
                        $inside = ($gx -ge 16.5) -and ($gx -le 19.5) -and
                                  ($gy -ge 5.0) -and ($gy -le 24.0)
                    }
                    # Flag.
                    if (-not $inside) {
                        $inside = ($gy -ge 5.0) -and ($gy -le 12.0) -and
                                  ($gx -ge 18.5) -and ($gx -le (18.5 + (12.0 - $gy) * 1.2))
                    }
                    if ($inside) { $hits++ }
                }
            }

            $alpha = [int][math]::Round(255.0 * $hits * $inv)
            $i = ($y * $Size + $x) * 4
            $pixels[$i] = [byte]$inkB
            $pixels[$i + 1] = [byte]$inkG
            $pixels[$i + 2] = [byte]$inkR
            $pixels[$i + 3] = [byte]$alpha
        }
    }
    return ,$pixels
}

function New-IconDib {
    param([int]$Size, [byte[]]$Pixels)

    $ms = New-Object System.IO.MemoryStream
    $bw = New-Object System.IO.BinaryWriter($ms)

    # BITMAPINFOHEADER. Height is doubled because an icon DIB carries the colour
    # bitmap followed by a 1-bit AND mask (kept all zero: the alpha channel is
    # what Windows uses for 32bpp icons).
    $bw.Write([uint32]40)
    $bw.Write([int32]$Size)
    $bw.Write([int32]($Size * 2))
    $bw.Write([uint16]1)
    $bw.Write([uint16]32)
    $bw.Write([uint32]0)
    $bw.Write([uint32]($Size * $Size * 4))
    $bw.Write([int32]0)
    $bw.Write([int32]0)
    $bw.Write([uint32]0)
    $bw.Write([uint32]0)

    # Pixels are stored bottom-up.
    $rowBytes = $Size * 4
    for ($y = $Size - 1; $y -ge 0; $y--) {
        $bw.Write([byte[]]$Pixels, $y * $rowBytes, $rowBytes)
    }

    # AND mask rows are padded to 4 bytes.
    $maskRow = [int]([math]::Floor(($Size + 31) / 32) * 4)
    $mask = New-Object 'byte[]' ($maskRow * $Size)
    for ($y = 0; $y -lt $Size; $y++) {
        $bw.Write($mask, 0, $maskRow)
    }

    $bw.Flush()
    return ,$ms.ToArray()
}

Write-Host "Rendering $($sizes.Count) sizes..."
$images = New-Object System.Collections.ArrayList
foreach ($size in $sizes) {
    $pixels = New-IconPixels -Size $size
    [void]$images.Add((New-IconDib -Size $size -Pixels $pixels))
    Write-Host ("  {0,3}x{0,-3} {1,7} bytes" -f $size, $images[$images.Count - 1].Length)
}

$ms = New-Object System.IO.MemoryStream
$bw = New-Object System.IO.BinaryWriter($ms)

# ICONDIR: reserved, type (1 = icon), image count.
$bw.Write([uint16]0)
$bw.Write([uint16]1)
$bw.Write([uint16]$sizes.Count)

# ICONDIRENTRY is 16 bytes: width, height, colours, reserved, planes, bpp,
# data size, data offset. A dimension of 256 is written as 0.
$offset = 6 + 16 * $sizes.Count
for ($i = 0; $i -lt $sizes.Count; $i++) {
    $size = $sizes[$i]
    $data = $images[$i]
    $d = if ($size -ge 256) { 0 } else { $size }

    $entry = New-Object 'byte[]' 16
    $entry[0] = [byte]$d
    $entry[1] = [byte]$d
    $entry[2] = 0
    $entry[3] = 0
    [BitConverter]::GetBytes([uint16]1).CopyTo($entry, 4)
    [BitConverter]::GetBytes([uint16]32).CopyTo($entry, 6)
    [BitConverter]::GetBytes([uint32]$data.Length).CopyTo($entry, 8)
    [BitConverter]::GetBytes([uint32]$offset).CopyTo($entry, 12)
    $bw.Write([byte[]]$entry, 0, $entry.Length)

    $offset += $data.Length
}

foreach ($data in $images) {
    $bw.Write([byte[]]$data, 0, $data.Length)
}
$bw.Flush()

[System.IO.File]::WriteAllBytes($Out, $ms.ToArray())
Write-Host ("Wrote {0} ({1} bytes)" -f $Out, (Get-Item $Out).Length)
