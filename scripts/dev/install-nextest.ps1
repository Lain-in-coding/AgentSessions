<#
.SYNOPSIS
Installs the pinned Windows x86_64 cargo-nextest into a user-local directory.

.DESCRIPTION
Downloads cargo-nextest 0.9.149 from the pinned GitHub release, verifies the
archive SHA-256, extracts it into a staging directory, and installs only the
verified executable under
<DestinationRoot>\nextest-0.9.149\bin\cargo-nextest.exe.

The default destination is %LOCALAPPDATA%\ASG\tools\nextest. The script is
idempotent: a second run validates the install manifest, executable SHA-256 and
exact version before returning success. It never changes PATH or global Cargo
configuration and never replaces an existing unverified installation.

.PARAMETER DestinationRoot
Overrides the installation root. CI uses this to keep the tool under its runner
temporary directory. The versioned installation directory and executable layout
remain the same.

.EXAMPLE
pwsh ./scripts/dev/install-nextest.ps1

Installs into the stable user-local root used by verify.ps1 auto-discovery.

.EXAMPLE
pwsh ./scripts/dev/install-nextest.ps1 -DestinationRoot "$env:RUNNER_TEMP\asg-tools\nextest"

Installs into an isolated root suitable for CI.
#>

[CmdletBinding()]
param(
    [string]$DestinationRoot
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$nextestVersion = '0.9.149'
$archiveName = 'cargo-nextest-0.9.149-x86_64-pc-windows-msvc.zip'
$archiveSha256 = 'bf904b85c7c5c2a29c340053a8d9d8e8027ff0d55a2bf7cef7d23032f8166438'
$executableSha256 = '4a9731975449492e2377dfc1d87a04a7a2ba8e4b8c5bda91a35b877ec3992898'
$downloadUrl = 'https://github.com/nextest-rs/nextest/releases/download/cargo-nextest-0.9.149/cargo-nextest-0.9.149-x86_64-pc-windows-msvc.zip'
$expectedHost = 'x86_64-pc-windows-msvc'
$installDirectoryName = "nextest-$nextestVersion"

if ([Environment]::OSVersion.Platform -ne [PlatformID]::Win32NT) {
    throw 'This installer supports Windows x86_64 only.'
}

$osArchitecture = $null
try {
    $osArchitecture = [System.Runtime.InteropServices.RuntimeInformation]::OSArchitecture.ToString()
} catch {
    $osArchitecture = $null
}
if (-not [string]::IsNullOrWhiteSpace($osArchitecture)) {
    if ($osArchitecture -ne 'X64') {
        throw "This installer supports Windows x86_64 only; detected $osArchitecture."
    }
} elseif ($env:PROCESSOR_ARCHITECTURE -ne 'AMD64') {
    throw "This installer supports Windows x86_64 only; detected $($env:PROCESSOR_ARCHITECTURE)."
}

if ([string]::IsNullOrWhiteSpace($DestinationRoot)) {
    if ([string]::IsNullOrWhiteSpace($env:LOCALAPPDATA)) {
        throw 'LOCALAPPDATA is not set; pass -DestinationRoot to choose an installation root.'
    }
    $DestinationRoot = Join-Path $env:LOCALAPPDATA 'ASG/tools/nextest'
}

$destinationRootFull = [System.IO.Path]::GetFullPath($DestinationRoot)
if (Test-Path -LiteralPath $destinationRootFull -PathType Leaf) {
    throw "Destination root is a file: $destinationRootFull"
}
$installPath = Join-Path $destinationRootFull $installDirectoryName
$executablePath = Join-Path $installPath 'bin/cargo-nextest.exe'
$manifestPath = Join-Path $installPath 'install.json'

function Get-FileSha256 {
    param([string]$Path)

    return (Get-FileHash -LiteralPath $Path -Algorithm SHA256).Hash
}

function Get-NextestIdentity {
    param([string]$Path)

    $identity = [pscustomobject]@{
        Valid = $false
        Detail = ''
        VersionOutput = ''
    }
    if (-not (Test-Path -LiteralPath $Path -PathType Leaf)) {
        $identity.Detail = 'file does not exist'
        return $identity
    }

    $output = @()
    $exitCode = 1
    try {
        $output = @(& $Path --version 2>&1)
        $exitCode = [int]$LASTEXITCODE
    } catch {
        $identity.Detail = $_.Exception.Message
        return $identity
    }

    $versionText = ($output | ForEach-Object { [string]$_ }) -join "`n"
    $identity.VersionOutput = $versionText
    if ($exitCode -ne 0) {
        $identity.Detail = "version command exited with code $exitCode"
        return $identity
    }
    if ($versionText -notmatch '(?im)^\s*cargo-nextest\s+0\.9\.149(?:\s|\()') {
        $identity.Detail = "expected cargo-nextest $nextestVersion"
        return $identity
    }
    if ($versionText -notmatch '(?im)^\s*host:\s*x86_64-pc-windows-msvc\s*$') {
        $identity.Detail = "expected host $expectedHost"
        return $identity
    }

    $identity.Valid = $true
    $identity.Detail = "cargo-nextest $nextestVersion ($expectedHost)"
    return $identity
}

function Read-InstallManifest {
    param([string]$Path)

    if (-not (Test-Path -LiteralPath $Path -PathType Leaf)) {
        throw "Install manifest is missing: $Path"
    }
    try {
        $manifest = Get-Content -LiteralPath $Path -Raw | ConvertFrom-Json
    } catch {
        throw "Install manifest is not valid JSON: $Path. $($_.Exception.Message)"
    }

    if ([string]$manifest.version -ne $nextestVersion) {
        throw "Install manifest version does not match ${nextestVersion}: $Path"
    }
    if ([string]$manifest.host -ne $expectedHost) {
        throw "Install manifest host does not match ${expectedHost}: $Path"
    }
    if ([string]$manifest.archive -ne $archiveName) {
        throw "Install manifest archive name does not match ${archiveName}: $Path"
    }
    if (([string]$manifest.archive_sha256).ToUpperInvariant() -ne $archiveSha256.ToUpperInvariant()) {
        throw "Install manifest archive SHA-256 does not match the pinned digest: $Path"
    }
    if ([string]$manifest.source_url -ne $downloadUrl) {
        throw "Install manifest source URL does not match the pinned release URL: $Path"
    }
    if ([string]$manifest.executable -ne 'bin/cargo-nextest.exe') {
        throw "Install manifest executable path is invalid: $Path"
    }
    if (([string]$manifest.executable_sha256).ToUpperInvariant() -ne $executableSha256.ToUpperInvariant()) {
        throw "Install manifest executable SHA-256 does not match the pinned digest: $Path"
    }
    return $manifest
}

function Assert-InstalledNextest {
    param(
        [string]$Executable,
        [string]$Manifest
    )

    $identity = Get-NextestIdentity -Path $Executable
    if (-not $identity.Valid) {
        throw "Installed cargo-nextest is unusable at '$Executable': $($identity.Detail)"
    }

    $manifestData = Read-InstallManifest -Path $Manifest
    $actualExecutableSha256 = Get-FileSha256 -Path $Executable
    if ($actualExecutableSha256.ToUpperInvariant() -ne ([string]$manifestData.executable_sha256).ToUpperInvariant()) {
        throw "Installed cargo-nextest SHA-256 does not match its verified install manifest: $Executable"
    }
    return $identity
}

function Invoke-PinnedDownload {
    param(
        [string]$Uri,
        [string]$OutFile
    )

    $curlCommand = Get-Command -Name curl.exe -CommandType Application -ErrorAction SilentlyContinue
    $curlFailure = $null
    if ($null -ne $curlCommand) {
        & $curlCommand.Source --fail --location --silent --show-error --retry 3 --retry-delay 1 --output $OutFile $Uri
        $curlExitCode = $LASTEXITCODE
        if ($curlExitCode -eq 0) {
            return
        }
        $curlFailure = "curl.exe failed to download $Uri with exit code $curlExitCode"
        if (Test-Path -LiteralPath $OutFile) {
            Remove-Item -LiteralPath $OutFile -Force
        }
    }

    try {
        Write-Host 'nextest: retrying download with Invoke-WebRequest'
        Invoke-WebRequest -Uri $Uri -OutFile $OutFile -UseBasicParsing -TimeoutSec 30
        return
    } catch {
        $webRequestFailure = $_.Exception.Message
        if ($null -ne $curlFailure) {
            throw "$curlFailure; Invoke-WebRequest also failed: $webRequestFailure"
        }
        throw
    }
}

if (Test-Path -LiteralPath $installPath) {
    if (-not (Test-Path -LiteralPath $executablePath -PathType Leaf)) {
        throw "An incomplete cargo-nextest installation already exists at '$installPath'; remove it explicitly before retrying."
    }
    $existingIdentity = Assert-InstalledNextest -Executable $executablePath -Manifest $manifestPath
    Write-Host "nextest: verified existing installation ($($existingIdentity.Detail)): $executablePath"
    return
}

if (-not (Test-Path -LiteralPath $destinationRootFull -PathType Container)) {
    New-Item -ItemType Directory -Path $destinationRootFull -Force | Out-Null
}

$stagingRoot = Join-Path $destinationRootFull ('.install-nextest-' + [Guid]::NewGuid().ToString('N'))
$archivePath = Join-Path $stagingRoot $archiveName
$extractPath = Join-Path $stagingRoot 'payload'
$stagedInstallPath = Join-Path $stagingRoot $installDirectoryName
$stagedExecutablePath = Join-Path $stagedInstallPath 'bin/cargo-nextest.exe'
$stagedManifestPath = Join-Path $stagedInstallPath 'install.json'
try {
    New-Item -ItemType Directory -Path $extractPath -Force | Out-Null
    New-Item -ItemType Directory -Path (Split-Path -Parent $stagedExecutablePath) -Force | Out-Null

    $ProgressPreference = 'SilentlyContinue'
    $securityProtocol = [Net.ServicePointManager]::SecurityProtocol
    if (($securityProtocol -band [Net.SecurityProtocolType]::Tls12) -eq 0) {
        [Net.ServicePointManager]::SecurityProtocol = $securityProtocol -bor [Net.SecurityProtocolType]::Tls12
    }

    Write-Host "nextest: downloading $archiveName"
    Invoke-PinnedDownload -Uri $downloadUrl -OutFile $archivePath

    $actualArchiveSha256 = Get-FileSha256 -Path $archivePath
    if ($actualArchiveSha256.ToUpperInvariant() -ne $archiveSha256.ToUpperInvariant()) {
        throw "Archive SHA-256 mismatch. Expected $archiveSha256, got $actualArchiveSha256."
    }

    Expand-Archive -LiteralPath $archivePath -DestinationPath $extractPath -Force
    $extractedExecutables = @(Get-ChildItem -LiteralPath $extractPath -Recurse -File -Filter 'cargo-nextest.exe')
    if ($extractedExecutables.Count -ne 1) {
        throw "Expected exactly one cargo-nextest.exe in $archiveName; found $($extractedExecutables.Count)."
    }

    Copy-Item -LiteralPath $extractedExecutables[0].FullName -Destination $stagedExecutablePath
    $stagedIdentity = Get-NextestIdentity -Path $stagedExecutablePath
    if (-not $stagedIdentity.Valid) {
        throw "Extracted cargo-nextest is unusable: $($stagedIdentity.Detail)"
    }

    $stagedExecutableSha256 = Get-FileSha256 -Path $stagedExecutablePath
    if ($stagedExecutableSha256.ToUpperInvariant() -ne $executableSha256.ToUpperInvariant()) {
        throw "Extracted cargo-nextest SHA-256 mismatch. Expected $executableSha256, got $stagedExecutableSha256."
    }
    $installManifest = [ordered]@{
        version = $nextestVersion
        host = $expectedHost
        archive = $archiveName
        archive_sha256 = $archiveSha256
        source_url = $downloadUrl
        executable = 'bin/cargo-nextest.exe'
        executable_sha256 = $stagedExecutableSha256
    }
    $installManifest | ConvertTo-Json | Set-Content -LiteralPath $stagedManifestPath -Encoding UTF8
    $stagedIdentity = Assert-InstalledNextest -Executable $stagedExecutablePath -Manifest $stagedManifestPath

    if (Test-Path -LiteralPath $installPath) {
        throw "Another process created '$installPath' while the installer was running; the verified staged copy was not moved."
    }
    Move-Item -LiteralPath $stagedInstallPath -Destination $installPath
    $installedIdentity = Assert-InstalledNextest -Executable $executablePath -Manifest $manifestPath
    Write-Host "nextest: installed and verified ($($installedIdentity.Detail)): $executablePath"
}
finally {
    if (Test-Path -LiteralPath $stagingRoot) {
        $stagingRootFull = [System.IO.Path]::GetFullPath($stagingRoot)
        $destinationPrefix = $destinationRootFull.TrimEnd('\', '/') + [System.IO.Path]::DirectorySeparatorChar
        $stagingLeaf = [System.IO.Path]::GetFileName($stagingRootFull)
        if ($stagingRootFull.StartsWith($destinationPrefix, [System.StringComparison]::OrdinalIgnoreCase) -and
            $stagingLeaf.StartsWith('.install-nextest-', [System.StringComparison]::Ordinal)) {
            Remove-Item -LiteralPath $stagingRootFull -Recurse -Force
        } else {
            Write-Warning "Refusing to clean unexpected staging path: $stagingRootFull"
        }
    }
}



