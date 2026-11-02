param(
    [Parameter(Mandatory)][string]$Executable,
    [Parameter(Mandatory)][string]$ExpectedIco,
    [string]$Output,
    [long]$WindowHandle = 0
)
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Drawing
if (-not ('PandoraAppIconNative' -as [type])) {
Add-Type @'
using System;
using System.Runtime.InteropServices;
public static class PandoraAppIconNative {
    [DllImport("user32.dll", CharSet=CharSet.Unicode, SetLastError=true)]
    static extern uint PrivateExtractIcons(string file, int index, int width, int height, IntPtr[] icons, uint[] ids, uint count, uint flags);
    [DllImport("user32.dll")] public static extern bool DestroyIcon(IntPtr icon);
    [DllImport("user32.dll")] static extern IntPtr SendMessageW(IntPtr hwnd, uint message, IntPtr wp, IntPtr lp);
    [DllImport("user32.dll", EntryPoint="GetClassLongPtrW")] static extern IntPtr GetClassLongPtr(IntPtr hwnd, int index);
    public static IntPtr Extract(string file, int size) {
        var icons = new IntPtr[1]; var ids = new uint[1];
        if (PrivateExtractIcons(file, 0, size, size, icons, ids, 1, 0) != 1 || icons[0] == IntPtr.Zero)
            throw new Exception("Cannot extract native icon from " + file);
        return icons[0];
    }
    public static IntPtr WindowIcon(IntPtr hwnd) {
        var icon = SendMessageW(hwnd, 0x007F, (IntPtr)1, IntPtr.Zero);
        if (icon == IntPtr.Zero) icon = GetClassLongPtr(hwnd, -14);
        return icon;
    }
}
'@
}
function Read-Icon([IntPtr]$Handle) {
    if ($Handle -eq [IntPtr]::Zero) { throw 'Missing native icon handle' }
    $icon = [Drawing.Icon]::FromHandle($Handle)
    try { return ,$icon.ToBitmap() } finally { $icon.Dispose() }
}
function Extract-Icon([string]$Path, [int]$Size) {
    $handle = [PandoraAppIconNative]::Extract($Path, $Size)
    try { return ,(Read-Icon $handle) } finally { $null=[PandoraAppIconNative]::DestroyIcon($handle) }
}
function Assert-SameIcon($Actual, $Expected) {
    if ($Actual.Size -ne $Expected.Size) { throw 'Native icon dimensions differ' }
    for ($y=0; $y -lt $Actual.Height; $y++) {
        for ($x=0; $x -lt $Actual.Width; $x++) {
            $a=$Actual.GetPixel($x,$y); $b=$Expected.GetPixel($x,$y)
            if (($a.A -ne 0 -or $b.A -ne 0) -and $a.ToArgb() -ne $b.ToArgb()) { throw "Native icon differs at $x,$y" }
        }
    }
}
$exePath=(Resolve-Path -LiteralPath $Executable).Path
$icoPath=(Resolve-Path -LiteralPath $ExpectedIco).Path
if ($Output) { $null=New-Item -ItemType Directory -Path $Output -Force }
$results=@()
foreach($size in @(16,32,256)) {
    $actual=Extract-Icon $exePath $size
    $expected=Extract-Icon $icoPath $size
    try {
        Assert-SameIcon $actual $expected
        if ($Output) {
            $actual.Save((Join-Path $Output "exe-$size.png"),[Drawing.Imaging.ImageFormat]::Png)
            foreach($tone in @('dark','light')) {
                $canvas=[Drawing.Bitmap]::new($size,$size)
                $graphics=[Drawing.Graphics]::FromImage($canvas)
                try {
                    $color=if($tone -eq 'dark'){[Drawing.Color]::FromArgb(32,32,32)}else{[Drawing.Color]::FromArgb(243,243,243)}
                    $graphics.Clear($color); $graphics.DrawImageUnscaled($actual,0,0)
                    $canvas.Save((Join-Path $Output "exe-$size-$tone.png"),[Drawing.Imaging.ImageFormat]::Png)
                } finally { $graphics.Dispose(); $canvas.Dispose() }
            }
        }
        $results+=@{size=$size;nativeResourceMatchesApprovedIco=$true}
    } finally { $actual.Dispose(); $expected.Dispose() }
}
$windowResult=$null
if($WindowHandle -ne 0) {
    $windowIcon=Read-Icon ([PandoraAppIconNative]::WindowIcon([IntPtr]$WindowHandle))
    $expected=Extract-Icon $icoPath $windowIcon.Width
    try {
        Assert-SameIcon $windowIcon $expected
        $windowResult=@{size=$windowIcon.Width;matchesApprovedIco=$true;method='WM_GETICON with class HICON fallback'}
        if($Output){$windowIcon.Save((Join-Path $Output 'window-native-icon.png'),[Drawing.Imaging.ImageFormat]::Png)}
    } finally { $windowIcon.Dispose(); $expected.Dispose() }
}
$record=@{executableSha256=(Get-FileHash -LiteralPath $exePath -Algorithm SHA256).Hash.ToLowerInvariant();expectedIcoSha256=(Get-FileHash -LiteralPath $icoPath -Algorithm SHA256).Hash.ToLowerInvariant();resourceFrames=$results;window=$windowResult;backgroundComparison='Native resource composited on #202020/#f3f3f3; these are diagnostic surfaces, not actual taskbar screenshots'}
if($Output){$record | ConvertTo-Json -Depth 5 | Set-Content -Encoding utf8 (Join-Path $Output 'icon-validation.json')}
$record | ConvertTo-Json -Depth 5
