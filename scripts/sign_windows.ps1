<#
.SYNOPSIS
  Authenticode-sign one Windows file if a signing identity is configured.

.DESCRIPTION
  Used by scripts/build_windows.sh for the launcher exe and as cargo-packager's
  signCommand for the uninstaller and the installer. Configuration comes only
  from environment variables (GitHub secrets in CI); nothing is read from the
  repository. The first configured method wins:

  1. Azure Artifact Signing (formerly Trusted Signing)
       AZURE_ARTIFACT_SIGNING_ENDPOINT   e.g. https://weu.codesigning.azure.net
       AZURE_ARTIFACT_SIGNING_ACCOUNT    signing account name
       AZURE_ARTIFACT_SIGNING_PROFILE    certificate profile name
       AZURE_ARTIFACT_SIGNING_DLIB       optional path to Azure.CodeSigning.Dlib.dll
                                         (default: Microsoft.ArtifactSigning.Client from nuget.org)
       plus AZURE_CLIENT_ID / AZURE_CLIENT_SECRET / AZURE_TENANT_ID (or any
       other DefaultAzureCredential source).
  2. PFX file
       WINDOWS_SIGN_PFX_BASE64           base64 of the .pfx
       WINDOWS_SIGN_PFX_PASSWORD         its password
  3. Certificate already in the user store (hardware token, Certum SimplySign…)
       WINDOWS_SIGN_CERT_THUMBPRINT      SHA-1 thumbprint

  WINDOWS_SIGN_TIMESTAMP_URL overrides the RFC 3161 timestamp server for
  methods 2 and 3. With no method configured the file is left unsigned and a
  warning is printed; WINDOWS_SIGN_REQUIRED=1 turns that into an error.
  Already validly signed files are left untouched, so the script is idempotent.
#>
param([Parameter(Mandatory = $true)][string]$Path)

$ErrorActionPreference = 'Stop'
$Description = 'Wachiland Launcher'

if (-not (Test-Path -LiteralPath $Path -PathType Leaf)) {
    throw "[firma] no existe el archivo: $Path"
}
$Path = (Resolve-Path -LiteralPath $Path).Path
$name = Split-Path -Leaf $Path

$existing = Get-AuthenticodeSignature -LiteralPath $Path
if ($existing.Status -eq 'Valid') {
    Write-Host "[firma] $name ya está firmado por $($existing.SignerCertificate.Subject)"
    exit 0
}

function Find-SignTool {
    $onPath = Get-Command signtool.exe -ErrorAction SilentlyContinue
    if ($onPath) { return $onPath.Source }
    $kits = Join-Path ${env:ProgramFiles(x86)} 'Windows Kits\10\bin'
    $found = Get-ChildItem -Path $kits -Filter signtool.exe -Recurse -ErrorAction SilentlyContinue |
        Where-Object { $_.DirectoryName -like '*\x64' } |
        Sort-Object { [version]($_.Directory.Parent.Name -replace '[^0-9.]', '') } -Descending |
        Select-Object -First 1
    if (-not $found) { throw '[firma] no se encuentra signtool.exe (Windows SDK)' }
    return $found.FullName
}

$timestamp = if ($env:WINDOWS_SIGN_TIMESTAMP_URL) { $env:WINDOWS_SIGN_TIMESTAMP_URL } else { 'http://timestamp.digicert.com' }
$method = $null
$arguments = @()
$cleanup = @()

try {
    if ($env:AZURE_ARTIFACT_SIGNING_ENDPOINT -and $env:AZURE_ARTIFACT_SIGNING_ACCOUNT -and
        $env:AZURE_ARTIFACT_SIGNING_PROFILE) {
        $method = 'Azure Artifact Signing'
        if (-not $env:AZURE_ARTIFACT_SIGNING_DLIB) {
            # Microsoft's signtool plug-in from nuget.org, cached per machine.
            $client = Join-Path ([IO.Path]::GetTempPath()) 'wbombo-artifact-signing-client-1.0.128'
            $dlib = Join-Path $client 'bin\x64\Azure.CodeSigning.Dlib.dll'
            if (-not (Test-Path -LiteralPath $dlib)) {
                $package = "$client.zip"
                Invoke-WebRequest -UseBasicParsing -OutFile $package `
                    -Uri 'https://api.nuget.org/v3-flatcontainer/microsoft.artifactsigning.client/1.0.128/microsoft.artifactsigning.client.1.0.128.nupkg'
                Expand-Archive -LiteralPath $package -DestinationPath $client -Force
                Remove-Item -LiteralPath $package -Force
            }
            $env:AZURE_ARTIFACT_SIGNING_DLIB = $dlib
        }
        $metadata = Join-Path ([IO.Path]::GetTempPath()) ("wbombo-sign-" + [guid]::NewGuid() + '.json')
        @{
            Endpoint               = $env:AZURE_ARTIFACT_SIGNING_ENDPOINT
            CodeSigningAccountName = $env:AZURE_ARTIFACT_SIGNING_ACCOUNT
            CertificateProfileName = $env:AZURE_ARTIFACT_SIGNING_PROFILE
        } | ConvertTo-Json | Set-Content -LiteralPath $metadata -Encoding utf8
        $cleanup += $metadata
        $arguments = @('/tr', 'http://timestamp.acs.microsoft.com', '/td', 'SHA256',
            '/dlib', $env:AZURE_ARTIFACT_SIGNING_DLIB, '/dmdf', $metadata)
    } elseif ($env:WINDOWS_SIGN_PFX_BASE64) {
        $method = 'certificado PFX'
        $pfx = Join-Path ([IO.Path]::GetTempPath()) ("wbombo-sign-" + [guid]::NewGuid() + '.pfx')
        [IO.File]::WriteAllBytes($pfx, [Convert]::FromBase64String($env:WINDOWS_SIGN_PFX_BASE64))
        $cleanup += $pfx
        $arguments = @('/f', $pfx, '/p', $env:WINDOWS_SIGN_PFX_PASSWORD, '/tr', $timestamp, '/td', 'SHA256')
    } elseif ($env:WINDOWS_SIGN_CERT_THUMBPRINT) {
        $method = 'certificado del almacén'
        $arguments = @('/sha1', $env:WINDOWS_SIGN_CERT_THUMBPRINT, '/tr', $timestamp, '/td', 'SHA256')
    }

    if (-not $method) {
        $message = "[firma] AVISO: $name queda SIN FIRMAR (no hay certificado configurado). Windows SmartScreen lo mostrará como editor desconocido."
        if ($env:WINDOWS_SIGN_REQUIRED -eq '1') { throw $message }
        Write-Warning $message
        if ($env:GITHUB_ACTIONS -eq 'true') { Write-Host "::warning title=Sin firma::$name queda sin firmar: no hay certificado configurado." }
        exit 0
    }

    $signtool = Find-SignTool
    Write-Host "[firma] firmando $name con $method"
    # Output is not echoed verbatim: signtool never prints the password, but keep
    # the log minimal anyway.
    & $signtool sign /fd SHA256 /d $Description @arguments $Path | Out-Null
    if ($LASTEXITCODE -ne 0) { throw "[firma] signtool falló con código $LASTEXITCODE al firmar $name" }
    & $signtool verify /pa /q $Path | Out-Null
    if ($LASTEXITCODE -ne 0) { throw "[firma] la firma de $name no verifica" }
    $signed = Get-AuthenticodeSignature -LiteralPath $Path
    Write-Host "[firma] $name firmado: $($signed.SignerCertificate.Subject) (sello de tiempo: $([bool]$signed.TimeStamperCertificate))"
} finally {
    foreach ($file in $cleanup) { Remove-Item -LiteralPath $file -Force -ErrorAction SilentlyContinue }
}
