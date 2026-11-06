<#
.SYNOPSIS
  Install, reinstall over and uninstall the Windows installer silently, then
  check files, registry, shortcuts and PE metadata.

.DESCRIPTION
  Meant for disposable CI runners: it installs for the current user. Launcher
  data (%APPDATA%\WachilandLauncher) must not be created or removed by the
  installer; the script checks that too. Unsigned files only produce a warning.
#>
param(
    [Parameter(Mandatory = $true)][string]$Installer,
    [Parameter(Mandatory = $true)][string]$Launcher
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

function Check($condition, $message) {
    if (-not $condition) { throw "FALLO: $message" }
    Write-Host "ok: $message"
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

$dataExisted = Test-Path -LiteralPath $data
Check (-not (Test-Path -LiteralPath $installDir)) 'no había una instalación previa'

foreach ($round in 'instalación limpia', 'reinstalación encima') {
    $p = Start-Process -FilePath $Installer -ArgumentList '/S' -Wait -PassThru
    Check ($p.ExitCode -eq 0) "$round termina con código 0"
    Check (Test-Path -LiteralPath $exe) "$round deja WachilandLauncher.exe"
    Check ((Get-FileHash -LiteralPath $exe).Hash -eq (Get-FileHash -LiteralPath $Launcher).Hash) "$round instala el mismo exe"
    Check (Test-Path -LiteralPath $uninstaller) "$round deja uninstall.exe"
    $key = Get-ItemProperty -LiteralPath $uninstallKey
    Check ($key.DisplayName -eq $product -and $key.Publisher -eq 'Wachiland Elite') "$round registra la entrada de Aplicaciones instaladas"
    Check ($key.UninstallString -eq "`"$uninstaller`"") "$round registra el desinstalador"
    Check (Test-Path -LiteralPath $startMenu) "$round crea el acceso del menú Inicio"
    Check (Test-Path -LiteralPath $desktop) "$round crea el acceso del escritorio"
    $link = (New-Object -ComObject WScript.Shell).CreateShortcut($startMenu)
    Check ($link.TargetPath -eq $exe) "${round}: el acceso apunta al exe instalado"
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
