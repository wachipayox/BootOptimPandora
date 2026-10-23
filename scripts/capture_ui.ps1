param(
    [string]$Output = (Join-Path $PSScriptRoot '..\..\..\coordination\272-estetica-launchers\pandora'),
    [switch]$SkipBuild,
    [string]$Executable,
    [string]$TestProfile,
    [string]$Revision,
    [string]$InstanceName = 'agente-274-ui',
    [switch]$Components,
    [switch]$HomeScreens,
    [switch]$NewsScreens,
    [string]$NewsFixture,
    [string[]]$Pages,
    [switch]$SkipUpdateCapture,
    [switch]$DesignViewport,
    [string]$PortableDirectory
)
$ErrorActionPreference = 'Stop'
$repo = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
$outputPath = [IO.Path]::GetFullPath($Output)
$null = New-Item -ItemType Directory -Force -Path $outputPath

if ($TestProfile -and (-not $TestProfile.StartsWith('profile_test-') -or -not $InstanceName.StartsWith('agente-274-'))) { throw 'Capture requires an owned test profile and agent-274 instance.' }
if (-not $SkipBuild -and -not $Executable) {
    cargo build --release -p pandora_launcher --manifest-path (Join-Path $repo 'Cargo.toml')
    if ($LASTEXITCODE -ne 0) { throw "Pandora build failed ($LASTEXITCODE)" }
}
$builtExe = if ($Executable) { [IO.Path]::GetFullPath($Executable) } else { Join-Path $repo 'target\release\pandora_launcher.exe' }
if (-not (Test-Path $builtExe)) { throw "Missing executable: $builtExe" }

$ownsDirectory = -not $PortableDirectory
$tempRoot = if ($PortableDirectory) { [IO.Path]::GetFullPath($PortableDirectory) } else { Join-Path ([IO.Path]::GetTempPath()) ("wb274-" + [guid]::NewGuid().ToString('N').Substring(0,8)) }
$null = New-Item -ItemType Directory -Force -Path $tempRoot
# Pandora treats an executable whose name contains "portable" as an isolated
# portable install, so this run never resolves to the user's regular data folder.
$portableExe = Join-Path $tempRoot 'pandora-portable.exe'
if (-not (Test-Path $portableExe) -or $ownsDirectory) { Copy-Item -LiteralPath $builtExe -Destination $portableExe }

Add-Type @'
using System;
using System.Runtime.InteropServices;
public static class CaptureWindow {
  [StructLayout(LayoutKind.Sequential)] public struct RECT { public int Left, Top, Right, Bottom; }
  [StructLayout(LayoutKind.Sequential)] public struct POINT { public int X, Y; }
  [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr hWnd, out RECT rect);
  [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr hWnd, out uint processId);
  [DllImport("user32.dll")] public static extern bool IsIconic(IntPtr hWnd);
  [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr hWnd, int command);
  [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr hWnd);
  [DllImport("user32.dll")] public static extern bool PrintWindow(IntPtr hWnd, IntPtr hdc, uint flags);
  [DllImport("user32.dll")] public static extern IntPtr SetThreadDpiAwarenessContext(IntPtr context);
  [DllImport("user32.dll")] public static extern uint GetDpiForWindow(IntPtr hWnd);
  [DllImport("user32.dll")] public static extern bool GetClientRect(IntPtr hWnd, out RECT rect);
  [DllImport("user32.dll")] public static extern bool MoveWindow(IntPtr hWnd, int x, int y, int width, int height, bool repaint);
  [DllImport("user32.dll")] public static extern bool ClientToScreen(IntPtr hWnd, ref POINT point);
  [DllImport("user32.dll")] public static extern bool PostMessage(IntPtr hWnd, uint message, IntPtr wParam, IntPtr lParam);
}
'@
Add-Type -AssemblyName System.Drawing
$oldDpiContext = [CaptureWindow]::SetThreadDpiAwarenessContext([IntPtr](-4))
$captureMetadata = @()

function Save-Window([string]$Path, [IntPtr]$handle, [int]$processId) {
    [uint32]$owner = 0
    [void][CaptureWindow]::GetWindowThreadProcessId($handle, [ref]$owner)
    if ($owner -ne $processId) { throw "Window handle belongs to PID $owner, expected $processId" }
    $shell = New-Object -ComObject WScript.Shell
    [void]$shell.AppActivate($processId)
    [void][CaptureWindow]::ShowWindow($handle, $(if ($DesignViewport) { 9 } else { 3 }))
    [void][CaptureWindow]::SetForegroundWindow($handle)
    Start-Sleep -Milliseconds 700
    $rect = New-Object CaptureWindow+RECT
    if (-not [CaptureWindow]::GetWindowRect($handle, [ref]$rect)) { throw 'Cannot read foreground window bounds' }
    if ($DesignViewport) {
        $client = New-Object CaptureWindow+RECT
        [void][CaptureWindow]::GetClientRect($handle, [ref]$client)
        $scale = [CaptureWindow]::GetDpiForWindow($handle) / 96.0
        $frameWidth = ($rect.Right - $rect.Left) - ($client.Right - $client.Left)
        $frameHeight = ($rect.Bottom - $rect.Top) - ($client.Bottom - $client.Top)
        [void][CaptureWindow]::MoveWindow($handle, 32, 32, [int](1280 * $scale + $frameWidth), [int](860 * $scale + $frameHeight), $true)
        Start-Sleep -Milliseconds 700
        [void][CaptureWindow]::GetWindowRect($handle, [ref]$rect)
    }
    if ([CaptureWindow]::IsIconic($handle)) { [void][CaptureWindow]::ShowWindow($handle, 9); Start-Sleep -Milliseconds 500; [void][CaptureWindow]::GetWindowRect($handle, [ref]$rect) }
    # Capture the client area, excluding invisible DWM margins and desktop pixels.
    $client = New-Object CaptureWindow+RECT
    $clientOrigin = New-Object CaptureWindow+POINT
    if (-not [CaptureWindow]::GetClientRect($handle, [ref]$client) -or -not [CaptureWindow]::ClientToScreen($handle, [ref]$clientOrigin)) { throw 'Cannot read client capture bounds' }
    if ($DesignViewport -and ($clientOrigin.X -lt 0 -or $clientOrigin.Y -lt 0)) { throw 'Design viewport is outside the visible desktop' }
    $width = $client.Right - $client.Left
    $height = $client.Bottom - $client.Top
    if ($width -lt 480 -or $height -lt 270) { throw "Foreground window is not a Pandora window ($width x $height)" }
    $bitmap = New-Object System.Drawing.Bitmap $width, $height
    $graphics = [System.Drawing.Graphics]::FromImage($bitmap)
    try {
        if ([CaptureWindow]::GetForegroundWindow() -eq $handle) {
            $graphics.CopyFromScreen($clientOrigin.X, $clientOrigin.Y, 0, 0, $bitmap.Size)
        } else {
            $dc = $graphics.GetHdc()
            try {
                if (-not [CaptureWindow]::PrintWindow($handle, $dc, 3)) { throw 'Unable to capture Pandora window' }
            } finally { $graphics.ReleaseHdc($dc) }
        }
        $bitmap.Save($Path, [System.Drawing.Imaging.ImageFormat]::Png)
        $script:captureMetadata += @{ file=[IO.Path]::GetFileName($Path); width=$width; height=$height; dpi=[CaptureWindow]::GetDpiForWindow($handle) }
    } finally { $graphics.Dispose(); $bitmap.Dispose() }
}

function Invoke-CaptureCli([string[]]$Arguments) {
    $cli = Start-Process -FilePath $portableExe -ArgumentList $Arguments -WindowStyle Hidden -Wait -PassThru -RedirectStandardOutput (Join-Path $tempRoot 'capture-cli.out') -RedirectStandardError (Join-Path $tempRoot 'capture-cli.err')
    if ($cli.ExitCode -ne 0) { throw "Capture CLI failed ($($cli.ExitCode)); inspect isolated capture-cli files." }
}
$oldNewsFixture = $env:WBOMBO_UI_CAPTURE_NEWS
if ($NewsFixture) { $env:WBOMBO_UI_CAPTURE_NEWS = (Resolve-Path -LiteralPath $NewsFixture).Path }
$createdInstance = $false
$oldPage = $env:WBOMBO_UI_CAPTURE_PAGE
$screens = @(
    @{ page = 'instances'; file = 'instances.png'; label = 'Inicio con la instancia seleccionada o el fallback Vanilla' },
    @{ page = 'appearance'; file = 'settings-appearance.png'; label = 'Ajustes y apariencia' },
    @{ page = 'accounts'; file = 'accounts.png'; label = 'Menú de cuentas (sin iniciar sesión)' }
)
try {
    if ($TestProfile) {
        $createArgs = @('--create-instance', $InstanceName, '--from-profile', $TestProfile)
        if ($Revision) { $createArgs += @('--revision', $Revision) }
        Invoke-CaptureCli $createArgs
        $createdInstance = $true
    }
    if ($TestProfile -or $PortableDirectory) {
        $screens += @{ page = "instance:$InstanceName"; file = 'instance-detail.png'; label = 'Owned CLI instance detail' }
        if (-not $SkipUpdateCapture) { $screens += @{ page = "update:$InstanceName"; file = 'update-dialog.png'; label = 'Real inherited update check on owned CLI instance' } }
    }
    if ($HomeScreens) {
        $screens += @{ page='home-menu'; file='home-menu.png'; label='Menú de la instancia seleccionada' }
        $screens += @{ page='home-news'; file='home-news.png'; label='Destino de Ver todas (pantalla completa en paso 3)' }
    }
    if ($NewsScreens) {
        foreach ($newsPage in @('home-news','news-history','news-saved','news-search','news-profiles','news-article','news-changelog')) {
            $screens += @{ page=$newsPage; file="$newsPage.png"; label='Native News screen/state' }
        }
    }
    if ($Pages) { $screens = @($Pages | ForEach-Object { @{ page=$_; file=(($_ -replace ':','-') + '.png'); label='Explicit native capture route' } }) }
    if ($Components) {
        $screens += @{ page = 'components'; file = 'components.png'; label = 'Wachiland base controls' }
        $screens += @{ page = 'components-dialog'; file = 'components-dialog.png'; label = 'Wachiland base dialog' }
    }
    foreach ($screen in $screens) {
        $env:WBOMBO_UI_CAPTURE_PAGE = $screen.page
        # The user requested visible UI captures; SW_HIDE prevents GPUI's first
        # window from becoming visible and makes MainWindowHandle remain zero.
        $proc = Start-Process -FilePath $portableExe -PassThru
        $deadline = (Get-Date).AddSeconds(45)
        while ((Get-Date) -lt $deadline -and ($proc.MainWindowHandle -eq 0 -or $proc.HasExited)) {
            Start-Sleep -Milliseconds 300
            $proc.Refresh()
        }
        if ($proc.HasExited -or $proc.MainWindowHandle -eq 0) { throw "Pandora did not open for $($screen.page)" }
        [void][CaptureWindow]::ShowWindow($proc.MainWindowHandle, 3)
        [void][CaptureWindow]::SetForegroundWindow($proc.MainWindowHandle)
        Start-Sleep -Seconds 5
        if ($HomeScreens -and $screen.page -in @('instances','accounts','home-menu','home-news')) {
            $log = Join-Path $tempRoot 'WachilandLauncher\launcher.log'
            $ready = "home-ready pid=$($proc.Id)"
            $deadline = (Get-Date).AddSeconds(120)
            while ((Get-Date) -lt $deadline -and -not (Select-String -LiteralPath $log -SimpleMatch $ready -Quiet -ErrorAction SilentlyContinue)) { Start-Sleep -Milliseconds 300 }
            if (-not (Select-String -LiteralPath $log -SimpleMatch $ready -Quiet -ErrorAction SilentlyContinue)) { throw 'Home presentation did not settle before capture.' }
        }
        if ($screen.page -eq 'home-news' -or $screen.page.StartsWith('news-')) {
            $log = Join-Path $tempRoot 'WachilandLauncher\launcher.log'
            $ready = "news-ready pid=$($proc.Id)"
            $deadline = (Get-Date).AddSeconds(120)
            while ((Get-Date) -lt $deadline -and -not (Select-String -LiteralPath $log -SimpleMatch $ready -Quiet -ErrorAction SilentlyContinue)) { Start-Sleep -Milliseconds 300 }
            if (-not (Select-String -LiteralPath $log -SimpleMatch $ready -Quiet -ErrorAction SilentlyContinue)) { throw 'News did not settle before capture.' }
            $ready = "news-media-ready pid=$($proc.Id)"
            $deadline = (Get-Date).AddSeconds(120)
            while ((Get-Date) -lt $deadline -and -not (Select-String -LiteralPath $log -SimpleMatch $ready -Quiet -ErrorAction SilentlyContinue)) { Start-Sleep -Milliseconds 300 }
            if (-not (Select-String -LiteralPath $log -SimpleMatch $ready -Quiet -ErrorAction SilentlyContinue)) { throw 'News media did not settle before capture.' }
        }
        if ($screen.page.StartsWith('update:')) {
            $ready = "update-dialog-ready pid=$($proc.Id)"
            $log = Join-Path $tempRoot 'WachilandLauncher\launcher.log'
            $deadline = (Get-Date).AddSeconds(45)
            while ((Get-Date) -lt $deadline -and -not (Select-String -LiteralPath $log -SimpleMatch $ready -Quiet -ErrorAction SilentlyContinue)) { Start-Sleep -Milliseconds 300 }
            if (-not (Select-String -LiteralPath $log -SimpleMatch $ready -Quiet -ErrorAction SilentlyContinue)) { throw 'No real update prompt was opened; repin the owned test profile before capturing.' }
        }
        Save-Window (Join-Path $outputPath $screen.file) $proc.MainWindowHandle $proc.Id
        if ($screen.page -eq 'components') {
            # Target the verified capture window, so a foreground change cannot
            # send a key to the user's editor or another application.
            if (-not [CaptureWindow]::PostMessage($proc.MainWindowHandle, 0x100, [IntPtr]9, [IntPtr]0x000f0001)) { throw 'Cannot send capture Tab keydown' }
            if (-not [CaptureWindow]::PostMessage($proc.MainWindowHandle, 0x101, [IntPtr]9, [IntPtr]0xc00f0001L)) { throw 'Cannot send capture Tab keyup' }
            Save-Window (Join-Path $outputPath 'components-focus.png') $proc.MainWindowHandle $proc.Id
        }
        $proc.CloseMainWindow() | Out-Null
        if (-not $proc.WaitForExit(5000)) { $proc.Kill(); $proc.WaitForExit() }
        Write-Output "CAPTURE_OK page=$($screen.page) file=$($screen.file)"
    }
    @{
        launcher = 'BootOptimPandora'
        viewport = if ($DesignViewport) { '1280 x 860 logical pixels' } else { 'maximized' }
        captures = $captureMetadata
        dataDirectory = if ($ownsDirectory) { 'temporary portable directory; removed after capture' } else { 'caller-owned isolated portable directory' }
        screens = @($screens | ForEach-Object { @{ page=$_.page; file=$_.file; note=$_.label } })
        unavailable = if ($TestProfile -or $PortableDirectory) { @() } else { @('instance-detail and update-dialog require an owned test profile fixture') }
    } | ConvertTo-Json -Depth 5 | Set-Content -Encoding utf8 (Join-Path $outputPath 'capture-manifest.json')
} finally {
    if ($null -ne $oldNewsFixture) { $env:WBOMBO_UI_CAPTURE_NEWS=$oldNewsFixture } else { Remove-Item Env:WBOMBO_UI_CAPTURE_NEWS -ErrorAction SilentlyContinue }
    if ($null -ne $proc -and -not $proc.HasExited) {
        $proc.CloseMainWindow() | Out-Null
        if (-not $proc.WaitForExit(5000)) { $proc.Kill(); $proc.WaitForExit() }
    }
    if ($null -ne $oldPage) { $env:WBOMBO_UI_CAPTURE_PAGE = $oldPage } else { Remove-Item Env:WBOMBO_UI_CAPTURE_PAGE -ErrorAction SilentlyContinue }
    if ($createdInstance) { Invoke-CaptureCli @('--delete-instance', $InstanceName, '--confirm', $InstanceName) }
    if ($ownsDirectory) {
        $resolvedTempRoot = [IO.Path]::GetFullPath($tempRoot)
        $allowedTemp = [IO.Path]::GetFullPath([IO.Path]::GetTempPath())
        if (-not $resolvedTempRoot.StartsWith($allowedTemp, [StringComparison]::OrdinalIgnoreCase) -or -not ([IO.Path]::GetFileName($resolvedTempRoot)).StartsWith('wb274-')) { throw 'Refusing cleanup outside capture temp directory' }
        Remove-Item -LiteralPath $resolvedTempRoot -Recurse -Force -ErrorAction SilentlyContinue
    }
    if ($oldDpiContext -ne [IntPtr]::Zero) { [void][CaptureWindow]::SetThreadDpiAwarenessContext($oldDpiContext) }
}
