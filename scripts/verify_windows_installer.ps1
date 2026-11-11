<#
.SYNOPSIS
  Install, reinstall over and uninstall the Windows installer silently against a
  local test Distribution, then check files, registry, shortcuts, PE metadata and
  that every failed download leaves the machine as it was.

.DESCRIPTION
  The installer downloads the launcher (crates/setup_fetch). Here it is pointed
  at scripts/test_launcher_release_server.py on 127.0.0.1 through
  WBOMBO_SETUP_TEST_ORIGIN and WBOMBO_SETUP_TEST_CA; that server publishes
  -Launcher as a synthetic release signed with a throwaway key. Needs Python
  with the `cryptography` package.

  Meant for disposable CI runners: it installs for the current user. Launcher
  data (%APPDATA%\WachilandLauncher) must not be created or removed by the
  installer; the script checks that too. Unsigned files only produce a warning.
#>
param(
    [Parameter(Mandatory = $true)][string]$Installer,
    [Parameter(Mandatory = $true)][string]$Launcher,
    [string]$Python = 'python'
)
$ErrorActionPreference = 'Stop'
$product = 'Wachiland Launcher'
$installDir = Join-Path $env:LOCALAPPDATA "Programs\$product"
$exe = Join-Path $installDir 'WachilandLauncher.exe'
$uninstaller = Join-Path $installDir 'uninstall.exe'
$uninstallKey = "HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\$product"
$startMenu = Join-Path ([Environment]::GetFolderPath('Programs')) "$product.lnk"
$desktop = Join-Path ([Environment]::GetFolderPath('Desktop')) "$product.lnk"
$data = Join-Path $env:APPDATA 'WachilandLauncher'
$serverScript = Join-Path $PSScriptRoot 'test_launcher_release_server.py'
$Installer = (Resolve-Path -LiteralPath $Installer).Path
$Launcher = (Resolve-Path -LiteralPath $Launcher).Path

function Check($condition, $message) {
    if (-not $condition) { throw "FALLO: $message" }
    Write-Host "ok: $message"
}

# Starts the test server and points the installer at it; returns the process.
function Start-TestServer([string]$mode, [string]$version) {
    $ready = Join-Path ([IO.Path]::GetTempPath()) ("wachiland-setup-ready-" + [Guid]::NewGuid() + '.json')
    $process = Start-Process -FilePath $Python -PassThru -WindowStyle Hidden -ArgumentList @(
        "`"$serverScript`"", '--artifact', "`"$Launcher`"", '--version', $version, '--mode', $mode,
        '--ready-file', "`"$ready`"")
    for ($i = 0; $i -lt 100 -and -not (Test-Path -LiteralPath $ready); $i++) { Start-Sleep -Milliseconds 200 }
    if (-not (Test-Path -LiteralPath $ready)) { Stop-Process -Id $process.Id -Force; throw "el servidor de prueba ($mode) no arrancó" }
    $info = Get-Content -LiteralPath $ready -Raw | ConvertFrom-Json
    Remove-Item -LiteralPath $ready
    $env:WBOMBO_SETUP_TEST_ORIGIN = $info.origin
    $env:WBOMBO_SETUP_TEST_CA = $info.ca
    return $process
}

function Install([string]$label) {
    $p = Start-Process -FilePath $Installer -ArgumentList '/S' -Wait -PassThru
    Write-Host "$label -> código $($p.ExitCode)"
    return $p.ExitCode
}

# Runs a silent install whose download must fail, and checks nothing changed.
function Expect-Refused([string]$label, [string]$mode, [string]$version) {
    $before = if (Test-Path -LiteralPath $exe) { (Get-FileHash -LiteralPath $exe).Hash } else { $null }
    $displayVersion = (Get-ItemProperty -LiteralPath $uninstallKey -ErrorAction SilentlyContinue).DisplayVersion
    $server = if ($mode -eq 'offline') { $null } else { Start-TestServer $mode $version }
    if ($mode -eq 'offline') {
        # Nothing listens on port 1.
        $env:WBOMBO_SETUP_TEST_ORIGIN = 'https://127.0.0.1:1'
        Remove-Item Env:\WBOMBO_SETUP_TEST_CA -ErrorAction SilentlyContinue
    }
    try {
        Check ((Install $label) -ne 0) "$label termina con error"
    } finally {
        if ($server) { Stop-Process -Id $server.Id -Force }
    }
    if ($before) {
        Check ((Get-FileHash -LiteralPath $exe).Hash -eq $before) "${label}: el launcher instalado no cambia"
        Check ((Get-ItemProperty -LiteralPath $uninstallKey).DisplayVersion -eq $displayVersion) "${label}: la versión registrada no cambia"
        Check (-not (Get-ChildItem -LiteralPath $installDir -Filter '*.wbombo-*' -ErrorAction SilentlyContinue)) "${label}: no deja descargas a medias"
    } else {
        Check (-not (Test-Path -LiteralPath $installDir)) "${label}: no crea la carpeta de instalación"
        Check (-not (Test-Path -LiteralPath $uninstallKey)) "${label}: no registra nada"
        Check (-not (Test-Path -LiteralPath $startMenu) -and -not (Test-Path -LiteralPath $desktop)) "${label}: no crea accesos directos"
    }
}

foreach ($file in $Installer, $Launcher) {
    $info = (Get-Item -LiteralPath $file).VersionInfo
    Check ($info.ProductName -eq $product) "$([IO.Path]::GetFileName($file)) ProductName = $($info.ProductName)"
    Check ($info.CompanyName -eq 'Wachiland Elite') "$([IO.Path]::GetFileName($file)) CompanyName = $($info.CompanyName)"
    $signature = Get-AuthenticodeSignature -LiteralPath $file
    if ($signature.Status -eq 'Valid') {
        Write-Host "ok: $([IO.Path]::GetFileName($file)) firmado por $($signature.SignerCertificate.Subject)"
    } else {
        Write-Warning "$([IO.Path]::GetFileName($file)) sin firma válida ($($signature.Status))"
    }
}
Check ((Get-Item -LiteralPath $Launcher).VersionInfo.OriginalFilename -eq 'WachilandLauncher.exe') 'OriginalFilename del launcher'
$setupBytes = (Get-Item -LiteralPath $Installer).Length
Check ($setupBytes -lt 10000000) "el instalador ocupa $setupBytes bytes (menos de 10 MB, el límite de Discord sin Nitro)"

# Served versions start at the launcher's own, so the installer never sees a newer one installed.
$base = [Version](((Get-Item -LiteralPath $Launcher).VersionInfo.ProductVersion -split '[-+]')[0])
$first = "$($base.Major).$($base.Minor).$($base.Build)"
$second = "$($base.Major).$($base.Minor).$($base.Build + 1)"
$third = "$($base.Major).$($base.Minor).$($base.Build + 2)"
# Below the installed exe's own file version: the setup must not downgrade it.
$older = if ($base.Build -gt 0) { "$($base.Major).$($base.Minor).$($base.Build - 1)" }
         elseif ($base.Minor -gt 0) { "$($base.Major).$($base.Minor - 1).0" }
         else { "$($base.Major - 1).0.0" }

$dataExisted = Test-Path -LiteralPath $data
Check (-not (Test-Path -LiteralPath $installDir)) 'no había una instalación previa'

try {
    Expect-Refused 'sin conexión, instalación limpia' 'offline' $first
    Expect-Refused 'sin versión publicada, instalación limpia' 'no-release' $first
    Expect-Refused 'firma inválida, instalación limpia' 'bad-signature' $first

    foreach ($round in @(@('instalación limpia', $first), @('reinstalación encima con una versión nueva', $second))) {
        $label, $version = $round
        $server = Start-TestServer 'ok' $version
        try {
            Check ((Install $label) -eq 0) "$label termina con código 0"
        } finally {
            Stop-Process -Id $server.Id -Force
        }
        Check (Test-Path -LiteralPath $exe) "$label deja WachilandLauncher.exe"
        Check ((Get-FileHash -LiteralPath $exe).Hash -eq (Get-FileHash -LiteralPath $Launcher).Hash) "$label instala el exe publicado"
        Check (-not (Get-ChildItem -LiteralPath $installDir -Filter '*.wbombo-*')) "$label no deja temporales"
        Check (Test-Path -LiteralPath $uninstaller) "$label deja uninstall.exe"
        $key = Get-ItemProperty -LiteralPath $uninstallKey
        Check ($key.DisplayName -eq $product -and $key.Publisher -eq 'Wachiland Elite') "$label registra la entrada de Aplicaciones instaladas"
        Check ($key.DisplayVersion -eq $version) "$label registra la versión descargada ($version)"
        Check ($key.UninstallString -eq "`"$uninstaller`"") "$label registra el desinstalador"
        Check (Test-Path -LiteralPath $startMenu) "$label crea el acceso del menú Inicio"
        Check (Test-Path -LiteralPath $desktop) "$label crea el acceso del escritorio"
        $link = (New-Object -ComObject WScript.Shell).CreateShortcut($startMenu)
        Check ($link.TargetPath -eq $exe) "${label}: el acceso apunta al exe instalado"
    }

    Expect-Refused 'sin conexión, encima de una instalación' 'offline' $third
    Expect-Refused 'firma inválida, encima de una instalación' 'bad-signature' $third
    Expect-Refused 'descarga alterada, encima de una instalación' 'bad-hash' $third
    Expect-Refused 'versión publicada más antigua que la instalada' 'ok' $older
} finally {
    Remove-Item Env:\WBOMBO_SETUP_TEST_ORIGIN, Env:\WBOMBO_SETUP_TEST_CA -ErrorAction SilentlyContinue
}
Check ((Test-Path -LiteralPath $data) -eq $dataExisted) 'el instalador no crea ni borra los datos del launcher'

# Leftovers of the in-place updater must go with the uninstall.
Set-Content -LiteralPath "$exe.wbombo-previous-00000000-0000-0000-0000-000000000000" -Value 'old'
$p = Start-Process -FilePath $uninstaller -ArgumentList '/S', "_?=$installDir" -Wait -PassThru
Check ($p.ExitCode -eq 0) 'la desinstalación termina con código 0'
Check (-not (Test-Path -LiteralPath $exe)) 'la desinstalación quita el exe'
Check (-not (Get-ChildItem -LiteralPath $installDir -Filter '*.wbombo-*' -ErrorAction SilentlyContinue)) 'la desinstalación quita las copias del actualizador'
Check (-not (Test-Path -LiteralPath $uninstallKey)) 'la desinstalación quita la entrada del registro'
Check (-not (Test-Path -LiteralPath $startMenu) -and -not (Test-Path -LiteralPath $desktop)) 'la desinstalación quita los accesos directos'
Check ((Test-Path -LiteralPath $data) -eq $dataExisted) 'la desinstalación no toca los datos del launcher'
# With _?= the uninstaller runs in place and cannot delete itself.
Remove-Item -LiteralPath $uninstaller -Force
Remove-Item -LiteralPath $installDir -Force
Write-Host 'instalador verificado'
