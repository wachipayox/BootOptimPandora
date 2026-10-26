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
    [switch]$LauncherUpdateScreens,
    [string]$LauncherUpdateFixture,
    [string]$NewsFixture,
    [ValidateSet('advanced','version','expanded','editing')][string]$CaptureState,
    [string[]]$Pages,
    [switch]$SkipUpdateCapture,
    [switch]$DesignViewport,
    [int]$ViewportWidth = 1280,
    [int]$ViewportHeight = 860,
    [switch]$Framebuffer,
    [switch]$HitTest,
    [string]$HitTestReferenceDirectory,
    [switch]$DragRegions,
    [switch]$ResizeProbe,
    [string]$PortableDirectory
)
$ErrorActionPreference = 'Stop'
if (($HitTest -or $DragRegions -or $ResizeProbe) -and -not $Framebuffer) { throw 'Native diagnostics require -Framebuffer.' }
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
  [StructLayout(LayoutKind.Sequential)] public struct MINMAXINFO { public POINT reserved, maxSize, maxPosition, minTrackSize, maxTrackSize; }
  [DllImport("user32.dll", EntryPoint="SendMessageW")] private static extern IntPtr ReadMinMax(IntPtr hwnd, uint message, IntPtr wparam, ref MINMAXINFO info);
  public static MINMAXINFO GetMinMax(IntPtr hwnd) { var info=new MINMAXINFO(); ReadMinMax(hwnd,0x24,IntPtr.Zero,ref info); return info; }
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
  [DllImport("user32.dll")] public static extern bool GetCursorPos(out POINT point);
  [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
  [DllImport("user32.dll", SetLastError=true)] public static extern IntPtr SendMessageTimeout(IntPtr hwnd, uint message, IntPtr wparam, IntPtr lparam, uint flags, uint timeout, out IntPtr result);
  [DllImport("user32.dll", EntryPoint="GetWindowLongPtrW")] public static extern IntPtr GetWindowLongPtr(IntPtr hwnd, int index);
}
'@
Add-Type -AssemblyName System.Drawing
$oldDpiContext = [CaptureWindow]::SetThreadDpiAwarenessContext([IntPtr](-4))
$captureMetadata = @()
$captureDiagnostics = @()
$scriptHashAtStart = (Get-FileHash -LiteralPath $PSCommandPath -Algorithm SHA256).Hash.ToLowerInvariant()

function Save-FirstResizeFrames([string]$Stem, [IntPtr]$Handle) {
    $samples=@()
    foreach($dimensions in @(@(1100,740),@(960,600),@(1280,860),@(1000,700))) {
        foreach($name in @('frame.png','failed')) {
            $path=Join-Path $framebufferRoot $name
            if(Test-Path -LiteralPath $path){Remove-Item -LiteralPath $path}
        }
        $requestUtc=[DateTime]::UtcNow.ToString('o')
        [IO.File]::WriteAllText((Join-Path $framebufferRoot 'request'),(@{resize=$dimensions}|ConvertTo-Json -Compress))
        $deadline=(Get-Date).AddSeconds(20)
        $frame=Join-Path $framebufferRoot 'frame.png'
        while(-not(Test-Path -LiteralPath $frame) -and (Get-Date) -lt $deadline){
            if(Test-Path -LiteralPath (Join-Path $framebufferRoot 'failed')){throw 'First-resize-frame capture failed.'}
            Start-Sleep -Milliseconds 50
        }
        if(-not(Test-Path -LiteralPath $frame)){throw 'First-resize-frame capture timed out.'}
        $name="$Stem-first-resize-$($dimensions[0])x$($dimensions[1])-gpu.png"
        Copy-Item -LiteralPath $frame -Destination (Join-Path $outputPath $name)
        $samples+=@{file=$name;logicalSize=$dimensions;requestUtc=$requestUtc;copiedUtc=[DateTime]::UtcNow.ToString('o');method='first fresh draw armed before GPUI resize; GPU copy before Present'}
        # Give the old implementation time to prepare too. Each sample starts
        # with an available previous surface, not another unfinished sample.
        Start-Sleep -Seconds 2
    }
    $samples|ConvertTo-Json -Depth 5|Set-Content -Encoding utf8 (Join-Path $outputPath "$Stem-first-resize.json")
}

function Test-WindowHitRegions([string]$Path, [IntPtr]$Handle, [int]$ProcessId, [string]$RegionsPath) {
    [uint32]$owner=0
    [void][CaptureWindow]::GetWindowThreadProcessId($Handle,[ref]$owner)
    if ($owner -ne $ProcessId) { throw 'Hit-test window ownership changed.' }
    $regions=Get-Content -LiteralPath $RegionsPath -Raw | ConvertFrom-Json
    $origin=New-Object CaptureWindow+POINT
    [void][CaptureWindow]::ClientToScreen($Handle,[ref]$origin)
    $outer=New-Object CaptureWindow+RECT
    [void][CaptureWindow]::GetWindowRect($Handle,[ref]$outer)
    $scale=[CaptureWindow]::GetDpiForWindow($Handle)/96.0
    $points=@()
    $boundaryProbes=@()
    $resizable=([CaptureWindow]::GetWindowLongPtr($Handle,-16).ToInt64() -band 0x40000) -ne 0
    if($resizable) {
        $controlChecks=@()
        foreach($area in @('Some(Min)','Some(Max)','Some(Close)')) {
            $control=@($regions|Where-Object area -eq $area)
            # WindowControl uses size_8 (two rem), not a fixed pixel size.
            # A flex-row squeeze shortens width while height stays unchanged.
            $controlChecks+=@{area=$area;bounds=@($control|ForEach-Object bounds);passed=$control.Count -eq 1 -and $control[0].bounds.width -ge 16 -and [math]::Abs($control[0].bounds.width-$control[0].bounds.height) -lt 0.1}
        }
        $controlChecks|ConvertTo-Json -Depth 5|Set-Content -LiteralPath ($Path -replace '\.json$','-controls.json') -Encoding utf8
    }
    foreach($region in $regions) {
        $expected=switch($region.area) {'Some(Drag)' {2}; 'Some(Min)' {8}; 'Some(Max)' {9}; 'Some(Close)' {20}; 'None' {1}; default {throw 'Unknown capture region type'}}
        :probe foreach($point in $region.points) {
            if ($point -isnot [System.Array] -or $point.Count -ne 2) {throw 'A native hit-test probe must contain one x/y pair.'}
            # Require caption samples to be strictly away from a control's
            # one-physical-pixel boundary band. DPI rounding can otherwise
            # project a logical gap onto the first pixel of a button.
            if ($region.area -eq 'Some(Drag)') {
                $qx=[math]::Round($point[0]*$scale)/$scale
                $qy=[math]::Round($point[1]*$scale)/$scale
                $margin=1.0/$scale
                foreach($control in $regions | Where-Object area -ne 'Some(Drag)') {
                    $b=$control.bounds; $right=$b.x+$b.width; $bottom=$b.y+$b.height
                    $expanded=($qx -ge $b.x-$margin -and $qx -le $right+$margin -and $qy -ge $b.y-$margin -and $qy -le $bottom+$margin)
                    $interior=($qx -gt $b.x+$margin -and $qx -lt $right-$margin -and $qy -gt $b.y+$margin -and $qy -lt $bottom-$margin)
                    if ($expanded -and -not $interior) {
                        $boundaryProbes+=@{label=$region.label;x=$qx;y=$qy;near=$control.label;reason='one physical pixel from an interactive boundary; not queried'}
                        continue probe
                    }
                }
            }
            $points+=@{label=$region.label;kind=$region.area;x=$origin.X+[int][math]::Round($point[0]*$scale);y=$origin.Y+[int][math]::Round($point[1]*$scale);expected=$expected}
        }
    }
    $middleX=[int](($outer.Left+$outer.Right)/2)
    $middleY=[int](($outer.Top+$outer.Bottom)/2)
    foreach($edge in @(
        @('left',($outer.Left+1),$middleY,10), @('right',($outer.Right-2),$middleY,11),
        @('top',$middleX,($outer.Top+1),12), @('bottom',$middleX,($outer.Bottom-2),15),
        @('top-left',($outer.Left+1),($outer.Top+1),13), @('top-right',($outer.Right-2),($outer.Top+1),14),
        @('bottom-left',($outer.Left+1),($outer.Bottom-2),16), @('bottom-right',($outer.Right-2),($outer.Bottom-2),17)
    )) { $points+=@{label=$edge[0];kind='resize';x=$edge[1];y=$edge[2];expected=if($resizable){$edge[3]}else{$null}} }
    $previous=New-Object CaptureWindow+POINT
    [void][CaptureWindow]::GetCursorPos([ref]$previous)
    $results=@()
    try {
        foreach($point in $points) {
            if (-not [CaptureWindow]::SetCursorPos($point.x,$point.y)) {throw 'Cannot position the native hit-test probe.'}
            $clientX=$point.x-$origin.X; $clientY=$point.y-$origin.Y
            $client=[IntPtr](([int64]$clientY -shl 16) -bor ([int64]$clientX -band 0xffff))
            $screen=[IntPtr](([int64]$point.y -shl 16) -bor ([int64]$point.x -band 0xffff))
            $reply=[IntPtr]::Zero
            # Update hover identically for baseline and candidate, then query
            # the real HWND. No button-down/up or click messages are sent.
            if ([CaptureWindow]::SendMessageTimeout($Handle,0x200,[IntPtr]::Zero,$client,2,2000,[ref]$reply) -eq [IntPtr]::Zero) {throw 'Hit-test mouse position update timed out.'}
            if ([CaptureWindow]::SendMessageTimeout($Handle,0x84,[IntPtr]::Zero,$screen,2,2000,[ref]$reply) -eq [IntPtr]::Zero) {throw 'WM_NCHITTEST timed out.'}
            $passed=if($null -ne $point.expected){$reply.ToInt64() -eq $point.expected}else{$reply.ToInt64() -notin (10..17)}
            $results+=@{label=$point.label;kind=$point.kind;x=$point.x;y=$point.y;expected=$point.expected;actual=$reply.ToInt64();resizable=$resizable;passed=$passed}
        }
    } finally { [void][CaptureWindow]::SetCursorPos($previous.X,$previous.Y) }
    $results | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath $Path -Encoding utf8
    ConvertTo-Json -InputObject @($boundaryProbes) -Depth 5 | Set-Content -LiteralPath ($Path -replace '\.json$','-boundary.json') -Encoding utf8
    $results | ForEach-Object {[pscustomobject]$_} | Export-Csv -LiteralPath ($Path -replace '\.json$','.csv') -NoTypeInformation -Encoding utf8
    $failures=@($results|Where-Object {-not $_.passed}).Count
    Write-Output "HIT_TEST file=$([IO.Path]::GetFileName($Path)) points=$($results.Count) mismatches=$failures"
}

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
    if ($DesignViewport -and -not $script:fixedCaptureWindow) {
        $client = New-Object CaptureWindow+RECT
        [void][CaptureWindow]::GetClientRect($handle, [ref]$client)
        $scale = [CaptureWindow]::GetDpiForWindow($handle) / 96.0
        $frameWidth = ($rect.Right - $rect.Left) - ($client.Right - $client.Left)
        $frameHeight = ($rect.Bottom - $rect.Top) - ($client.Bottom - $client.Top)
        [void][CaptureWindow]::MoveWindow($handle, 32, 32, [int]($ViewportWidth * $scale + $frameWidth), [int]($ViewportHeight * $scale + $frameHeight), $true)
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
    $minMax=[CaptureWindow]::GetMinMax($handle)
    $minimumWidth=if($script:fixedCaptureWindow){400}else{480}
    $minimumHeight=if($script:fixedCaptureWindow){80}else{270}
    if ($width -lt $minimumWidth -or $height -lt $minimumHeight) { throw "Foreground window is not a Pandora window ($width x $height)" }
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
        $script:captureMetadata += @{ file=[IO.Path]::GetFileName($Path); width=$width; height=$height; dpi=[CaptureWindow]::GetDpiForWindow($handle); method='desktop'; nativeMinTrackPhysical=@($minMax.minTrackSize.X,$minMax.minTrackSize.Y) }
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
        if ($HitTest) {
            $stem=[IO.Path]::GetFileNameWithoutExtension($Path)
            $regionSource=if($HitTestReferenceDirectory){Join-Path $HitTestReferenceDirectory "$stem.regions.json"}else{Join-Path $framebufferRoot 'regions.json'}
            $regionTarget=Join-Path $outputPath "$stem.regions.json"
            Copy-Item -LiteralPath $regionSource -Destination $regionTarget -Force
            Test-WindowHitRegions (Join-Path $outputPath "$stem.hit-test.json") $handle $processId $regionTarget
        }
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
$oldDragRegions = $env:WBOMBO_UI_CAPTURE_DRAG_REGIONS
if ($DragRegions) {$env:WBOMBO_UI_CAPTURE_DRAG_REGIONS='1'} else {Remove-Item Env:WBOMBO_UI_CAPTURE_DRAG_REGIONS -ErrorAction SilentlyContinue}
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
$oldLauncherUpdateFixture = $env:WBOMBO_UI_CAPTURE_LAUNCHER_RELEASES
$screens = @(
    @{ page = 'instances'; file = 'instances.png'; label = 'Inicio con la instancia seleccionada o el fallback Vanilla' },
    @{ page = 'appearance'; file = 'settings-appearance.png'; label = 'Ajustes y apariencia' },
    @{ page = 'accounts'; file = 'accounts.png'; label = 'Menú de cuentas (sin iniciar sesión)' }
)
try {
    if ($LauncherUpdateFixture) { $env:WBOMBO_UI_CAPTURE_LAUNCHER_RELEASES = (Resolve-Path -LiteralPath $LauncherUpdateFixture).Path }
    if (($LauncherUpdateScreens -or @($Pages | Where-Object { $_ -like 'launcher-update*' }).Count -gt 0) -and -not $LauncherUpdateFixture) {
        throw 'Launcher update captures require an explicit private cosmetic fixture; never publish a release for capture.'
    }
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
    if ($LauncherUpdateScreens) {
        foreach ($updateRoute in @('launcher-update-indicator','launcher-update','launcher-update-expanded','launcher-update-progress','launcher-update-failed','launcher-update-checking','launcher-update-current','launcher-update-search-error','launcher-update-verification-error')) {
            $screens += @{page=$updateRoute;file="$updateRoute.png";label='Cosmetic local fixture only; installation is rejected by its fixed capture source marker'}
        }
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
        if ($screen.page -like 'launcher-update*') {
            $ready="launcher-update-ready pid=$($proc.Id)"
            $log=Join-Path $tempRoot 'WachilandLauncher\launcher.log'
            $deadline=(Get-Date).AddSeconds(45)
            while ((Get-Date) -lt $deadline -and -not (Select-String -LiteralPath $log -SimpleMatch $ready -Quiet -ErrorAction SilentlyContinue)) { Start-Sleep -Milliseconds 300 }
            if (-not (Select-String -LiteralPath $log -SimpleMatch $ready -Quiet -ErrorAction SilentlyContinue)) { throw 'Launcher update capture fixture did not become ready.' }
        }
        $updateHome = $screen.page -in @('launcher-update-indicator','launcher-update','launcher-update-expanded','launcher-update-progress','launcher-update-failed')
        if ($HomeScreens -and ($screen.page -in @('instances','accounts','home-menu','home-news') -or $updateHome)) {
            $log = Join-Path $tempRoot 'WachilandLauncher\launcher.log'
            $ready = "home-ready pid=$($proc.Id)"
            $deadline = (Get-Date).AddSeconds(120)
            while ((Get-Date) -lt $deadline -and -not (Select-String -LiteralPath $log -SimpleMatch $ready -Quiet -ErrorAction SilentlyContinue)) { Start-Sleep -Milliseconds 300 }
            if (-not (Select-String -LiteralPath $log -SimpleMatch $ready -Quiet -ErrorAction SilentlyContinue)) { throw 'Home presentation did not settle before capture.' }
            if ($screen.page -in @('instances','accounts','home-menu') -or $updateHome) {
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
        $captureHandle=$proc.MainWindowHandle
        $script:fixedCaptureWindow=$screen.page -eq 'window-dialog'
        if ($script:fixedCaptureWindow) {
            $ready="window-dialog-ready pid=$($proc.Id)"
            $log=Join-Path $tempRoot 'WachilandLauncher\launcher.log'
            $deadline=(Get-Date).AddSeconds(30)
            while((Get-Date) -lt $deadline -and -not(Select-String -LiteralPath $log -SimpleMatch $ready -Quiet -ErrorAction SilentlyContinue)){Start-Sleep -Milliseconds 100}
            if(-not(Select-String -LiteralPath $log -SimpleMatch $ready -Quiet -ErrorAction SilentlyContinue)){throw 'Standalone dialog did not become ready.'}
            $captureHandle=[CaptureWindow]::GetForegroundWindow()
            [uint32]$dialogOwner=0
            [void][CaptureWindow]::GetWindowThreadProcessId($captureHandle,[ref]$dialogOwner)
            if($dialogOwner -ne $proc.Id -or ([CaptureWindow]::GetWindowLongPtr($captureHandle,-16).ToInt64() -band 0x40000) -ne 0){throw 'Standalone dialog HWND is not the active owned nonresizable window.'}
        }
        Save-Window (Join-Path $outputPath $screen.file) $captureHandle $proc.Id
        if ($ResizeProbe -and -not $script:fixedCaptureWindow) {
            Save-FirstResizeFrames ([IO.Path]::GetFileNameWithoutExtension($screen.file)) $captureHandle
        }
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
        if ($LauncherScreens -or $screen.page.StartsWith('launcher-') -or $screen.page -in @('skins','skins-url','skins-copy','skins-import','account-offline','add-instance','add-server','add-local') -or $screen.page -in @('components','home-news','window-dialog') -or $screen.page -match '^(instance-|derived:|duplicate:|update:|delete:|export:|groups:)') {
            # Target the verified capture window, so a foreground change cannot
            # send a key to the user's editor or another application.
            if (-not [CaptureWindow]::PostMessage($captureHandle, 0x100, [IntPtr]9, [IntPtr]0x000f0001)) { throw 'Cannot send capture Tab keydown' }
            if (-not [CaptureWindow]::PostMessage($captureHandle, 0x101, [IntPtr]9, [IntPtr]0xc00f0001L)) { throw 'Cannot send capture Tab keyup' }
            Save-Window (Join-Path $outputPath (($screen.file -replace '\.png$', '') + '-focus.png')) $captureHandle $proc.Id
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
        launcherUpdateFixtureSha256 = if ($LauncherUpdateFixture) { (Get-FileHash -LiteralPath $LauncherUpdateFixture -Algorithm SHA256).Hash.ToLowerInvariant() } else { $null }
        captureScriptCommit = (git -C $repo rev-parse HEAD)
        captureScriptSha256 = $scriptHashAtStart
        nativeHitTest = [bool]$HitTest
        hitTestRegionSource = if($HitTestReferenceDirectory){'candidate geometry reused for baseline HWND queries'}else{'painted native regions from this executable'}
        diagnosticDragColors = [bool]$DragRegions
        viewport = if ($DesignViewport) { "$ViewportWidth x $ViewportHeight logical pixels" } else { 'maximized' }
        captures = $captureMetadata
        diagnostics = $captureDiagnostics
        dataDirectory = if ($ownsDirectory) { 'temporary portable directory; removed after capture' } else { 'caller-owned isolated portable directory' }
        screens = @($screens | ForEach-Object { @{ page=$_.page; file=$_.file; note=$_.label } })
        unavailable = if ($TestProfile -or $PortableDirectory) { @() } else { @('instance-detail and update-dialog require an owned test profile fixture') }
    } | ConvertTo-Json -Depth 5 | Set-Content -Encoding utf8 (Join-Path $outputPath 'capture-manifest.json')
} finally {
    if ($null -ne $oldDragRegions) { $env:WBOMBO_UI_CAPTURE_DRAG_REGIONS=$oldDragRegions } else { Remove-Item Env:WBOMBO_UI_CAPTURE_DRAG_REGIONS -ErrorAction SilentlyContinue }
    if ($null -ne $oldLauncherUpdateFixture) { $env:WBOMBO_UI_CAPTURE_LAUNCHER_RELEASES=$oldLauncherUpdateFixture } else { Remove-Item Env:WBOMBO_UI_CAPTURE_LAUNCHER_RELEASES -ErrorAction SilentlyContinue }
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
