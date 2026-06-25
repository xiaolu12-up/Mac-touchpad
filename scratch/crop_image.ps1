Add-Type -AssemblyName System.Drawing
$file = Get-ChildItem -Filter "*.png" | Where-Object { $_.Name -like "ChatGPT*" } | Select-Object -First 1
if (-not $file) {
    Write-Host "No image found"
    exit
}

Write-Host "Cropping file: $($file.FullName)"
$srcBmp = New-Object System.Drawing.Bitmap($file.FullName)

# Bounding box found: X=[86, 934], Y=[83, 958]
$minX = 86
$maxX = 934
$minY = 83
$maxY = 958

$w = $maxX - $minX + 1
$h = $maxY - $minY + 1
$size = [Math]::Max($w, $h)

# Create a new square bitmap
$dstBmp = New-Object System.Drawing.Bitmap($size, $size)
$g = [System.Drawing.Graphics]::FromImage($dstBmp)
$g.Clear([System.Drawing.Color]::Transparent)

# Center the source cropped area in the destination square bitmap
$dstX = ($size - $w) / 2
$dstY = ($size - $h) / 2

$srcRect = New-Object System.Drawing.Rectangle($minX, $minY, $w, $h)
$dstRect = New-Object System.Drawing.Rectangle($dstX, $dstY, $w, $h)

$g.DrawImage($srcBmp, $dstRect, $srcRect, [System.Drawing.GraphicsUnit]::Pixel)

# Save to a temporary file in the root
$croppedPath = Join-Path $file.DirectoryName "cropped_icon.png"
$dstBmp.Save($croppedPath, [System.Drawing.Imaging.ImageFormat]::Png)

# Also save directly to tray_icon.png in src-tauri/icons
$trayIconDir = Join-Path $file.DirectoryName "src-tauri\icons"
if (-not (Test-Path $trayIconDir)) {
    New-Item -ItemType Directory -Path $trayIconDir | Out-Null
}
$trayIconPath = Join-Path $trayIconDir "tray_icon.png"
$dstBmp.Save($trayIconPath, [System.Drawing.Imaging.ImageFormat]::Png)

$g.Dispose()
$dstBmp.Dispose()
$srcBmp.Dispose()

Write-Host "Cropped image saved to: $croppedPath and $trayIconPath"
