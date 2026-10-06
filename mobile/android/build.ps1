[CmdletBinding()]
param(
    [string]$AndroidSdk = $env:ANDROID_HOME,
    [string]$JavaHome = $env:JAVA_HOME,
    [string]$OutputDirectory = (Join-Path $PSScriptRoot 'out')
)
$ErrorActionPreference = 'Stop'
$EngineCommit = '5e76d7485d6ca34fe2adf86b31156f2714f5ccd9'
if (!$AndroidSdk -or !(Test-Path -LiteralPath $AndroidSdk)) { throw 'Provide -AndroidSdk with an Android SDK containing platform 35, NDK 27.0.12077973 and CMake 3.22.1.' }
if (!$JavaHome -or !(Test-Path -LiteralPath (Join-Path $JavaHome 'bin/java.exe'))) { throw 'Provide -JavaHome pointing to JDK 21.' }
$engine = Join-Path $PSScriptRoot '.engine'
$patch = Join-Path $PSScriptRoot 'wachiland.patch'
$patchHash = (Get-FileHash -LiteralPath $patch -Algorithm SHA256).Hash
$marker = Join-Path $engine '.wachiland-build-source'
if (!(Test-Path -LiteralPath $engine)) {
    & git clone --no-checkout https://github.com/FCL-Team/FoldCraftLauncher.git $engine
    if ($LASTEXITCODE -ne 0) { throw 'Engine clone failed.' }
    & git -C $engine checkout --detach $EngineCommit
    if ($LASTEXITCODE -ne 0) { throw 'Pinned engine checkout failed.' }
    & git -C $engine apply --check $patch
    if ($LASTEXITCODE -ne 0) { throw 'Source patch does not match the pinned engine.' }
    & git -C $engine apply $patch
    if ($LASTEXITCODE -ne 0) { throw 'Applying source patch failed.' }
    [IO.File]::WriteAllText($marker, $patchHash)
} elseif (!(Test-Path -LiteralPath $marker) -or [IO.File]::ReadAllText($marker) -ne $patchHash) {
    throw 'The existing .engine checkout does not match this patch. Preserve it and use a fresh source directory.'
}

# Signing belongs to the builder, never to the people installing the APK.
# Keep these two files backed up to sign compatible updates. Never ship them.
$privateDirectory = Join-Path $env:LOCALAPPDATA 'WachilandBuild\private-signing'
[IO.Directory]::CreateDirectory($privateDirectory) | Out-Null
$keystore = Join-Path $privateDirectory 'android.jks'
$passwordFile = Join-Path $privateDirectory 'android.password'
if (!(Test-Path -LiteralPath $keystore)) {
    if (Test-Path -LiteralPath $passwordFile) { throw 'Password file exists without its signing key. Restore the key before continuing.' }
    $password = [Guid]::NewGuid().ToString('N') + [Guid]::NewGuid().ToString('N')
    [IO.File]::WriteAllText($passwordFile, $password)
    $env:FCL_KEYSTORE_PASSWORD = $password
    & (Join-Path $JavaHome 'bin/keytool.exe') -genkeypair -alias wachiland -keyalg RSA -keysize 4096 -validity 10000 -keystore $keystore -storepass:env FCL_KEYSTORE_PASSWORD -keypass:env FCL_KEYSTORE_PASSWORD -dname 'CN=Wachiland Android, O=Wachiland'
    if ($LASTEXITCODE -ne 0) { throw 'Signing key generation failed.' }
}
if (!(Test-Path -LiteralPath $passwordFile)) { throw 'Restore the password file for the existing signing key.' }
$env:FCL_KEYSTORE_PASSWORD = [IO.File]::ReadAllText($passwordFile)
$env:WACHILAND_KEYSTORE = $keystore
$env:JAVA_HOME = $JavaHome
$env:JAVA_TOOL_OPTIONS = '-Djava.net.preferIPv4Stack=true'
$env:OAUTH_API_KEY = 'e5226706-5096-431d-9516-ae48fe263401' # Public Microsoft client identifier, not a secret.
[IO.File]::WriteAllText((Join-Path $engine 'local.properties'), ('sdk.dir=' + $AndroidSdk.Replace('\','/') + "`narch=arm64`n"))
Push-Location $engine
try {
    & .\gradlew.bat :FCL:assembleDebug -Darch=arm64 --no-daemon --console=plain
    if ($LASTEXITCODE -ne 0) { throw 'Android build failed.' }
} finally {
    Pop-Location
    Remove-Item Env:FCL_KEYSTORE_PASSWORD -ErrorAction SilentlyContinue
}
[IO.Directory]::CreateDirectory($OutputDirectory) | Out-Null
$apk = Join-Path $OutputDirectory 'Wachiland-Launcher-Android-0.1.2-alpha-arm64.apk'
Copy-Item -LiteralPath (Join-Path $engine 'FCL/build/outputs/apk/debug/FCL-debug-0.1.2-alpha-arm64-v8a.apk') -Destination $apk
$hash = (Get-FileHash -LiteralPath $apk -Algorithm SHA256).Hash.ToLowerInvariant()
[IO.File]::WriteAllText((Join-Path $OutputDirectory 'SHA256.txt'), "$hash  $([IO.Path]::GetFileName($apk))`n")
Write-Output "APK: $apk"
