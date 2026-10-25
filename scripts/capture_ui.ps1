param(
    [string]$Output = (Join-Path $PSScriptRoot '..\..\..\coordination\272-estetica-launchers\pandora'),
    [switch]$SkipBuild,
    [string]$Executable,
    [ValidatePattern('^$|^[0-9a-fA-F]{7,40}$')][string]$ExecutableSourceCommit = '',
    [string]$TestProfile,
    [string]$Revision,
    [string]$InstanceName = 'agente-274-ui',
    [switch]$Components,
    [switch]$HomeScreens,
    [switch]$NewsScreens,
    [switch]$InstanceScreens,
    [switch]$LauncherScreens,
    [string]$NewsFixture,
    [ValidateSet('advanced','version','expanded','editing')][string]$CaptureState,
    [string[]]$Pages,
    [switch]$SkipUpdateCapture,
    [switch]$DesignViewport,
    [switch]$Framebuffer,
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
$framebufferRoot = Join-Path $tempRoot 'capture-framebuffer'
if ($Framebuffer) { $null = New-Item -ItemType Directory -Force -Path $framebufferRoot }
# Pandora treats an executable whose name contains "portable" as an isolated
# portable install, so this run never resolves to the user's regular data folder.
$portableExe = Join-Path $tempRoot 'pandora-portable.exe'
if (-not [string]::Equals($builtExe, $portableExe, [StringComparison]::OrdinalIgnoreCase)) {
    Copy-Item -LiteralPath $builtExe -Destination $portableExe -Force
}
$executableHash = (Get-FileHash -LiteralPath $portableExe -Algorithm SHA256).Hash.ToLowerInvariant()

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
$captureDiagnostics = @()

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
        $script:captureMetadata += @{ file=[IO.Path]::GetFileName($Path); width=$width; height=$height; dpi=[CaptureWindow]::GetDpiForWindow($handle); method='desktop' }
    } finally { $graphics.Dispose(); $bitmap.Dispose() }
    if ($Framebuffer) {
        foreach ($name in @('frame.png', 'failed')) {
            $previous = Join-Path $framebufferRoot $name
            if (Test-Path -LiteralPath $previous) { Remove-Item -LiteralPath $previous }
        }
        [IO.File]::WriteAllText((Join-Path $framebufferRoot 'request'), '')
        $deadline = (Get-Date).AddSeconds(15)
        $frame = Join-Path $framebufferRoot 'frame.png'
        while (-not (Test-Path -LiteralPath $frame) -and (Get-Date) -lt $deadline) {
            if (Test-Path -LiteralPath (Join-Path $framebufferRoot 'failed')) { throw 'Native framebuffer readback failed; inspect the isolated capture directory.' }
            Start-Sleep -Milliseconds 100
        }
        if (-not (Test-Path -LiteralPath $frame)) { throw 'Native framebuffer timed out; build with --features ui-capture.' }
        $nativePath = [IO.Path]::Combine([IO.Path]::GetDirectoryName($Path), ([IO.Path]::GetFileNameWithoutExtension($Path) + '-gpu.png'))
        Copy-Item -LiteralPath $frame -Destination $nativePath
        $native = [Drawing.Bitmap]::new($nativePath)
        try {
            if ($native.Width -ne $width -or $native.Height -ne $height) { throw 'Native framebuffer and client dimensions differ.' }
        } finally { $native.Dispose() }
        $script:captureMetadata += @{file=[IO.Path]::GetFileName($nativePath);width=$width;height=$height;dpi=[CaptureWindow]::GetDpiForWindow($handle);method='gpu-readback'}
    }
}

function Close-CapturedProcess([System.Diagnostics.Process]$CapturedProcess) {
    if ($CapturedProcess.HasExited) { return }
    try {
        $CapturedProcess.CloseMainWindow() | Out-Null
        if (-not $CapturedProcess.WaitForExit(5000)) {
            $CapturedProcess.Refresh()
            if (-not $CapturedProcess.HasExited) { $CapturedProcess.Kill() }
            $CapturedProcess.WaitForExit()
        }
    } catch {
        # Windows can deny Kill when the process exits between WaitForExit and
        # Kill. Suppress only a confirmed exit; a still-running child is an error.
        $CapturedProcess.Refresh()
        if (-not $CapturedProcess.HasExited) { throw }
    }
}

function Invoke-CaptureCli([string[]]$Arguments) {
    $cli = Start-Process -FilePath $portableExe -ArgumentList $Arguments -WindowStyle Hidden -Wait -PassThru -RedirectStandardOutput (Join-Path $tempRoot 'capture-cli.out') -RedirectStandardError (Join-Path $tempRoot 'capture-cli.err')
    if ($cli.ExitCode -ne 0) { throw "Capture CLI failed ($($cli.ExitCode)); inspect isolated capture-cli files." }
}
$oldNewsFixture = $env:WBOMBO_UI_CAPTURE_NEWS
if ($NewsFixture) { $env:WBOMBO_UI_CAPTURE_NEWS = (Resolve-Path -LiteralPath $NewsFixture).Path }
$createdInstance = $false
$distributionCaptureActive = $false
$distributionOriginalConfig = $null
$distributionConfigPath = Join-Path $tempRoot "WachilandLauncher\config.json"
$oldPage = $env:WBOMBO_UI_CAPTURE_PAGE
$oldFramebufferDirectory = $env:WBOMBO_UI_CAPTURE_FRAMEBUFFER_DIR
if ($Framebuffer) { $env:WBOMBO_UI_CAPTURE_FRAMEBUFFER_DIR = $framebufferRoot }
$oldCaptureState = $env:WBOMBO_UI_CAPTURE_STATE
if ($CaptureState) { $env:WBOMBO_UI_CAPTURE_STATE = $CaptureState }
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
    if ($InstanceScreens) {
        if (-not ($TestProfile -or $PortableDirectory) -or -not $InstanceName.StartsWith('agente-274-')) { throw 'Instance screenshots require an owned agent-274 CLI fixture.' }
        foreach ($instanceRoute in @('instance-mods','instance-resources','instance-shaders','instance-overwrites','instance-logs','instance-crashes','instance-live','instance-changelog','instance-settings','derived','duplicate')) {
            $screens += @{page="$($instanceRoute):$InstanceName";file="$instanceRoute.png";label='Owned instance screen; live output is synthetic and never starts Minecraft'}
        }
    }
    if ($LauncherScreens) {
        foreach ($launcherRoute in @('launcher-general','launcher-appearance','launcher-theme-joke','launcher-network','launcher-distribution','launcher-ignored','launcher-windows','launcher-about','skins','skins-url','skins-copy','skins-import','launcher-defender-dialog','accounts','account-offline','add-instance','add-server','add-local','import','syncing')) {
            $screens += @{page=$launcherRoute;file="$launcherRoute.png";label='Native launcher screen or existing dialog; no action is confirmed'}
        }
        if ($TestProfile -or $PortableDirectory) { $screens += @{page="content-browser:$InstanceName";file='content-browser.png';label='Browse content for owned fixture; no install is started'} }
    }
    if ($Pages) { $screens = @($Pages | ForEach-Object { @{ page=$_; file=(($_ -replace ':','-') + '.png'); label='Explicit native capture route' } }) }
    if ($Components) {
        $screens += @{ page = 'components'; file = 'components.png'; label = 'Wachiland base controls' }
        $screens += @{ page = 'components-dialog'; file = 'components-dialog.png'; label = 'Wachiland base dialog' }
    }
    foreach ($screen in $screens) {
        $env:WBOMBO_UI_CAPTURE_PAGE = $screen.page
        if ($screen.page -eq 'launcher-distribution') {
            # Even a fresh private build embeds its endpoint: never capture it.
            $distributionOriginalConfig = if (Test-Path -LiteralPath $distributionConfigPath) { [IO.File]::ReadAllBytes($distributionConfigPath) } else { $null }
            $captureConfig = if ($null -ne $distributionOriginalConfig) { [Text.Encoding]::UTF8.GetString($distributionOriginalConfig) | ConvertFrom-Json } else { [pscustomobject]@{} }
            $captureConfig | Add-Member -NotePropertyName distribution -NotePropertyValue ([pscustomobject]@{base_url='https://example.invalid';tls_ca_certificate_path=''}) -Force
            $null = New-Item -ItemType Directory -Force -Path (Split-Path -Parent $distributionConfigPath)
            $distributionCaptureActive = $true
            [IO.File]::WriteAllText($distributionConfigPath, ($captureConfig | ConvertTo-Json -Depth 30), [Text.UTF8Encoding]::new($false))
        }
        # The user requested visible UI captures; SW_HIDE prevents GPUI's first
        # window from becoming visible and makes MainWindowHandle remain zero.
        $proc = Start-Process -FilePath $portableExe -PassThru -RedirectStandardOutput (Join-Path $tempRoot 'capture-window.out') -RedirectStandardError (Join-Path $tempRoot 'capture-window.err')
        $deadline = (Get-Date).AddSeconds(45)
        while ((Get-Date) -lt $deadline -and -not $proc.HasExited -and $proc.MainWindowHandle -eq 0) {
            Start-Sleep -Milliseconds 300
            $proc.Refresh()
        }
        if ($proc.HasExited) { throw "Pandora exited for $($screen.page), code $($proc.ExitCode); see capture-window.err in the isolated portable directory" }
        if ($proc.MainWindowHandle -eq 0) { throw "Pandora did not open for $($screen.page)" }
        [void][CaptureWindow]::ShowWindow($proc.MainWindowHandle, 3)
        [void][CaptureWindow]::SetForegroundWindow($proc.MainWindowHandle)
        Start-Sleep -Seconds 5
        $proc.Refresh()
        if ($proc.HasExited) { throw "Pandora exited for $($screen.page), code $($proc.ExitCode); see capture-window.err in the isolated portable directory" }
        if ($HomeScreens -and $screen.page -in @('instances','accounts','home-menu','home-news')) {
            $log = Join-Path $tempRoot 'WachilandLauncher\launcher.log'
            $ready = "home-ready pid=$($proc.Id)"
            $deadline = (Get-Date).AddSeconds(120)
            while ((Get-Date) -lt $deadline -and -not (Select-String -LiteralPath $log -SimpleMatch $ready -Quiet -ErrorAction SilentlyContinue)) { Start-Sleep -Milliseconds 300 }
            if (-not (Select-String -LiteralPath $log -SimpleMatch $ready -Quiet -ErrorAction SilentlyContinue)) { throw 'Home presentation did not settle before capture.' }
            if ($screen.page -in @('instances','accounts','home-menu')) {
                $ready = "glass-ready pid=$($proc.Id)"
                $deadline = (Get-Date).AddSeconds(60)
                while ((Get-Date) -lt $deadline -and -not (Select-String -LiteralPath $log -SimpleMatch $ready -Quiet -ErrorAction SilentlyContinue)) { Start-Sleep -Milliseconds 300 }
                if (-not (Select-String -LiteralPath $log -SimpleMatch $ready -Quiet -ErrorAction SilentlyContinue)) { throw 'Glass backdrop did not settle before capture.' }
            }
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
        if ($screen.page -eq 'news-changelog') {
            $ready = "news-changelog-ready pid=$($proc.Id)"
            $log = Join-Path $tempRoot 'WachilandLauncher\launcher.log'
            $deadline = (Get-Date).AddSeconds(120)
            while ((Get-Date) -lt $deadline -and -not (Select-String -LiteralPath $log -SimpleMatch $ready -Quiet -ErrorAction SilentlyContinue)) { Start-Sleep -Milliseconds 300 }
            if (-not (Select-String -LiteralPath $log -SimpleMatch $ready -Quiet -ErrorAction SilentlyContinue)) { throw 'News changelog did not settle before capture.' }
        }
        $instanceReadyMarkers = @()
        if ($screen.page -match '^instance-(mods|resources|shaders):') { $instanceReadyMarkers=@('instance-content-ready','instance-content-origin-ready') }
        if ($screen.page -match '^(instance-settings|derived|duplicate):') { $instanceReadyMarkers=@('instance-settings-profile-ready','instance-settings-group-ready','instance-artwork-ready','instance-memory-ready') }
        if ($screen.page -match '^(derived|duplicate):') { $instanceReadyMarkers += 'instance-derived-ready' }
        if ($screen.page -match '^instance-overwrites:') { $instanceReadyMarkers=@('instance-overwrites-ready') }
        if ($screen.page -match '^instance-(logs|crashes):') { $instanceReadyMarkers=@('instance-log-ready') }
        if ($screen.page -match '^instance-changelog:') { $instanceReadyMarkers=@('instance-changelog-ready','instance-stone-ready') }
        if ($screen.page -match '^instance-live:') { $instanceReadyMarkers=@('instance-live-fixture-ready') }
        if ($screen.page.StartsWith('update:')) { $instanceReadyMarkers=@('instance-update-changelog-ready') }
        foreach ($instanceMarker in $instanceReadyMarkers) {
            $ready="$instanceMarker pid=$($proc.Id)"
            $log=Join-Path $tempRoot 'WachilandLauncher\launcher.log'
            $deadline=(Get-Date).AddSeconds(120)
            while ((Get-Date) -lt $deadline -and -not (Select-String -LiteralPath $log -SimpleMatch $ready -Quiet -ErrorAction SilentlyContinue)) { if ($proc.HasExited) { throw "Capture process exited before $instanceMarker" };Start-Sleep -Milliseconds 300 }
            if (-not (Select-String -LiteralPath $log -SimpleMatch $ready -Quiet -ErrorAction SilentlyContinue)) { throw "Instance capture did not settle: $instanceMarker" }
        }
        Save-Window (Join-Path $outputPath $screen.file) $proc.MainWindowHandle $proc.Id
        if ($screen.page.StartsWith('news-article')) {
            # Wheel packets target only the verified capture window. They never
            # move the pointer or send input to another foreground application.
            $newsClient = New-Object CaptureWindow+RECT
            $newsOrigin = New-Object CaptureWindow+POINT
            [void][CaptureWindow]::GetClientRect($proc.MainWindowHandle,[ref]$newsClient)
            [void][CaptureWindow]::ClientToScreen($proc.MainWindowHandle,[ref]$newsOrigin)
            $newsX = $newsOrigin.X + [int]($newsClient.Right * 0.7)
            $newsY = $newsOrigin.Y + [int]($newsClient.Bottom * 0.7)
            $newsPoint = [IntPtr](([int64]$newsY -shl 16) -bor ([int64]$newsX -band 0xffff))
            foreach ($part in @('body','media')) {
                for ($wheel = 0; $wheel -lt 8; $wheel++) {
                    if (-not [CaptureWindow]::PostMessage($proc.MainWindowHandle,0x20a,[IntPtr](-7864320),$newsPoint)) { throw 'Cannot scroll the article capture window' }
                }
                Save-Window (Join-Path $outputPath (($screen.file -replace '\.png$', '') + "-$part.png")) $proc.MainWindowHandle $proc.Id
            }
        }
        if ($screen.page -match '^instance-(settings|changelog):') {
            $instanceClient = New-Object CaptureWindow+RECT
            $instanceOrigin = New-Object CaptureWindow+POINT
            [void][CaptureWindow]::GetClientRect($proc.MainWindowHandle,[ref]$instanceClient)
            [void][CaptureWindow]::ClientToScreen($proc.MainWindowHandle,[ref]$instanceOrigin)
            $instanceX = $instanceOrigin.X + [int]($instanceClient.Right * 0.5)
            $instanceY = $instanceOrigin.Y + [int]($instanceClient.Bottom * 0.7)
            $instancePoint = [IntPtr](([int64]$instanceY -shl 16) -bor ([int64]$instanceX -band 0xffff))
            foreach ($part in @('middle','bottom')) {
                for ($wheel = 0; $wheel -lt 8; $wheel++) {
                    if (-not [CaptureWindow]::PostMessage($proc.MainWindowHandle,0x20a,[IntPtr](-7864320),$instancePoint)) { throw 'Cannot scroll the instance capture window' }
                }
                Save-Window (Join-Path $outputPath (($screen.file -replace '\.png$', '') + "-$part.png")) $proc.MainWindowHandle $proc.Id
            }
        }
        if ($LauncherScreens -or $screen.page.StartsWith('launcher-') -or $screen.page -in @('skins','skins-url','skins-copy','skins-import','account-offline','add-instance','add-server','add-local') -or $screen.page -in @('components','home-news') -or $screen.page -match '^(instance-|derived:|duplicate:|update:|delete:|export:|groups:)') {
            # Target the verified capture window, so a foreground change cannot
            # send a key to the user's editor or another application.
            if (-not [CaptureWindow]::PostMessage($proc.MainWindowHandle, 0x100, [IntPtr]9, [IntPtr]0x000f0001)) { throw 'Cannot send capture Tab keydown' }
            if (-not [CaptureWindow]::PostMessage($proc.MainWindowHandle, 0x101, [IntPtr]9, [IntPtr]0xc00f0001L)) { throw 'Cannot send capture Tab keyup' }
            Save-Window (Join-Path $outputPath (($screen.file -replace '\.png$', '') + '-focus.png')) $proc.MainWindowHandle $proc.Id
        }
        $newsLogPath = Join-Path $tempRoot 'WachilandLauncher\launcher.log'
        $errorsBeforeClose = @(Select-String -LiteralPath $newsLogPath -Pattern ' ERROR ' -ErrorAction SilentlyContinue).Count
        Close-CapturedProcess $proc
        if ($distributionCaptureActive) {
            if ($null -ne $distributionOriginalConfig) { [IO.File]::WriteAllBytes($distributionConfigPath, $distributionOriginalConfig) } else { Remove-Item -LiteralPath $distributionConfigPath -ErrorAction SilentlyContinue }
            $distributionCaptureActive = $false
        }
        $captureDiagnostics += @{page=$screen.page;errorsBeforeClose=$errorsBeforeClose;errorsAfterClose=@(Select-String -LiteralPath $newsLogPath -Pattern ' ERROR ' -ErrorAction SilentlyContinue).Count}
        Write-Output "CAPTURE_OK page=$($screen.page) file=$($screen.file)"
    }
    @{
        launcher = 'BootOptimPandora'
        executableSha256 = $executableHash
        executableSourceCommit = $ExecutableSourceCommit
        captureScriptCommit = (git -C $repo rev-parse HEAD)
        viewport = if ($DesignViewport) { '1280 x 860 logical pixels' } else { 'maximized' }
        captures = $captureMetadata
        diagnostics = $captureDiagnostics
        dataDirectory = if ($ownsDirectory) { 'temporary portable directory; removed after capture' } else { 'caller-owned isolated portable directory' }
        screens = @($screens | ForEach-Object { @{ page=$_.page; file=$_.file; note=$_.label } })
        unavailable = if ($TestProfile -or $PortableDirectory) { @() } else { @('instance-detail and update-dialog require an owned test profile fixture') }
    } | ConvertTo-Json -Depth 5 | Set-Content -Encoding utf8 (Join-Path $outputPath 'capture-manifest.json')
} finally {
    if ($null -ne $oldFramebufferDirectory) { $env:WBOMBO_UI_CAPTURE_FRAMEBUFFER_DIR=$oldFramebufferDirectory } else { Remove-Item Env:WBOMBO_UI_CAPTURE_FRAMEBUFFER_DIR -ErrorAction SilentlyContinue }
    if ($null -ne $oldNewsFixture) { $env:WBOMBO_UI_CAPTURE_NEWS=$oldNewsFixture } else { Remove-Item Env:WBOMBO_UI_CAPTURE_NEWS -ErrorAction SilentlyContinue }
    if ($null -ne $proc -and -not $proc.HasExited) {
        Close-CapturedProcess $proc
    }
    if ($distributionCaptureActive) {
        if ($null -ne $distributionOriginalConfig) { [IO.File]::WriteAllBytes($distributionConfigPath, $distributionOriginalConfig) } else { Remove-Item -LiteralPath $distributionConfigPath -ErrorAction SilentlyContinue }
    }
    if ($null -ne $oldCaptureState) { $env:WBOMBO_UI_CAPTURE_STATE=$oldCaptureState } else { Remove-Item Env:WBOMBO_UI_CAPTURE_STATE -ErrorAction SilentlyContinue }
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
