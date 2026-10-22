param(
    [string]$Output = (Join-Path $PSScriptRoot '..\..\..\coordination\272-estetica-launchers\pandora'),
    [switch]$SkipBuild
)
$ErrorActionPreference = 'Stop'
$repo = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
$outputPath = [IO.Path]::GetFullPath($Output)
$null = New-Item -ItemType Directory -Force -Path $outputPath

if (-not $SkipBuild) {
    cargo build --release -p pandora_launcher --manifest-path (Join-Path $repo 'Cargo.toml')
    if ($LASTEXITCODE -ne 0) { throw "Pandora build failed ($LASTEXITCODE)" }
}
$builtExe = Join-Path $repo 'target\release\pandora_launcher.exe'
if (-not (Test-Path $builtExe)) { throw "Missing executable: $builtExe" }

$tempRoot = Join-Path ([IO.Path]::GetTempPath()) ("wbombo-272-pandora-" + [guid]::NewGuid().ToString('N'))
$null = New-Item -ItemType Directory -Path $tempRoot
# Pandora treats an executable whose name contains "portable" as an isolated
# portable install, so this run never resolves to the user's regular data folder.
$portableExe = Join-Path $tempRoot 'pandora-portable.exe'
Copy-Item -LiteralPath $builtExe -Destination $portableExe

Add-Type @'
using System;
using System.Runtime.InteropServices;
public static class CaptureWindow {
  [StructLayout(LayoutKind.Sequential)] public struct RECT { public int Left, Top, Right, Bottom; }
  [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr hWnd, out RECT rect);
  [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr hWnd, out uint processId);
  [DllImport("user32.dll")] public static extern bool IsIconic(IntPtr hWnd);
  [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr hWnd, int command);
  [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr hWnd);
  [DllImport("user32.dll")] public static extern bool PrintWindow(IntPtr hWnd, IntPtr hdc, uint flags);
}
'@
Add-Type -AssemblyName System.Drawing

function Save-Window([string]$Path, [IntPtr]$handle, [int]$processId) {
    [uint32]$owner = 0
    [void][CaptureWindow]::GetWindowThreadProcessId($handle, [ref]$owner)
    if ($owner -ne $processId) { throw "Window handle belongs to PID $owner, expected $processId" }
    $shell = New-Object -ComObject WScript.Shell
    [void]$shell.AppActivate($processId)
    [void][CaptureWindow]::ShowWindow($handle, 3)
    [void][CaptureWindow]::SetForegroundWindow($handle)
    Start-Sleep -Milliseconds 700
    $rect = New-Object CaptureWindow+RECT
    if (-not [CaptureWindow]::GetWindowRect($handle, [ref]$rect)) { throw 'Cannot read foreground window bounds' }
    if ([CaptureWindow]::IsIconic($handle)) { [void][CaptureWindow]::ShowWindow($handle, 9); Start-Sleep -Milliseconds 500; [void][CaptureWindow]::GetWindowRect($handle, [ref]$rect) }
    $width = $rect.Right - $rect.Left
    $height = $rect.Bottom - $rect.Top
    if ($width -lt 480 -or $height -lt 270) { throw "Foreground window is not a Pandora window ($width x $height)" }
    $bitmap = New-Object System.Drawing.Bitmap $width, $height
    $graphics = [System.Drawing.Graphics]::FromImage($bitmap)
    try {
        if ([CaptureWindow]::GetForegroundWindow() -eq $handle) {
            $graphics.CopyFromScreen($rect.Left, $rect.Top, 0, 0, $bitmap.Size)
        } else {
            $dc = $graphics.GetHdc()
            try {
                if (-not [CaptureWindow]::PrintWindow($handle, $dc, 2)) { throw 'Unable to capture Pandora window' }
            } finally { $graphics.ReleaseHdc($dc) }
        }
        $bitmap.Save($Path, [System.Drawing.Imaging.ImageFormat]::Png)
    } finally { $graphics.Dispose(); $bitmap.Dispose() }
}

$oldPage = $env:WBOMBO_UI_CAPTURE_PAGE
$screens = @(
    @{ page = 'instances'; file = 'instances.png'; label = 'Carrusel de perfiles globales y lista de instancias (vacía en datos limpios)' },
    @{ page = 'appearance'; file = 'settings-appearance.png'; label = 'Ajustes y apariencia' },
    @{ page = 'accounts'; file = 'accounts.png'; label = 'Hoja de cuentas vacía (sin iniciar sesión)' }
)
try {
    foreach ($screen in $screens) {
        $env:WBOMBO_UI_CAPTURE_PAGE = $screen.page
        $proc = Start-Process -FilePath $portableExe -PassThru
        $deadline = (Get-Date).AddSeconds(45)
        while ((Get-Date) -lt $deadline -and ($proc.MainWindowHandle -eq 0 -or $proc.HasExited)) {
            Start-Sleep -Milliseconds 300
            $proc.Refresh()
        }
        if ($proc.HasExited -or $proc.MainWindowHandle -eq 0) { throw "Pandora did not open for $($screen.page)" }
        [void][CaptureWindow]::ShowWindow($proc.MainWindowHandle, 3)
        [void][CaptureWindow]::SetForegroundWindow($proc.MainWindowHandle)
        Start-Sleep -Seconds 3
        Save-Window (Join-Path $outputPath $screen.file) $proc.MainWindowHandle $proc.Id
        $proc.CloseMainWindow() | Out-Null
        if (-not $proc.WaitForExit(5000)) { $proc.Kill(); $proc.WaitForExit() }
        Write-Output "CAPTURE_OK page=$($screen.page) file=$($screen.file)"
    }
    @{
        launcher = 'BootOptimPandora'
        dataDirectory = 'temporary portable directory; removed after capture'
        screens = @($screens | ForEach-Object { @{ page=$_.page; file=$_.file; note=$_.label } })
        unavailable = @(
            @{ page='instance-detail'; reason='The isolated environment has no installed instance fixture.' },
            @{ page='update-dialog'; reason='No signed update fixture is available in the isolated data directory.' }
        )
    } | ConvertTo-Json -Depth 5 | Set-Content -Encoding utf8 (Join-Path $outputPath 'capture-manifest.json')
} finally {
    if ($null -ne $proc -and -not $proc.HasExited) {
        $proc.CloseMainWindow() | Out-Null
        if (-not $proc.WaitForExit(5000)) { $proc.Kill(); $proc.WaitForExit() }
    }
    if ($null -ne $oldPage) { $env:WBOMBO_UI_CAPTURE_PAGE = $oldPage } else { Remove-Item Env:WBOMBO_UI_CAPTURE_PAGE -ErrorAction SilentlyContinue }
    Remove-Item -LiteralPath $tempRoot -Recurse -Force -ErrorAction SilentlyContinue
}
